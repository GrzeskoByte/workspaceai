//! MCP connectors: how they read and the requests to switch or remove them.

use crate::api::seg;
use crate::types::McpServer;
use serde_json::{json, Value};

const MAX_DESCRIPTION: usize = 80;

/// The command or URL a connector runs, "Connector" for hosted ones, with "Built in · " for the app's own.
pub fn describe(m: &McpServer) -> String {
    let base = match m.kind_of.as_str() {
        "remote" => m.url.clone().unwrap_or_default(),
        "local" => m.command.as_deref().map(|c| c.join(" ")).unwrap_or_default(),
        _ => "Connector".to_string(),
    };
    let text = if m.builtin.is_some() { format!("Built in · {base}") } else { base };
    text.chars().take(MAX_DESCRIPTION).collect()
}

/// Built-in and organization-locked connectors cannot be removed.
pub fn can_remove(m: &McpServer) -> bool {
    m.locked != Some(true) && m.builtin.is_none()
}

/// Locked ones cannot be switched either; built-in ones can.
pub fn can_toggle(m: &McpServer) -> bool {
    m.locked != Some(true)
}

pub fn toggle_body(on: bool) -> Value {
    json!({ "on": on })
}

pub fn item_path(workspace: &str, id: &str) -> String {
    format!("/api/ws/{}/mcp/{}", seg(workspace), seg(id))
}
