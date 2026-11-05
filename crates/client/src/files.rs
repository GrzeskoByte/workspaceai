//! `@` file references: the project file search the composer calls while a mention is being typed.

use serde_json::Value;

/// Paths from `{"files": [...]}`; anything else is skipped.
pub fn parse_files(reply: &Value) -> Vec<String> {
    reply["files"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// Percent-encodes everything but unreserved characters (RFC 3986), byte by byte so non-ASCII is safe.
fn encode(s: &str) -> String {
    s.bytes().fold(String::new(), |mut out, b| {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
        out
    })
}

pub fn search_path(workspace: &str, project: &str, query: &str) -> String {
    format!("/api/ws/{workspace}/projects/{project}/files?q={}", encode(query))
}

/// Search again only when a mention is being typed and its query differs from the last one searched.
pub fn needs_search(last: Option<&str>, current: Option<&str>) -> bool {
    match current {
        None => false,
        Some(q) => last != Some(q),
    }
}
