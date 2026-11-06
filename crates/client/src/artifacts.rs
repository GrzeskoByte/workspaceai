//! One artifact's versions, export, share, restore, move and delete (the native parts of the web ArtifactPanel).

use crate::api::seg;
use crate::types::Artifact;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    pub v: u32,
    pub note: String,
    pub format: String,
    /// Open Design saved it: `Some(lint findings)`.
    pub lint: Option<usize>,
    pub at: i64,
    pub chat: Option<String>,
    pub is_latest: bool,
}

pub fn format_label(format: &str) -> &'static str {
    match format {
        "html" => "HTML",
        "csv" => "CSV",
        "svg" => "SVG",
        "json" => "JSON",
        _ => "Markdown",
    }
}

/// Versions newest first. A version without a format reads as Markdown.
pub fn version_rows(a: &Artifact) -> Vec<VersionRow> {
    let latest = a.versions.iter().map(|v| v.v).max().unwrap_or(a.version);
    let mut rows: Vec<VersionRow> = a
        .versions
        .iter()
        .map(|v| VersionRow { v: v.v, note: v.note.clone(), format: v.format.clone().unwrap_or_else(|| "markdown".into()), lint: v.od.as_ref().map(|o| o.lint.len()), at: v.at, chat: v.chat.clone(), is_latest: v.v == latest })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.v));
    rows
}

/// "Saved by Open Design · lint clean", "… · lint 2 findings", or the built-in store.
pub fn lint_text(lint: Option<usize>) -> String {
    match lint {
        None => "Built-in store · Open Design not running for this version".into(),
        Some(0) => "Saved by Open Design · lint clean".into(),
        Some(n) => format!("Saved by Open Design · lint {n} finding{}", if n == 1 { "" } else { "s" }),
    }
}

/// The first few findings as one line each.
pub fn lint_lines(a: &Artifact, v: u32) -> Vec<String> {
    let Some(od) = a.versions.iter().find(|x| x.v == v).and_then(|x| x.od.as_ref()) else { return vec![] };
    od.lint
        .iter()
        .take(8)
        .map(|f| {
            let sev = ["severity", "level"].iter().find_map(|k| f[k].as_str()).unwrap_or("P?");
            let msg = ["message", "rule", "id"].iter().find_map(|k| f[k].as_str()).map(String::from).unwrap_or_else(|| f.to_string());
            format!("{sev} {msg}")
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExportFormat {
    pub label: String,
    #[serde(default)]
    pub ext: String,
    pub format: String,
}

pub fn parse_formats(reply: &Value) -> Vec<ExportFormat> {
    serde_json::from_value(reply["formats"].clone()).unwrap_or_default()
}

fn item(ws: &str, art: &str) -> String {
    format!("/api/ws/{}/artifacts/{}", seg(ws), seg(art))
}
pub fn item_path(ws: &str, art: &str) -> String {
    item(ws, art)
}
pub fn exports_path(ws: &str, art: &str) -> String {
    format!("{}/exports", item(ws, art))
}
/// The download route; `version` 0 means the latest.
pub fn export_path(ws: &str, art: &str, format: &str, version: u32) -> String {
    let v = if version > 0 { format!("&v={version}") } else { String::new() };
    format!("{}/export?format={}{v}", item(ws, art), crate::api::pct(format))
}
pub fn share_path(ws: &str, art: &str) -> String {
    format!("{}/share", item(ws, art))
}
pub fn restore_path(ws: &str, art: &str) -> String {
    format!("{}/restore", item(ws, art))
}
pub fn restore_body(v: u32) -> Value {
    json!({ "v": v })
}
pub fn move_path(ws: &str, art: &str) -> String {
    format!("{}/move", item(ws, art))
}
pub fn move_body(project: &str) -> Value {
    json!({ "project": project })
}

/// (what to tell the user, the link to copy) from the share route.
pub fn share_result(reply: &Value) -> (String, Option<String>) {
    let note = reply["note"].as_str().unwrap_or("Shared").to_string();
    let url = reply["url"].as_str().map(String::from);
    (match &url {
        Some(u) => format!("{note} · {u}"),
        None => note,
    }, url)
}
