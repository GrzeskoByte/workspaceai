//! HTTP client for the supervisor: every request carries the `x-harness-token` header.

use crate::supervisor::Info;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// HTTP status, or 0 when the supervisor could not be reached.
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.status)
    }
}

#[derive(Debug, Clone)]
pub struct Api {
    pub base: String,
    pub token: String,
}

impl Api {
    pub fn new(info: &Info) -> Api {
        Api { base: format!("http://127.0.0.1:{}", info.port), token: info.token.clone() }
    }

    pub fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, ApiError> {
        let req = ureq::request(method, &format!("{}{path}", self.base)).set("x-harness-token", &self.token);
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        match res {
            Ok(r) => {
                let text = r.into_string().map_err(|e| ApiError { status: 0, message: e.to_string() })?;
                Ok(if text.is_empty() { Value::Object(Default::default()) } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) })
            }
            Err(ureq::Error::Status(status, r)) => {
                let text = r.into_string().unwrap_or_default();
                let message = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["err"].as_str().map(str::to_string)).unwrap_or(text);
                Err(ApiError { status, message })
            }
            Err(e) => Err(ApiError { status: 0, message: e.to_string() }),
        }
    }

    pub fn get(&self, path: &str) -> Result<Value, ApiError> {
        self.request("GET", path, None)
    }
    pub fn post(&self, path: &str, body: Option<&Value>) -> Result<Value, ApiError> {
        self.request("POST", path, Some(body.unwrap_or(&Value::Object(Default::default()))))
    }
    pub fn patch(&self, path: &str, body: &Value) -> Result<Value, ApiError> {
        self.request("PATCH", path, Some(body))
    }
    pub fn del(&self, path: &str) -> Result<Value, ApiError> {
        self.request("DELETE", path, None)
    }
}

/// Percent-encodes everything but unreserved characters (RFC 3986), byte by byte so non-ASCII is safe.
pub fn pct(s: &str) -> String {
    s.bytes().fold(String::new(), |mut out, b| {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
        out
    })
}

/// What an id that cannot be a path segment turns into. A real id never contains a space, so the server answers
/// "not found" instead of routing the request somewhere else.
pub const INVALID_ID: &str = "invalid%20id";

/// One id as a single URL path segment. Ids come from the server (and from files it reads), so a `/`, `?` or `#` is
/// encoded, and ids that no encoding can make safe (`.`, `..`, empty) become `INVALID_ID`: `%2E%2E` is still a dot
/// segment to many HTTP stacks, and an empty segment would turn `/facts/{id}` into the bulk `/facts` route.
pub fn seg(id: &str) -> String {
    if id.is_empty() || id == "." || id == ".." {
        return INVALID_ID.to_string();
    }
    pct(id)
}

impl Api {
    /// Streams a GET to a new file in `dir` (named by the reply's Content-Disposition, never overwriting) and returns it.
    pub fn download_to(&self, path: &str, dir: &std::path::Path) -> Result<std::path::PathBuf, ApiError> {
        let res = match ureq::get(&format!("{}{path}", self.base)).set("x-harness-token", &self.token).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(status, r)) => return Err(ApiError { status, message: r.into_string().unwrap_or_default() }),
            Err(e) => return Err(ApiError { status: 0, message: e.to_string() }),
        };
        let name = res.header("content-disposition").and_then(crate::spaces::filename_from_disposition).unwrap_or_else(|| "export".into());
        let target = crate::spaces::unique_path(dir, &name, |p| p.exists());
        let io = |e: std::io::Error| ApiError { status: 0, message: format!("Could not save {}: {e}", target.display()) };
        let mut file = std::fs::File::create(&target).map_err(io)?;
        std::io::copy(&mut res.into_reader(), &mut file).map_err(io)?;
        Ok(target)
    }
}
