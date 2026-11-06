//! `/api/ws/:id/engine*`, `/models` and `/opencode-json` — port of the engine group in `routes.ts`.

use crate::engine::detect::{install_opencode, pinned_opencode_bin};
use crate::engine::remote::test_remote;
use crate::jsonfile::read_text;
use crate::supervisor::{ApiError, Core};
use serde_json::{json, Value};
use workspaceai_shared::opencode_config::build_opencode_json;
use workspaceai_shared::types::{EngineMode, WorkspaceKind};

pub(super) fn status(core: &Core, id: &str) -> Result<Value, ApiError> {
    serde_json::to_value(core.rt(id)?.engine_status()).map_err(|e| ApiError::new(500, e.to_string()))
}

pub(super) fn start(core: &mut Core, id: &str) -> Result<Value, ApiError> {
    core.rt_mut(id)?.engine_client()?;
    status(core, id)
}

/// Pin this workspace to an exact opencode release, or go back to the system one.
pub(super) fn pin_version(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    core.assert_allowed(core.rt(id)?, "engine")?;
    let raw = body["version"].as_str().unwrap_or("system").trim();
    let stripped = raw.strip_prefix('v').filter(|r| r.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(raw);
    let version = if stripped.is_empty() { "system".to_string() } else { stripped.to_string() };
    let home = core.home.clone();
    if version != "system" && !pinned_opencode_bin(&home, &version).exists() {
        if body["install"].as_bool() != Some(true) {
            return Err(ApiError::new(409, format!("opencode {version} is not installed yet")));
        }
        install_opencode(&home, &version).map_err(ApiError::bad_request)?;
    }
    let rt = core.rt_mut(id)?;
    rt.ws.engine.version = version.clone();
    let detail = if version == "system" { "opencode from PATH".to_string() } else { format!("pinned to opencode {version}") };
    rt.audit("engine.version", &detail, "you");
    rt.changed()?;
    if rt.engine_running() {
        rt.restart_engine()?;
    }
    let status = serde_json::to_value(rt.engine_status()).map_err(|e| ApiError::new(500, e.to_string()))?;
    Ok(json!({ "version": version, "status": status }))
}

pub(super) fn restart(core: &mut Core, id: &str) -> Result<Value, ApiError> {
    core.rt_mut(id)?.restart_engine()?;
    status(core, id)
}

pub(super) fn stop(core: &mut Core, id: &str) -> Result<Value, ApiError> {
    core.rt_mut(id)?.stop_engine();
    status(core, id)
}

pub(super) fn set_mode(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    core.assert_allowed(core.rt(id)?, "engine")?;
    let rt = core.rt_mut(id)?;
    if body["mode"].as_str() == Some("remote") {
        let url = body["url"].as_str().unwrap_or_default().to_string();
        let username = body["username"].as_str().unwrap_or("opencode").to_string();
        let fallback = rt.keychain.get(id, "OPENCODE_SERVER_PASSWORD").unwrap_or_default();
        let password = body["password"].as_str().map(str::to_string).filter(|p| !p.is_empty()).unwrap_or(fallback);
        let test = test_remote(&url, &username, &password);
        if !test.ok {
            return Err(ApiError::bad_request(test.error.unwrap_or_else(|| "Connection test failed".into())));
        }
        if let Some(pw) = body["password"].as_str() {
            rt.keychain.set(id, "OPENCODE_SERVER_PASSWORD", pw).map_err(|e| ApiError::new(500, e))?;
        }
        rt.ws.engine.mode = EngineMode::Remote;
        rt.ws.engine.url = url;
        rt.ws.engine.username = username;
        rt.ws.engine.password_secret = Some("OPENCODE_SERVER_PASSWORD".into());
    } else {
        rt.ws.engine.mode = EngineMode::Managed;
    }
    let mode = rt.ws.engine.mode.as_str().to_string();
    let detail = if mode == "remote" { format!("remote {}", rt.ws.engine.url) } else { mode };
    rt.audit("engine.mode", &detail, "you");
    rt.save()?;
    rt.restart_engine()?;
    serde_json::to_value(rt.snapshot()).map_err(|e| ApiError::new(500, e.to_string()))
}

/// Last 500 lines of `<ws>/data/logs/opencode.log`.
pub(super) fn logs(core: &Core, id: &str) -> Result<Value, ApiError> {
    Ok(json!({ "text": core.rt(id)?.engine_logs() }))
}

/// Connected providers' models, from opencode plus what the workspace configured by hand.
pub(super) fn models(core: &mut Core, id: &str) -> Result<Value, ApiError> {
    let (allowed, agents) = {
        let rt = core.rt(id)?;
        let allowed = if rt.ws.kind == WorkspaceKind::Org { rt.policy.as_ref().map(|p| p.allowed_models.clone()) } else { None };
        (allowed, rt.ws.agents.clone())
    };
    let engine = core.rt_mut(id)?.engine_models();
    let rt = core.rt_mut(id)?;
    let mut groups: Vec<Value> = Vec::new();
    for p in rt.connected_providers() {
        let mut seen: Vec<(String, Value)> = Vec::new();
        for m in &engine {
            if m["providerID"].as_str() != Some(p.id.as_str()) {
                continue;
            }
            let mid = m["id"].as_str().unwrap_or_default();
            let mut option = json!({ "id": format!("{}/{mid}", p.id), "name": m["name"].as_str().unwrap_or(mid) });
            if let Some(efforts) = m["efforts"].as_array() {
                option["efforts"] = json!(efforts);
            }
            seen.retain(|(k, _)| k != mid);
            seen.push((mid.to_string(), option));
        }
        for m in &p.models {
            if seen.iter().any(|(k, _)| *k == m.id) {
                continue;
            }
            let name = m.name.as_deref().filter(|n| !n.is_empty()).unwrap_or(&m.id);
            seen.push((m.id.clone(), json!({ "id": format!("{}/{}", p.id, m.id), "name": name })));
        }
        let matches_allowed = |m: &Value| match &allowed {
            None => true,
            Some(a) => m["id"].as_str().is_some_and(|id| a.iter().any(|x| x == id)),
        };
        let models: Vec<Value> = seen.into_iter().map(|(_, v)| v).filter(matches_allowed).collect();
        if models.is_empty() {
            continue;
        }
        let note = if p.from_opencode == Some(true) {
            "your opencode login"
        } else if p.key_env.as_deref().is_some_and(|k| !k.is_empty()) {
            "API key"
        } else if p.note.is_empty() {
            "no key needed"
        } else {
            p.note.as_str()
        };
        groups.push(json!({ "provider": p.id, "name": p.name, "note": note, "models": models }));
    }
    let ids: Vec<Value> = groups.iter().flat_map(|g| g["models"].as_array().cloned().unwrap_or_default()).map(|m| m["id"].clone()).collect();
    let agent_models = rt.state.extra.get("agentModels").cloned().unwrap_or_else(|| json!({}));
    let mut out_agents = serde_json::Map::new();
    for a in agents.iter().filter(|a| a.on && a.id != "opencode") {
        out_agents.insert(a.id.clone(), agent_models.get(&a.id).cloned().unwrap_or_else(|| json!({ "models": [], "current": "" })));
    }
    Ok(json!({ "models": ids, "groups": groups, "agents": out_agents }))
}

pub(super) fn opencode_json(core: &Core, id: &str) -> Result<Value, ApiError> {
    let rt = core.rt(id)?;
    let built = build_opencode_json(&rt.ws, rt.policy.as_ref(), &serde_json::Map::new());
    Ok(json!({
        "path": rt.paths.opencode_json.to_string_lossy(),
        "json": Value::Object(built.json),
        "written": read_text(&rt.paths.opencode_json),
    }))
}
