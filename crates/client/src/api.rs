//! The app's view of the backend: every request is a call into the in-process `Supervisor::handle`.

use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use workspaceai_supervisor::Supervisor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// HTTP-style status (404 unknown route, 400 bad request, ...).
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.status)
    }
}

#[derive(Clone)]
pub struct Api {
    pub sup: Arc<Supervisor>,
}

/// Inverse of `pct`: `%XX` back to bytes (invalid escapes stay as typed), lossy UTF-8.
pub fn unpct(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%' && i + 2 < b.len()).then(|| std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())).flatten();
        match hex {
            Some(v) => {
                out.push(v);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `/a/b%20c?x=1&y=2` to the decoded path and query map the router takes.
pub fn split_request(path: &str) -> (String, BTreeMap<String, String>) {
    let (p, q) = path.split_once('?').unwrap_or((path, ""));
    // A decoded segment that is not one safe path segment (`%2F`, `..`) must not re-split into other routes.
    let path = p.split('/').map(|s| { let d = unpct(s); if d.contains('/') || d == ".." || d == "." { INVALID_ID.to_string() } else { d } }).collect::<Vec<_>>().join("/");
    let query = q.split('&').filter(|kv| !kv.is_empty()).map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (unpct(&k.replace('+', " ")), unpct(&v.replace('+', " ")))
    }).collect();
    (path, query)
}

impl Api {
    pub fn new(sup: Arc<Supervisor>) -> Api {
        Api { sup }
    }

    /// Sends every supervisor event to `tx` (as many times as called; one subscription per window).
    pub fn events(&self, tx: std::sync::mpsc::Sender<crate::types::UiEvent>) {
        let _ = self.sup.with_core(|c| c.on(Arc::new(move |e| { let _ = tx.send(crate::types::UiEvent::from_supervisor(e)); })));
    }

    pub fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, ApiError> {
        let (p, query) = split_request(path);
        self.sup.handle(method, &p, &query, body.cloned()).map_err(|e| ApiError { status: e.status, message: e.message })
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
    /// Saves a file route's answer to a new file in `dir` (never overwriting) and returns it. Only the workspace
    /// export answers bytes; the artifact exports arrive with the artifacts batch.
    pub fn download_to(&self, path: &str, dir: &std::path::Path) -> Result<std::path::PathBuf, ApiError> {
        let (p, _) = split_request(path);
        let segs: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
        let ["api", "ws", id, "export"] = segs.as_slice() else {
            return Err(ApiError { status: 404, message: "That export is not available yet.".into() });
        };
        let archive = self.sup.export_workspace(id).map_err(|e| ApiError { status: e.status, message: e.message })?;
        let target = crate::spaces::unique_path(dir, &archive.filename, |p| p.exists());
        let io = |e: std::io::Error| ApiError { status: 0, message: format!("Could not save {}: {e}", target.display()) };
        std::fs::create_dir_all(dir).map_err(io)?;
        std::fs::write(&target, &archive.bytes).map_err(io)?;
        Ok(target)
    }
}
