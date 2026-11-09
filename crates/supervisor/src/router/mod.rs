//! `handle(method, path, query, body)` — the in-process equivalent of `buildRouter()` in `routes.ts`.
//! Group 1 (batch 2): app state/settings, workspaces, workspace patch, facts, policy and audit.
//! Paths match the TS router segment for segment; anything unknown is a 404.

mod app;
mod engine;
mod patch;
mod projects;
mod providers;
mod secrets;
mod sessions;
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

        ["api", "ws", id, "snapshot"] if method == "GET" => {
            core.rt_mut(id)?.sync_sessions();
            workspace::snapshot(core, id)
        }
        ["api", "ws", id] if method == "PATCH" => patch::patch(core, id, &body),

        ["api", "ws", id, "facts"] if method == "POST" => workspace::add_fact(core, id, &body),
        ["api", "ws", id, "facts"] if method == "DELETE" => workspace::clear_facts(core, id, &body),
        ["api", "ws", id, "facts", fid] if method == "PATCH" => workspace::update_fact(core, id, fid, &body),
        ["api", "ws", id, "facts", fid] if method == "DELETE" => workspace::remove_fact(core, id, fid),
        ["api", "ws", id, "memory-changed"] if method == "POST" => workspace::memory_changed(core, id),

        // projects (batch 4); `GET /api/ws/:id/export` answers bytes, see `Supervisor::export_workspace`.
        ["api", "ws", id, "projects"] if method == "POST" => projects::create(core, id, &body),
        ["api", "ws", id, "projects", pid] if method == "PATCH" => projects::update(core, id, pid, &body),
        ["api", "ws", id, "projects", pid] if method == "DELETE" => projects::delete(core, id, pid),
        ["api", "ws", id, "projects", pid, "archive"] if method == "POST" => projects::archive(core, id, pid),
        ["api", "ws", id, "projects", pid, "restore"] if method == "POST" => projects::restore(core, id, pid),
        ["api", "ws", id, "projects", pid, "agents"] if method == "GET" => projects::agents(core, id, pid),
        ["api", "ws", id, "projects", pid, "agents"] if method == "PUT" => projects::write_agents(core, id, pid, &body),
        ["api", "ws", id, "projects", pid, "layers"] if method == "GET" => projects::layers(core, id, pid),
        ["api", "ws", id, "archived"] if method == "GET" => projects::archived(core, id),

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

        // sessions (batch 5)
        ["api", "ws", id, "chats"] if method == "POST" => sessions::create(core, id, &body),
        ["api", "ws", id, "chats", cid] if method == "PATCH" => sessions::patch(core, id, cid, &body),
        ["api", "ws", id, "chats", cid] if method == "DELETE" => sessions::delete(core, id, cid, &body),
        ["api", "ws", id, "chats", cid, "messages"] if method == "GET" => sessions::messages(core, id, cid),
        ["api", "ws", id, "chats", cid, "launch"] if method == "GET" => sessions::launch(core, id, cid),
        ["api", "ws", id, "chats", cid, "fork"] if method == "POST" => sessions::fork(core, id, cid),
        ["api", "ws", id, "chats", cid, "move"] if method == "POST" => sessions::move_to(core, id, cid, &body),
        ["api", "ws", id, "chats", cid, "attach"] if method == "GET" => sessions::attach(core, id, Some(cid)),
        ["api", "ws", id, "attach"] if method == "GET" => sessions::attach(core, id, None),
        ["api", "ws", id, "scan-sessions"] if method == "GET" => sessions::scan(core, id),
        ["api", "ws", id, "import-sessions"] if method == "POST" => sessions::import(core, id, &body),
        ["api", "ws", id, "scan-claude-sessions"] if method == "GET" => sessions::scan_claude(core, id),
        ["api", "ws", id, "import-claude-sessions"] if method == "POST" => sessions::import_claude(core, id, &body),
        ["api", "onboarding", "test-remote"] if method == "POST" => sessions::test_remote_route(&body),

        _ => Err(ApiError::not_found("Not found")),
    }
}
