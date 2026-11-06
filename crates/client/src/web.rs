//! What the embedded web view shows: artifact pages (sandboxed), and the browser panel's addresses and dev server.

use crate::api::seg;
use serde::Deserialize;
use serde_json::{json, Value};

/// The page Open Design stores for a version (`html` in the content reply); empty when there is none.
pub fn page_of(reply: &Value) -> String {
    reply["html"].as_str().unwrap_or("").to_string()
}

/// The document an artifact shows in the web view, or `None` when the app draws it natively: Markdown (not a
/// deck) and CSV are drawn natively, like the web app; everything else is the Open Design page, or the source
/// itself for HTML when there is no page.
pub fn web_doc(format: &str, kind: &str, content: &str, page: &str) -> Option<String> {
    if format == "csv" || (format == "markdown" && kind != "Deck") {
        return None;
    }
    if !page.trim().is_empty() {
        return Some(page.to_string());
    }
    (format == "html").then(|| content.to_string())
}

/// Whether the Desktop / Mobile toggle applies (pages, not decks).
pub fn has_device_toggle(kind: &str) -> bool {
    kind != "Deck"
}

/// Esc inside the sandboxed page would never reach the app, so the page passes it up (the host forwards it).
const ESC_RELAY: &str = r#"<script>addEventListener("keydown",function(e){if(e.key==="Escape"){e.preventDefault();parent.postMessage({harness:"leave-fullscreen"},"*")}},true)</script>"#;

/// The host page loaded into the web view. The artifact runs in an `<iframe sandbox="allow-scripts">` (no
/// `allow-same-origin`), so it gets an opaque origin: scripts run, but it cannot reach the host or the app. The
/// host only forwards the Esc message to the app's IPC handler. `mobile` narrows the frame to a phone width.
pub fn host_page(doc: &str, deck: bool, mobile: bool) -> String {
    let doc = if doc.contains("</body>") {
        let at = doc.rfind("</body>").unwrap_or(doc.len());
        format!("{}{ESC_RELAY}{}", &doc[..at], &doc[at..])
    } else {
        format!("{doc}{ESC_RELAY}")
    };
    let frame = if deck {
        "width:100%;aspect-ratio:16/9;max-height:100%;height:auto"
    } else if mobile {
        "width:390px;max-width:100%;height:100%"
    } else {
        "width:100%;height:100%"
    };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><style>html,body{{margin:0;height:100%;background:#1b1b1f;display:flex;align-items:center;justify-content:center;overflow:hidden}}iframe{{display:block;border:0;background:#fff;{frame}}}</style></head><body><iframe sandbox="allow-scripts" srcdoc="{}"></iframe><script>function leave(){{try{{window.ipc.postMessage("leave-fullscreen")}}catch(e){{}}}}addEventListener("message",function(e){{if(e.source===document.querySelector("iframe").contentWindow&&e.data&&e.data.harness==="leave-fullscreen")leave()}});addEventListener("keydown",function(e){{if(e.key==="Escape")leave()}})</script></body></html>"#,
        attr_escape(&doc)
    )
}

fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// True when the URL text (no scheme) really points at this machine. The authority is everything before the first
/// `/ ? # \`; one with userinfo (`@`) is never local, so `localhost:80@evil.com` cannot pass as a local preview.
fn is_local_authority(addr: &str) -> bool {
    let auth = addr.split(['/', '?', '#', '\\']).next().unwrap_or("");
    if auth.contains('@') {
        return false;
    }
    let host = match auth.strip_prefix('[') {
        Some(v6) => match v6.split_once(']') {
            Some((h, rest)) if rest.is_empty() || rest.starts_with(':') => return h == "::1",
            _ => return false,
        },
        None => auth.split(':').next().unwrap_or(""),
    };
    matches!(host, "localhost" | "127.0.0.1" | "0.0.0.0") || host.ends_with(".localhost")
}

/// What was typed, as a URL: "5173" → localhost:5173, "localhost:3000/x" → http://…, "example.com" → https://….
/// Only http and https URLs come out: a typed `file:` or `javascript:` address is prefixed with `https://` too.
pub fn to_url(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    if (2..=5).contains(&t.len()) && t.chars().all(|c| c.is_ascii_digit()) {
        return format!("http://localhost:{t}/");
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return t.to_string();
    }
    if is_local_authority(&lower) {
        format!("http://{t}")
    } else {
        format!("https://{t}")
    }
}

/// The first local address in a message or tool output, offered as "Preview …" under it.
pub fn local_url_in(text: &str) -> Option<String> {
    for scheme in ["http://", "https://"] {
        let mut from = 0;
        while let Some(i) = text[from..].find(scheme) {
            let start = from + i;
            let rest = &text[start + scheme.len()..];
            // `]` ends a link in prose, but not inside an IPv6 host like [::1].
            let v6 = rest.starts_with('[');
            let end = rest.find(|c: char| c.is_whitespace() || "'\"`)<>".contains(c) || (c == ']' && !v6)).unwrap_or(rest.len());
            let addr = &rest[..end];
            if is_local_authority(addr) {
                let url = format!("{scheme}{}", addr.replacen("0.0.0.0", "localhost", 1));
                return Some(url.trim_end_matches(['.', ',', ';', ':']).to_string());
            }
            from = start + scheme.len();
        }
    }
    None
}

/// "localhost:5173" from "http://localhost:5173/".
pub fn short_url(url: &str) -> String {
    url.trim_start_matches("http://").trim_start_matches("https://").trim_end_matches('/').to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Server {
    #[serde(default)]
    pub port: u32,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub process: String,
    #[serde(default)]
    pub pid: u32,
}

/// A project's dev server, from `GET …/projects/:pid/dev`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Dev {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub log: Vec<String>,
    #[serde(default)]
    pub exit: Option<i64>,
    #[serde(default)]
    pub servers: Vec<Server>,
}

pub fn parse_dev(v: &Value) -> Option<Dev> {
    serde_json::from_value(v.clone()).ok()
}

pub fn dev_path(workspace: &str, project: &str) -> String {
    format!("/api/ws/{}/projects/{}/dev", seg(workspace), seg(project))
}

pub fn start_body(command: &str) -> Option<Value> {
    let c = command.trim();
    (!c.is_empty()).then(|| json!({ "action": "start", "command": c }))
}

pub fn stop_body() -> Value {
    json!({ "action": "stop" })
}

/// The chips under the dev row: the server started here first, then other servers in the project. `(url, label)`.
pub fn server_chips(d: &Dev) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if !d.url.is_empty() {
        out.push((d.url.clone(), short_url(&d.url)));
    }
    for s in &d.servers {
        if d.url.is_empty() || !d.url.contains(&format!(":{}", s.port)) {
            out.push((s.url.clone(), format!("localhost:{}", s.port)));
        }
    }
    out
}
