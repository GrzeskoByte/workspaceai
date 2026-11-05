//! Agent subscriptions (Claude Code, Grok, …) as Settings › Agents shows them.

use crate::api::seg;
use crate::types::Subscription;
use serde_json::Value;

/// Rows from the list reply, which is an array (or an object holding one). Rows that do not parse are skipped.
pub fn parse_list(reply: &Value) -> Vec<Subscription> {
    let rows = reply.as_array().or_else(|| reply["subscriptions"].as_array());
    rows.map(|a| a.iter().filter_map(|r| serde_json::from_value(r.clone()).ok()).collect()).unwrap_or_default()
}

/// "Installed 2.1.0 · signed in as me@x · Connected to this workspace".
pub fn summary(s: &Subscription) -> String {
    let installed = if s.installed { format!("Installed{}", s.version.as_deref().map(|v| format!(" {v}")).unwrap_or_default()) } else { "Not installed".into() };
    let mut parts = vec![installed];
    if s.installed {
        parts.push(if s.signed_in { format!("signed in{}", s.account.as_deref().map(|a| format!(" as {a}")).unwrap_or_default()) } else { "not signed in".into() });
    }
    parts.push(if s.connected { "Connected to this workspace".into() } else { "Not connected to this workspace".into() });
    parts.join(" · ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubAction {
    Connect,
    Disconnect,
}

impl SubAction {
    pub fn label(self) -> &'static str {
        match self {
            SubAction::Connect => "Connect",
            SubAction::Disconnect => "Disconnect",
        }
    }
}

/// A connected subscription can always be disconnected; connecting needs the tool installed.
pub fn actions(s: &Subscription) -> Vec<SubAction> {
    if s.connected {
        vec![SubAction::Disconnect]
    } else if s.installed {
        vec![SubAction::Connect]
    } else {
        vec![]
    }
}

/// Connected here, but the login has expired.
pub fn needs_sign_in(s: &Subscription) -> bool {
    s.connected && !s.signed_in
}

pub fn list_path(workspace: &str) -> String {
    format!("/api/ws/{}/subscriptions", seg(workspace))
}

pub fn action_path(workspace: &str, id: &str, action: SubAction) -> String {
    let verb = match action {
        SubAction::Connect => "connect",
        SubAction::Disconnect => "disconnect",
    };
    format!("{}/{}/{verb}", list_path(workspace), seg(id))
}
