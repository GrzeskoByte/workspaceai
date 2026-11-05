//! Slash commands in the native composer. The supervisor decides what a command means (`POST …/command`); this
//! turns its reply into something the window can show.

use serde_json::Value;
use workspaceai_shared::commands::ChatCommand;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The command was sent to the agent as a turn; its answer streams into the chat.
    Ran(String),
    NewChat,
    Help,
    /// Information to show in a panel (status, usage, context).
    Text { title: String, body: String },
    /// A dialog the native app does not have yet (`model`, `effort`, `graph`).
    Unsupported(String),
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

fn status_lines(st: &Value) -> String {
    let mut lines = Vec::new();
    for (label, key) in [("Agent", "agentName"), ("Model", "model"), ("Effort", "effort"), ("Project", "project"), ("Folder", "folder"), ("Session", "session"), ("Engine", "engine")] {
        let v = s(st, key);
        if !v.is_empty() {
            lines.push(format!("{label}: {v}"));
        }
    }
    if let Some(cost) = st["cost"].as_f64() {
        lines.push(format!("Cost: ${cost:.4}"));
    }
    lines.join("\n")
}

pub fn interpret(reply: &Value) -> Outcome {
    match reply["ui"].as_str() {
        Some("new") => Outcome::NewChat,
        Some("help") => Outcome::Help,
        Some("status") => Outcome::Text { title: "Status".into(), body: status_lines(&reply["status"]) },
        Some("sheet") => {
            let sheet = &reply["sheet"];
            let mut parts: Vec<String> = Vec::new();
            let text = s(sheet, "text");
            if !text.is_empty() {
                parts.push(text.to_string());
            } else if sheet["status"].is_object() {
                parts.push(status_lines(&sheet["status"]));
            }
            if !s(sheet, "note").is_empty() {
                parts.push(s(sheet, "note").to_string());
            }
            Outcome::Text { title: s(sheet, "title").to_string(), body: parts.join("\n\n") }
        }
        Some(other @ ("model" | "effort" | "graph")) => Outcome::Unsupported(other.to_string()),
        _ => Outcome::Ran(s(reply, "ran").to_string()),
    }
}

/// Commands to offer while the name is being typed: `/` plus a prefix, no space yet. At most 8.
pub fn suggestions<'a>(text: &str, list: &'a [ChatCommand]) -> Vec<&'a ChatCommand> {
    let Some(prefix) = text.strip_prefix('/') else { return vec![] };
    if prefix.contains(char::is_whitespace) {
        return vec![];
    }
    let prefix = prefix.to_lowercase();
    list.iter().filter(|c| c.name.to_lowercase().starts_with(&prefix)).take(8).collect()
}
