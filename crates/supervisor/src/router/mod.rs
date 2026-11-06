//! `handle(method, path, query, body)` — the in-process equivalent of `buildRouter()` in `routes.ts`.
//! Group 1 (batch 2): app state/settings, workspaces, workspace patch, facts, policy and audit.
//! Paths match the TS router segment for segment; anything unknown is a 404.

mod app;
mod engine;
mod patch;
mod providers;
mod secrets;
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

        // engine (batch 3)
        ["api", "ws", id, "opencode-json"] if method == "GET" => engine::opencode_json(core, id),
        ["api", "ws", id, "engine"] if method == "GET" => engine::status(core, id),
        ["api", "ws", id, "engine", "start"] if method == "POST" => engine::start(core, id),
        ["api", "ws", id, "engine", "version"] if method == "POST" => engine::pin_version(core, id, &body),
        ["api", "ws", id, "engine", "restart"] if method == "POST" => engine::restart(core, id),
        ["api", "ws", id, "engine", "stop"] if method == "POST" => engine::stop(core, id),
        ["api", "ws", id, "engine", "mode"] if method == "POST" => engine::set_mode(core, id, &body),
        ["api", "ws", id, "engine", "logs"] if method == "GET" => engine::logs(core, id),
        ["api", "ws", id, "models"] if method == "GET" => engine::models(core, id),

        // providers, secrets and connectors
        ["api", "web-providers"] if method == "GET" => Ok(providers::web_list()),
        ["api", "ws", id, "providers"] if method == "POST" => providers::add(core, id, &body),
        ["api", "ws", id, "providers", "test"] if method == "POST" => providers::test(core, id, &body),
        ["api", "ws", id, "providers", pid] if method == "PATCH" => providers::patch(core, id, pid, &body),
        ["api", "ws", id, "providers", pid] if method == "DELETE" => providers::remove(core, id, pid),
        ["api", "ws", id, "secrets"] if method == "GET" => secrets::list(core, id),
        ["api", "ws", id, "secrets", name] if method == "PUT" => secrets::put(core, id, name, &body),
        ["api", "ws", id, "secrets", name] if method == "DELETE" => secrets::delete(core, id, name),
        ["api", "ws", id, "mcp"] if method == "POST" => providers::add_mcp(core, id, &body),
        ["api", "ws", id, "mcp", mid] if method == "PATCH" => providers::patch_mcp(core, id, mid, &body),
        ["api", "ws", id, "mcp", mid] if method == "DELETE" => providers::delete_mcp(core, id, mid),

        _ => Err(ApiError::not_found("Not found")),
    }
}
