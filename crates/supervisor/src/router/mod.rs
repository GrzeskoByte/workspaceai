//! `handle(method, path, query, body)` — the in-process equivalent of `buildRouter()` in `routes.ts`.
//! Group 1 (batch 2): app state/settings, workspaces, workspace patch, facts, policy and audit.
//! Paths match the TS router segment for segment; anything unknown is a 404.

mod app;
mod patch;
mod workspace;
mod workspaces;

use crate::supervisor::{ApiError, Core};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn dispatch(core: &mut Core, method: &str, path: &str, query: &BTreeMap<String, String>, body: Option<Value>) -> Result<Value, ApiError> {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let body = body.unwrap_or(Value::Null);
    match segs.as_slice() {
        ["api", "state"] if method == "GET" => app::state(core),
        ["api", "settings"] if method == "PATCH" => app::patch_settings(core, &body),

        ["api", "workspaces"] if method == "POST" => workspaces::create(core, &body),
        ["api", "workspaces", "join-org"] if method == "POST" => workspaces::join_org(core, &body),
        ["api", "workspaces", id, "activate"] if method == "POST" => workspaces::activate(core, id),
        ["api", "workspaces", id] if method == "DELETE" => workspaces::delete(core, id),

        ["api", "ws", id, "snapshot"] if method == "GET" => workspace::snapshot(core, id),
        ["api", "ws", id] if method == "PATCH" => patch::patch(core, id, &body),

        ["api", "ws", id, "facts"] if method == "POST" => workspace::add_fact(core, id, &body),
        ["api", "ws", id, "facts"] if method == "DELETE" => workspace::clear_facts(core, id, &body),
        ["api", "ws", id, "facts", fid] if method == "PATCH" => workspace::update_fact(core, id, fid, &body),
        ["api", "ws", id, "facts", fid] if method == "DELETE" => workspace::remove_fact(core, id, fid),
        ["api", "ws", id, "memory-changed"] if method == "POST" => workspace::memory_changed(core, id),

        ["api", "ws", id, "policy", "refresh"] if method == "POST" => workspace::refresh_policy(core, id, &body),
        ["api", "ws", id, "audit"] if method == "GET" => workspace::audit(core, id, query),
        ["api", "ws", id, "audit", "upload"] if method == "POST" => workspace::upload_audit(core, id),

        _ => Err(ApiError::not_found("Not found")),
    }
}
