//! `@` file references: the project file search the composer calls while a mention is being typed.

use crate::api::{pct, seg};
use serde_json::Value;

/// Paths from `{"files": [...]}`; anything else is skipped.
pub fn parse_files(reply: &Value) -> Vec<String> {
    reply["files"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

pub fn search_path(workspace: &str, project: &str, query: &str) -> String {
    format!("/api/ws/{}/projects/{}/files?q={}", seg(workspace), seg(project), pct(query))
}

/// Search again only when a mention is being typed and its query differs from the last one searched.
pub fn needs_search(last: Option<&str>, current: Option<&str>) -> bool {
    match current {
        None => false,
        Some(q) => last != Some(q),
    }
}

/// The `@` reference for a picked or dropped file (full path; quoted when it has spaces, like the mention popup).
pub fn reference(path: &str) -> String {
    if path.contains(char::is_whitespace) { format!("@\"{path}\"") } else { format!("@{path}") }
}

/// References for several files, each followed by a space, ready to append to the draft.
pub fn references(paths: &[String]) -> String {
    paths.iter().map(|p| format!("{} ", reference(p))).collect()
}
