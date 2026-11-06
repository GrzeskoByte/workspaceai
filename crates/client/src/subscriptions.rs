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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignIn {
    /// "idle", "waiting", "done" or "failed".
    pub state: String,
    pub url: Option<String>,
    pub error: Option<String>,
}

impl SignIn {
    pub fn waiting(&self) -> bool {
        self.state == "waiting"
    }
}

pub fn parse_signin(reply: &Value) -> SignIn {
    let s = |k: &str| reply[k].as_str().filter(|v| !v.is_empty()).map(str::to_string);
    SignIn { state: s("state").unwrap_or_else(|| "idle".into()), url: s("url"), error: s("error") }
}

/// What the Agents page says while a browser sign-in runs, or after it ended.
pub fn signin_text(s: &SignIn) -> String {
    match s.state.as_str() {
        "waiting" => "Finish signing in in your browser. If none opened, use the link below and paste the code it shows.".into(),
        "done" => "Signed in.".into(),
        "failed" => format!("Sign-in failed. {}", s.error.as_deref().unwrap_or("")).trim().to_string(),
        _ => String::new(),
    }
}

/// Only Claude Code has a sign-in that runs unattended; the other agents are signed in from a terminal.
pub fn can_sign_in(s: &Subscription) -> bool {
    s.installed && s.id == "claude"
}

pub fn signin_path(workspace: &str, id: &str) -> String {
    format!("{}/{}/signin", list_path(workspace), seg(id))
}

pub fn signin_code_path(workspace: &str, id: &str) -> String {
    format!("{}/code", signin_path(workspace, id))
}

pub fn signin_cancel_path(workspace: &str, id: &str) -> String {
    format!("{}/cancel", signin_path(workspace, id))
}

pub fn code_body(code: &str) -> Option<Value> {
    let c = code.trim();
    (!c.is_empty()).then(|| serde_json::json!({ "code": c }))
}
