//! Linked computers: how they read, and the requests for access, wake and unlink.

use crate::api::seg;
use crate::types::{Capabilities, Computer};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use workspaceai_shared::schedule::format_when;

/// "Online", or "Asleep · last seen Today 08:30" / "Offline · last seen …"; unknown states are shown as sent.
pub fn status_text(c: &Computer, tz: &str, now: DateTime<Utc>) -> String {
    let word = match c.status.as_str() {
        "online" => return "Online".into(),
        "asleep" => "Asleep",
        "offline" => "Offline",
        other => return other.to_string(),
    };
    format!("{word} · last seen {}", format_when(c.last_seen, tz, now.timestamp_millis()))
}

pub fn capability_text(c: &Capabilities) -> String {
    let on: Vec<&str> = [(c.files, "files"), (c.commands, "commands"), (c.browser, "browser"), (c.computer_use, "computer use"), (c.wake, "wake")]
        .into_iter()
        .filter_map(|(yes, name)| yes.then_some(name))
        .collect();
    if on.is_empty() {
        return "No capabilities".into();
    }
    let mut text = on.join(", ");
    text[..1].make_ascii_uppercase();
    text
}

pub fn allowed_for(c: &Computer, workspace: &str) -> bool {
    c.access.get(workspace).and_then(|a| a["allowed"].as_bool()).unwrap_or(false)
}

/// `PATCH /api/computers/:id` body. The server replaces the whole `access` map, so every other workspace and this
/// workspace's folders are carried over unchanged.
pub fn access_body(c: &Computer, workspace: &str, allowed: bool) -> Value {
    let mut access = c.access.clone();
    let entry = access.entry(workspace.to_string()).or_insert_with(|| json!({ "allowed": false, "folders": [] }));
    entry["allowed"] = json!(allowed);
    if !entry["folders"].is_array() {
        entry["folders"] = json!([]);
    }
    json!({ "access": access })
}

/// Waking needs a computer that is not online and allows it.
pub fn can_wake(c: &Computer) -> bool {
    c.status != "online" && c.capabilities.wake
}

pub fn item_path(id: &str) -> String {
    format!("/api/computers/{}", seg(id))
}

pub fn wake_path(id: &str) -> String {
    format!("{}/wake", item_path(id))
}
