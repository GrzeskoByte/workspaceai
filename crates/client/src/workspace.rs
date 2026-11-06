//! Requests and wording for the workspace settings pages: web search, members, vaults, skills, sharing, data,
//! audit, appearance, the opencode connection and Open Design. The supervisor merges `web`, `engine` and
//! `openDesign` into what it holds, so those are sent as partial objects.

use crate::api::seg;
use crate::types::{AuditEvent, Member, Skill, Vault};
use serde::Deserialize;
use serde_json::{json, Value};

pub const ACCENTS: [(&str, &str); 5] = [("default", "Theme default"), ("teal", "Teal"), ("amber", "Amber"), ("blue", "Blue"), ("green", "Green")];
pub const WEB_MODES: [(&str, &str); 4] = [("off", "Off"), ("exa", "Built-in (Exa)"), ("mcp", "Search provider"), ("searxng", "SearXNG")];
pub const WEB_PROVIDERS: [&str; 3] = ["Tavily", "Brave", "Firecrawl"];
pub const ROLES: [&str; 3] = ["Admin", "Member", "Viewer"];
pub const KEEP_CHATS: [&str; 3] = ["Forever", "1 year", "90 days"];
pub const VISIBILITY: [&str; 2] = ["Private", "Workspace"];

pub fn ws_path(workspace: &str) -> String {
    format!("/api/ws/{}", seg(workspace))
}

/// `PATCH /api/ws/:id` body for one nested setting, e.g. `nested("web", "mode", "exa")`.
pub fn nested(group: &str, field: &str, value: Value) -> Value {
    json!({ group: { field: value } })
}

// ---- web search ----

pub fn web_toast(mode: &str) -> String {
    format!("Web search · {mode} · opencode restarted")
}

pub fn web_preview_path(workspace: &str) -> String {
    format!("{}/web-preview", ws_path(workspace))
}

pub const WEB_TEST_PATH: &str = "/api/onboarding/test-web";

/// Body for "Check": the key is taken from the keychain by the server, the placeholder only marks it as present.
pub fn web_test_body(mode: &str, provider: &str, searxng_url: &str) -> Value {
    json!({ "mode": mode, "provider": provider, "url": searxng_url, "key": "from-keychain-xxxxxxxx" })
}

/// "OK" or the server's note/error for a finished check.
pub fn check_text(reply: &Value) -> String {
    if reply["ok"].as_bool() == Some(true) {
        reply["note"].as_str().filter(|n| !n.is_empty()).unwrap_or("OK").to_string()
    } else {
        reply["error"].as_str().filter(|e| !e.is_empty()).unwrap_or("Failed").to_string()
    }
}

// ---- members ----

/// New roster with `members[i]`'s role changed. The first member is the owner and cannot change.
pub fn role_body(members: &[Member], i: usize, role: &str) -> Option<Value> {
    if i == 0 || i >= members.len() || !ROLES.contains(&role) {
        return None;
    }
    let mut list = members.to_vec();
    list[i].role = role.to_string();
    Some(json!({ "members": list }))
}

/// New roster with one more Member. Refuses an empty address or one already invited.
pub fn invite_body(members: &[Member], email: &str) -> Option<Value> {
    let email = email.trim();
    if email.is_empty() || members.iter().any(|m| m.email.as_deref() == Some(email) || m.name == email) {
        return None;
    }
    let mut list = members.to_vec();
    list.push(Member { name: email.to_string(), email: Some(email.to_string()), role: "Member".into() });
    Some(json!({ "members": list }))
}

// ---- vaults ----

pub fn add_vault_body(vaults: &[Vault], name: &str, path: &str) -> Option<Value> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let mut list = vaults.to_vec();
    list.push(Vault { id: String::new(), name: name.trim().to_string(), path: path.to_string() });
    Some(json!({ "vaults": list }))
}

pub fn remove_vault_body(vaults: &[Vault], id: &str) -> Value {
    json!({ "vaults": vaults.iter().filter(|v| v.id != id).collect::<Vec<_>>() })
}

// ---- skills ----

pub fn toggle_skill_body(skills: &[Skill], id: &str) -> Value {
    let list: Vec<Skill> = skills.iter().map(|s| if s.id == id { Skill { on: !s.on, ..s.clone() } } else { s.clone() }).collect();
    json!({ "skills": list })
}

fn base36(mut n: u64) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// A skill from a folder path: named after its last segment, on, with an id from the clock.
pub fn add_skill_body(skills: &[Skill], folder: &str, now_ms: u64) -> Option<Value> {
    let folder = folder.trim();
    if folder.is_empty() {
        return None;
    }
    let name = folder.split('/').filter(|s| !s.is_empty()).last().unwrap_or(folder);
    let mut list = skills.to_vec();
    list.push(Skill { id: format!("sk-{}", base36(now_ms)), name: name.to_string(), note: folder.to_string(), on: true });
    Some(json!({ "skills": list }))
}

// ---- data & audit ----

pub fn archived_path(workspace: &str) -> String {
    format!("{}/archived", ws_path(workspace))
}

pub fn audit_path(workspace: &str, limit: u32) -> String {
    format!("{}/audit?limit={limit}", ws_path(workspace))
}

/// Events whose action, detail or actor contain `q` (any case); everything for an empty query.
pub fn filter_audit<'a>(events: &'a [AuditEvent], q: &str) -> Vec<&'a AuditEvent> {
    let q = q.trim().to_lowercase();
    events.iter().filter(|e| q.is_empty() || format!("{} {} {}", e.action, e.detail, e.actor).to_lowercase().contains(&q)).collect()
}

