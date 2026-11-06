//! Settings pages and the workspace/engine requests behind them.

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub id: &'static str,
    pub title: &'static str,
    /// Set by the organization's policy: shown, but not editable here.
    pub locked: bool,
}

const PAGES: [(&str, &str); 18] = [
    ("general", "General"),
    ("projects", "Projects"),
    ("members", "Workspaces"),
    ("providers", "Model providers"),
    ("agents", "Agents"),
    ("connectors", "Connectors"),
    ("web", "Web search"),
    ("memory", "Memory"),
    ("vaults", "Vaults"),
    ("skills", "Skills & plugins"),
    ("artifacts", "Artifacts & sharing"),
    ("opendesign", "Open Design"),
    ("routines", "Routines"),
    ("computers", "Computers"),
    ("data", "Data & retention"),
    ("audit", "Audit log"),
    ("engine", "Engine"),
    ("appearance", "Appearance"),
];

const ORG_LOCKED: [&str; 6] = ["providers", "members", "artifacts", "data", "web", "agents"];

pub fn pages(is_org: bool) -> Vec<Page> {
    PAGES.iter().map(|(id, title)| Page { id, title, locked: is_org && ORG_LOCKED.contains(id) }).collect()
}

/// `useMemory` and `memoryRecall` are on when the supervisor sends nothing.
pub fn switch_on(value: Option<bool>) -> bool {
    value.unwrap_or(true)
}

/// Body for `PATCH /api/ws/:id` changing one setting.
pub fn patch(field: &str, value: Value) -> Value {
    json!({ field: value })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineAction {
    Start,
    Stop,
    Restart,
}

impl EngineAction {
    pub fn path(self, workspace: &str) -> String {
        let verb = match self {
            EngineAction::Start => "start",
            EngineAction::Stop => "stop",
            EngineAction::Restart => "restart",
        };
        format!("/api/ws/{}/engine/{verb}", crate::api::seg(workspace))
    }

    pub fn label(self) -> &'static str {
        match self {
            EngineAction::Start => "Start",
            EngineAction::Stop => "Stop",
            EngineAction::Restart => "Restart",
        }
    }
}

/// The buttons that make sense for the engine's current state.
pub fn engine_actions(state: &str) -> Vec<EngineAction> {
    match state {
        "running" | "connected" => vec![EngineAction::Restart, EngineAction::Stop],
        "starting" | "restarting" => vec![],
        _ => vec![EngineAction::Start],
    }
}
