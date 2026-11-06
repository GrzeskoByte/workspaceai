//! Port of `OpencodeClient`: blocking JSON HTTP against a workspace's `opencode serve`, with Basic auth.
//! Requests never carry cookies or follow redirects beyond ureq's default; every call has a timeout.

use base64::Engine as _;
use serde_json::{json, Value};
use std::fmt;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpencodeError {
    pub status: u16,
    pub message: String,
}

impl OpencodeError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        OpencodeError { status, message: message.into() }
    }
}

impl fmt::Display for OpencodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OpencodeError {}

/// Query values are strings here (numbers stringified by the caller), as `URLSearchParams` does.
pub type Param = (&'static str, String);

#[derive(Debug, Clone, Default)]
pub struct Info {
    pub version: String,
    pub pid: i64,
    pub urls: Vec<String>,
}

#[derive(Clone)]
pub struct OpencodeClient {
    url: String,
    auth: String,
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl OpencodeClient {
    pub fn new(url: &str, username: &str, password: &str) -> Self {
        let auth = base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
        OpencodeClient { url: url.trim_end_matches('/').to_string(), auth: format!("Basic {auth}") }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    fn build_url(&self, path: &str, query: &[Param]) -> String {
        let mut url = format!("{}{path}", self.url);
        let params: Vec<String> = query.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("{k}={}", escape(v))).collect();
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        url
    }

    pub fn req(&self, method: &str, path: &str, body: Option<&Value>, query: &[Param], timeout: Duration) -> Result<Value, OpencodeError> {
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        let request = agent.request(method, &self.build_url(path, query)).set("authorization", &self.auth).set("accept", "application/json");
        let res = match body {
            Some(b) => request.set("content-type", "application/json").send_string(&b.to_string()),
            None => request.call(),
        };
        let (status, text) = match res {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
            Err(e) => return Err(OpencodeError::new(400, format!("{method} {path} → {e}"))),
        };
        if !(200..300).contains(&status) {
            return Err(OpencodeError::new(status, format!("{method} {path} → {status} {}", text.chars().take(300).collect::<String>())));
        }
        let trimmed = text.trim_start();
        if trimmed.starts_with('<') {
            return Err(OpencodeError::new(502, format!("{path} returned HTML; is this an opencode v2 server?")));
        }
        if text.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&text).map_err(|e| OpencodeError::new(502, format!("{path} returned invalid JSON: {e}")))
    }

    fn get(&self, path: &str, query: &[Param], timeout: Duration) -> Result<Value, OpencodeError> {
        self.req("GET", path, None, query, timeout)
    }

    pub fn info(&self) -> Result<Info, OpencodeError> {
        let v = self.get("/api/info", &[], Duration::from_secs(5))?;
        Ok(Info {
            version: v["version"].as_str().unwrap_or_default().to_string(),
            pid: v["pid"].as_i64().unwrap_or(0),
            urls: v["urls"].as_array().map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        })
    }

    /// Up to 50 pages of 200, as the TS does, stopping at `limit`.
    pub fn list_sessions(&self, directory: Option<&str>, limit: usize) -> Result<Vec<Value>, OpencodeError> {
        let mut out: Vec<Value> = Vec::new();
        let mut cursor = String::new();
        for _ in 0..50 {
            let query: Vec<Param> = vec![
                ("directory", directory.unwrap_or_default().to_string()),
                ("limit", limit.max(200).to_string()),
                ("cursor", cursor.clone()),
            ];
            let page = self.get("/api/session", &query, Duration::from_secs(30))?;
            if let Some(data) = page["data"].as_array() {
                out.extend(data.iter().cloned());
            }
            let next = match &page["cursor"]["next"] {
                Value::Null => String::new(),
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if next.is_empty() || out.len() >= limit {
                break;
            }
            cursor = next;
        }
        out.truncate(limit);
        Ok(out)
    }

    pub fn models(&self, directory: Option<&str>) -> Result<Vec<Value>, OpencodeError> {
        let query: Vec<Param> = vec![("location", directory.unwrap_or_default().to_string())];
        Ok(self.get("/api/model", &query, Duration::from_secs(30))?["data"].as_array().cloned().unwrap_or_default())
    }

    pub fn providers(&self) -> Result<Vec<Value>, OpencodeError> {
        Ok(self.get("/api/provider", &[], Duration::from_secs(30))?["data"].as_array().cloned().unwrap_or_default())
    }

    pub fn mcp(&self) -> Result<Vec<Value>, OpencodeError> {
        Ok(self.get("/api/mcp", &[], Duration::from_secs(30))?["data"].as_array().cloned().unwrap_or_default())
    }

    /// Reads `/api/event` until `stop` is set or the stream closes; malformed events are skipped.
    /// No consumers yet: batch 5 (session model) is the first.
    pub fn events(&self, mut on_event: impl FnMut(Value), stop: &AtomicBool) -> Result<(), OpencodeError> {
        let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).build();
        let res = agent
            .get(&self.build_url("/api/event", &[]))
            .set("authorization", &self.auth)
            .set("accept", "text/event-stream")
            .call()
            .map_err(|e| OpencodeError::new(400, format!("event stream {e}")))?;
        let mut data = String::new();
        for line in BufReader::new(res.into_reader()).lines() {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let Ok(line) = line else { break };
            if let Some(rest) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start());
                continue;
            }
            if line.is_empty() && !data.is_empty() {
                if let Ok(value) = serde_json::from_str::<Value>(&data) {
                    on_event(value);
                }
                data.clear();
            }
        }
        Ok(())
    }
}
