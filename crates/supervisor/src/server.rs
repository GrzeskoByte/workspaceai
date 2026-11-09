//! Loopback HTTP listener for external callers only (plan (e)): the MCP servers, routine webhooks and shared links.
//! The app itself calls `Supervisor::handle` in-process.
//!
//! - binds 127.0.0.1 only, on a free port;
//! - per-launch random token, sent as `x-harness-token`, compared in constant time;
//! - only allowlisted paths are routed; everything else is 404 (after the token check, so nothing is enumerable);
//! - request bodies are capped at `MAX_BODY`.
//!
//! The URL and token reach agents' MCP servers through the environment only (`HARNESS_SUPERVISOR`,
//! `HARNESS_SUPERVISOR_TOKEN`), never argv or config files: configs carry `{env:…}` references.

use crate::supervisor::{ApiError, Supervisor};
use base64::Engine;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock, Weak};

/// Same cap as the TS server (artifacts are whole HTML documents).
pub const MAX_BODY: usize = 20_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackInfo {
    pub url: String,
    pub token: String,
}

/// Shared by the core and every runtime: set while a listener runs.
pub type LoopbackSlot = Arc<RwLock<Option<LoopbackInfo>>>;

pub fn loopback_info(slot: &LoopbackSlot) -> Option<LoopbackInfo> {
    slot.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// `n` random bytes as base64url (no padding).
pub fn random_token(n: usize) -> String {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).expect("OS random source");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// Constant-time equality for equal-length inputs; a length mismatch returns early (the length is not secret).
pub fn token_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A running listener. Dropping it stops accepting and clears the published URL/token.
pub struct LoopbackServer {
    pub addr: SocketAddr,
    pub token: String,
    server: Arc<tiny_http::Server>,
    slot: LoopbackSlot,
}

impl LoopbackServer {
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for LoopbackServer {
    fn drop(&mut self) {
        self.server.unblock();
        *self.slot.write().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Starts the listener and publishes its URL and token to every workspace (new engine/session envs pick them up).
/// Call it before engines start, so their MCP servers get the env.
pub fn serve(sup: &Arc<Supervisor>) -> Result<LoopbackServer, ApiError> {
    let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").map_err(|e| ApiError::new(500, format!("Loopback listener: {e}")))?);
    let addr = server.server_addr().to_ip().ok_or_else(|| ApiError::new(500, "Loopback listener has no IP address"))?;
    let token = random_token(24);
    let slot = sup.with_core(|c| c.loopback.clone())?;
    *slot.write().unwrap_or_else(|e| e.into_inner()) = Some(LoopbackInfo { url: format!("http://{addr}"), token: token.clone() });
    let (weak, srv, tok) = (Arc::downgrade(sup), server.clone(), token.clone());
    std::thread::Builder::new().name("loopback".into()).spawn(move || accept(srv, weak, tok))?;
    Ok(LoopbackServer { addr, token, server, slot })
}

fn accept(server: Arc<tiny_http::Server>, sup: Weak<Supervisor>, token: String) {
    for mut req in server.incoming_requests() {
        let Some(sup) = sup.upgrade() else { break };
        let token = token.clone();
        // ponytail: one thread per request, unbounded; the callers are a handful of local MCP servers.
        let _ = std::thread::Builder::new().name("loopback-req".into()).spawn(move || {
            let (status, ctype, body, extra) = route(&sup, &token, &mut req);
            let mut res = tiny_http::Response::from_data(body).with_status_code(status);
            for (k, v) in std::iter::once(("content-type", ctype)).chain(extra) {
                if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
                    res.add_header(h);
                }
            }
            let _ = req.respond(res);
        });
    }
}

type Answer = (u16, &'static str, Vec<u8>, Vec<(&'static str, &'static str)>);

fn json_answer(status: u16, v: &Value) -> Answer {
    (status, "application/json", v.to_string().into_bytes(), vec![])
}

fn err(e: &ApiError) -> Answer {
    json_answer(e.status, &e.body())
}

/// The path prefixes external callers may reach (plan (e)). `/facts` is left out on purpose: it can add approved
/// facts, and agents may only suggest (R-22); the memory server writes suggestion files itself.
fn allowed(method: &str, segs: &[&str]) -> bool {
    match segs {
        ["api", "ws", _, "agent", _, ..] => true,
        ["api", "ws", _, "memory-changed"] => method == "POST",
        ["api", "ws", _, "snapshot"] => method == "GET",
        ["api", "ws", _, "artifacts", _, "content"] => method == "GET",
        ["api", "hooks", _, _] => method == "POST",
        _ => false,
    }
}

/// Token-less paths: shared links (unguessable key) and webhooks (per-routine secret, checked by the handler).
fn exempt(segs: &[&str]) -> bool {
    matches!(segs, ["api", "shared", _, _] | ["api", "hooks", _, _])
}

fn route(sup: &Supervisor, token: &str, req: &mut tiny_http::Request) -> Answer {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let method = req.method().as_str().to_ascii_uppercase();
    if !exempt(&segs) {
        let given = req.headers().iter().find(|h| h.field.equiv("x-harness-token")).map(|h| h.value.as_bytes().to_vec());
        if !given.is_some_and(|g| token_eq(&g, token.as_bytes())) {
            return json_answer(401, &serde_json::json!({ "error": "unauthorized" }));
        }
    }
    // Encoded or dot segments never reach the router: ids are plain path-safe strings.
    if path.contains('%') || segs.iter().any(|s| *s == "." || *s == "..") {
        return err(&ApiError::not_found("Not found"));
    }
    if let ["api", "shared", ws, key] = segs.as_slice() {
        if method != "GET" {
            return err(&ApiError::not_found("Not found"));
        }
        return match sup.shared_page(ws, key) {
            Some(page) => (
                200,
                "text/html; charset=utf-8",
                page,
                vec![("content-security-policy", "sandbox allow-scripts; default-src 'self' 'unsafe-inline' data: blob:"), ("x-content-type-options", "nosniff")],
            ),
            None => (404, "text/plain", b"This link was removed".to_vec(), vec![]),
        };
    }
    if !allowed(&method, &segs) {
        return err(&ApiError::not_found(format!("No route {method} {path}")));
    }
    let body = if method == "GET" || method == "DELETE" {
        None
    } else {
        match read_body(req) {
            Ok(b) => b,
            Err(e) => return err(&e),
        }
    };
    let query: BTreeMap<String, String> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    match sup.handle(&method, path, &query, body) {
        Ok(v) => json_answer(200, &v),
        Err(e) => err(&e),
    }
}

fn read_body(req: &mut tiny_http::Request) -> Result<Option<Value>, ApiError> {
    if req.body_length().is_some_and(|n| n > MAX_BODY) {
        return Err(ApiError::new(413, "Body too large"));
    }
    let mut buf = Vec::new();
    req.as_reader().take(MAX_BODY as u64 + 1).read_to_end(&mut buf)?;
    if buf.len() > MAX_BODY {
        return Err(ApiError::new(413, "Body too large"));
    }
    if buf.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(None);
    }
    serde_json::from_slice(&buf).map(Some).map_err(|_| ApiError::bad_request("Invalid JSON"))
}
