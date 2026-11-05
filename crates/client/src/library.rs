//! The Library: artifacts the agents made, and how one is shown.

use crate::api::seg;
use crate::types::{Artifact, Project};
use chrono::{DateTime, Utc};
use serde_json::Value;
use workspaceai_shared::schedule::format_when;

/// Longest text shown for a CSV/JSON/SVG artifact; the rest is cut so a huge file cannot freeze the window.
pub const MAX_SHOWN: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub project: String,
    pub versions: usize,
    pub updated_at: i64,
}

/// Artifacts newest first. `query` matches the title, type or project name, ignoring case; blank shows all.
pub fn rows(artifacts: &[Artifact], projects: &[Project], query: &str) -> Vec<Row> {
    let q = query.trim().to_lowercase();
    let project_name = |id: &str| projects.iter().find(|p| p.id == id).map_or("—".to_string(), |p| p.name.clone());
    let mut out: Vec<Row> = artifacts
        .iter()
        .map(|a| Row { id: a.id.clone(), title: a.title.clone(), kind: a.kind.clone(), project: project_name(&a.project), versions: a.versions.len().max(a.version as usize), updated_at: a.updated_at })
        .filter(|r| q.is_empty() || [&r.title, &r.kind, &r.project].iter().any(|f| f.to_lowercase().contains(&q)))
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub content: String,
    pub format: String,
}

/// `{content, format}` from the content route; a missing format reads as Markdown.
pub fn parse_view(reply: &Value) -> Option<View> {
    let content = reply["content"].as_str()?.to_string();
    Some(View { content, format: reply["format"].as_str().unwrap_or("markdown").to_string() })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Markdown(String),
    Text(String),
    /// Cannot be drawn natively yet; the string says why.
    Unsupported(String),
}

pub fn body_for(format: &str, content: &str) -> Body {
    match format {
        "markdown" => Body::Markdown(content.to_string()),
        "csv" | "json" | "svg" => {
            if content.chars().count() > MAX_SHOWN {
                Body::Text(format!("{}\n… (shortened)", content.chars().take(MAX_SHOWN).collect::<String>()))
            } else {
                Body::Text(content.to_string())
            }
        }
        "html" => Body::Unsupported("This is an HTML page. Previews need the web view, which arrives with the Open Design embed.".into()),
        other => Body::Unsupported(format!("The native app cannot show “{other}” yet.")),
    }
}

pub fn updated_text(ms: i64, tz: &str, now: DateTime<Utc>) -> String {
    format_when(ms, tz, now.timestamp_millis())
}

pub fn content_path(workspace: &str, artifact: &str, version: Option<u32>) -> String {
    let base = format!("/api/ws/{}/artifacts/{}/content", seg(workspace), seg(artifact));
    match version {
        Some(v) => format!("{base}?v={v}"),
        None => base,
    }
}
