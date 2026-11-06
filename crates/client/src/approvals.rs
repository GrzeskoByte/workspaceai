//! Approvals overview: every tool request an agent is waiting on, across chats.

use crate::api::seg;
use crate::types::{PermissionRequest, Snapshot};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub chat: String,
    pub chat_title: String,
    pub project: String,
    pub agent: String,
    pub id: String,
    pub title: String,
    /// The tool input; empty when there is nothing worth showing.
    pub detail: String,
}

/// Pending requests, grouped by chat in the order of the snapshot's chats (newest first).
pub fn pending(snap: &Snapshot, perms: &HashMap<String, Vec<PermissionRequest>>) -> Vec<Ask> {
    let mut chats: Vec<_> = snap.chats.iter().filter(|c| perms.get(&c.id).is_some_and(|l| !l.is_empty())).collect();
    chats.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
    let agent_name = |id: &str| match id {
        "" | "opencode" => "opencode".to_string(),
        other => snap.workspace.agents.iter().find(|a| a.id == other).map_or(other.to_string(), |a| a.name.clone()),
    };
    chats
        .into_iter()
        .flat_map(|c| {
            let agent = agent_name(&c.agent);
            let project = snap.projects.iter().find(|p| p.id == c.project).map_or(String::new(), |p| p.name.clone());
            perms[&c.id].iter().map(move |p| Ask {
                chat: c.id.clone(),
                chat_title: c.title.clone(),
                project: project.clone(),
                agent: agent.clone(),
                id: p.id.clone(),
                title: p.title.clone(),
                detail: if p.detail == "\"\"" { String::new() } else { p.detail.clone() },
            })
        })
        .collect()
}

/// Chats whose turn waits for an answer; their requests are fetched when the overview opens.
pub fn waiting_chats(snap: &Snapshot) -> Vec<String> {
    snap.chat_activity.iter().filter(|(_, s)| s.as_str() == "waiting").map(|(c, _)| c.clone()).collect()
}

pub fn reply_path(workspace: &str, chat: &str, request: &str) -> String {
    format!("/api/ws/{}/chats/{}/permission/{}", seg(workspace), seg(chat), seg(request))
}

pub fn reply_body(decision: &str) -> Value {
    json!({ "decision": decision })
}

pub fn decision_toast(decision: &str) -> &'static str {
    if decision == "reject" { "Rejected — the step goes on without it" } else { "Allowed — the run goes on" }
}