// ---- appearance ----

pub const APP_SETTINGS_PATH: &str = "/api/settings";

/// Choosing a theme stops following the system, as in the web app.
pub fn theme_body(theme: &str) -> Value {
    json!({ "theme": theme, "followSystem": false })
}

// ---- engine ----

pub fn engine_path(workspace: &str, tail: &str) -> String {
    format!("{}/engine/{tail}", ws_path(workspace))
}

pub fn managed_body() -> Value {
    json!({ "mode": "managed" })
}

/// Body for connecting to a server you run. The password is only sent when typed (else the keychain one is used).
pub fn remote_body(url: &str, username: &str, password: &str) -> Option<Value> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let mut body = json!({ "mode": "remote", "url": url, "username": username.trim() });
    if !password.is_empty() {
        body["password"] = json!(password);
    }
    Some(body)
}

/// Body to pin opencode to a version ("system" follows PATH). A leading `v` is dropped like the server does.
pub fn version_body(version: &str) -> Option<Value> {
    let v = version.trim().trim_start_matches(|c| c == 'v' || c == 'V');
    let v = if version.trim() == "system" { "system" } else { v };
    (!v.is_empty()).then(|| json!({ "version": v, "install": true }))
}

pub fn version_note(current: &str, running: &str, locked: bool) -> String {
    let mut s = if current.is_empty() || current == "system" { "Follows the opencode on PATH".to_string() } else { format!("Pinned to {current}") };
    if !running.is_empty() {
        s.push_str(&format!(" · running {running}"));
    }
    if locked {
        s.push_str(" · held by your organization");
    }
    s
}

/// The command in the logs reply, last 120 lines, or "(empty)".
pub fn log_text(reply: &Value, keep: usize) -> String {
    let text = reply["text"].as_str().unwrap_or("").trim_end();
    if text.is_empty() {
        return "(empty)".into();
    }
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(keep)..].join("\n")
}

// ---- Open Design ----

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
pub struct OdInstall {
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub installing: bool,
    #[serde(default)]
    pub progress: u32,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
pub struct OdDaemon {
    /// "not-installed", "stopped", "starting", "running" or "error".
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub pid: Option<u64>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OdArtifacts {
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub in_open_design: u32,
    #[serde(default)]
    pub lint_findings: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OdState {
    #[serde(default)]
    pub install: OdInstall,
    #[serde(default)]
    pub daemon: OdDaemon,
    #[serde(default)]
    pub settings: crate::types::OpenDesignSettings,
    #[serde(default)]
    pub data_dir: String,
    #[serde(default)]
    pub projects: u32,
    #[serde(default)]
    pub agent_tools: bool,
    #[serde(default)]
    pub artifacts: OdArtifacts,
}

impl OdState {
    /// Something is changing; the page asks again soon.
    pub fn busy(&self) -> bool {
        self.install.installing || self.daemon.state == "starting"
    }

    /// Artifacts the latest versions of which are not in Open Design yet.
    pub fn missing(&self) -> u32 {
        self.artifacts.total.saturating_sub(self.artifacts.in_open_design)
    }
}

pub fn daemon_label(state: &str) -> &'static str {
    match state {
        "not-installed" => "not installed",
        "stopped" => "stopped",
        "starting" => "starting…",
        "running" => "running",
        _ => "error",
    }
}

pub fn od_path(workspace: &str, tail: &str) -> String {
    if tail.is_empty() {
        format!("{}/opendesign", ws_path(workspace))
    } else {
        format!("{}/opendesign/{tail}", ws_path(workspace))
    }
}

pub const OD_INSTALL_PATH: &str = "/api/opendesign/install";
pub const OD_UPDATE_PATH: &str = "/api/opendesign/update";

pub fn design_systems_path(workspace: &str) -> String {
    format!("{}/design-systems", ws_path(workspace))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DesignSystem {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
}

pub fn parse_design_systems(reply: &Value) -> Vec<DesignSystem> {
    serde_json::from_value(reply["systems"].clone()).unwrap_or_default()
}

/// At most 12 design systems (60 when searching), always including the chosen one.
pub fn visible_systems<'a>(all: &'a [DesignSystem], chosen: &str, query: &str) -> Vec<&'a DesignSystem> {
    let q = query.trim().to_lowercase();
    let cap = if q.is_empty() { 12 } else { 60 };
    all.iter().filter(|d| d.title == chosen || format!("{} {}", d.title, d.summary).to_lowercase().contains(&q)).take(cap).collect()
}

// ---- custom provider: edit ----

/// `PATCH …/providers/:id`: name, URL and models; the key only when a new one was typed.
pub fn edit_provider_body(name: &str, url: &str, models_csv: &str, key: &str) -> Result<Value, &'static str> {
    if name.trim().is_empty() {
        return Err("Give the provider a name.");
    }
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("The base URL must start with http:// or https://.");
    }
    let models: Vec<&str> = models_csv.split(',').map(str::trim).filter(|m| !m.is_empty()).collect();
    if models.is_empty() {
        return Err("Add at least one model id.");
    }
    let mut body = json!({ "name": name.trim(), "url": url, "models": models });
    if let Some(k) = crate::providers::secret_body(key) {
        body["key"] = k["value"].clone();
    }
    Ok(body)
}

/// The models of a provider as the edit field shows them.
pub fn models_csv(p: &crate::types::Provider) -> String {
    p.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")
}
