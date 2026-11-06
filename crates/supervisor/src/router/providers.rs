//! Providers (R-20), secrets (R-34) and MCP connectors — port of the `routes.ts` group.
//! Error strings are copied verbatim from the TS; the keychain never returns a value over the wire.

use crate::providers::{test_endpoint, web_providers};
use crate::supervisor::{ApiError, Core};
use serde_json::{json, Value};
use workspaceai_shared::types::{McpServer, McpType, Provider, ProviderModel, WorkspaceKind};
use workspaceai_shared::util::uid;

fn provider_models(body: &Value) -> Vec<ProviderModel> {
    match body["models"].as_array() {
        Some(list) => list
            .iter()
            .map(|m| match m {
                Value::String(s) => ProviderModel { id: s.clone(), ..Default::default() },
                other => serde_json::from_value(other.clone()).unwrap_or_default(),
            })
            .collect(),
        None => vec![ProviderModel { id: body["model"].as_str().filter(|m| !m.is_empty()).unwrap_or("model").to_string(), ..Default::default() }],
    }
}

fn provider_id(raw: &str) -> String {
    raw.trim().to_lowercase().chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' { c } else { '-' }).collect()
}

pub(super) fn add(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    core.assert_allowed(core.rt(id)?, "providers")?;
    let rt = core.rt_mut(id)?;
    let pid = provider_id(body["id"].as_str().unwrap_or_default());
    if pid.is_empty() {
        return Err(ApiError::bad_request("Provider ID is required"));
    }
    if rt.ws.providers.iter().any(|p| p.id == pid) {
        return Err(ApiError::bad_request("A provider with that ID already exists"));
    }
    let default_env = format!("{}_API_KEY", pid.to_uppercase().replace('-', "_"));
    let key_env: String = body["env"].as_str().filter(|e| !e.is_empty()).unwrap_or(&default_env).to_uppercase().chars().map(|c| if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' { c } else { '_' }).collect();
    let npm = body["pkg"].as_str().filter(|p| !p.is_empty()).unwrap_or("@ai-sdk/openai-compatible").to_string();
    let url = body["url"].as_str().unwrap_or_default().to_string();
    let provider = Provider {
        id: pid.clone(),
        name: body["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(&pid).to_string(),
        note: format!("{npm} · {url}"),
        npm: Some(npm),
        base_url: body["url"].as_str().map(str::to_string),
        key_env: Some(key_env.clone()),
        models: provider_models(body),
        preset: Some("custom".into()),
        ..Default::default()
    };
    if let Some(key) = body["key"].as_str().filter(|k| !k.is_empty()) {
        rt.keychain.set(id, &key_env, key).map_err(|e| ApiError::new(500, e))?;
    }
    rt.ws.providers.push(provider.clone());
    if rt.ws.default_model.is_empty() {
        if let Some(first) = provider.models.first() {
            rt.ws.default_model = format!("{pid}/{}", first.id);
        }
    }
    rt.audit("provider.add", &format!("{pid} → opencode.json"), "you");
    rt.write_config()?;
    rt.changed()?;
    let running = rt.engine_running();
    if running {
        let _ = rt.restart_engine();
    }
    Ok(json!({ "provider": provider, "opencodeJson": crate::jsonfile::read_text(&rt.paths.opencode_json) }))
}

pub(super) fn test(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    let rt = core.rt(id)?;
    let found = body["id"].as_str().and_then(|pid| rt.ws.providers.iter().find(|p| p.id == pid));
    let url = body["url"].as_str().map(str::to_string).or_else(|| found.and_then(|p| p.base_url.clone()));
    let key = body["key"]
        .as_str()
        .map(str::to_string)
        .or_else(|| found.and_then(|p| p.key_env.as_deref()).and_then(|name| rt.keychain.get(id, name)));
    let Some(url) = url else {
        return Ok(json!({ "ok": true, "note": "opencode resolves this provider itself" }));
    };
    let pkg = body["pkg"].as_str().map(str::to_string).or_else(|| found.and_then(|p| p.npm.clone())).unwrap_or_default();
    // Only OpenAI-style SDKs have this chat route; a proxy for another SDK can only be listed.
    let model = if pkg.contains("openai") {
        body["model"].as_str().map(str::to_string).or_else(|| found.and_then(|p| p.models.first().map(|m| m.id.clone())))
    } else {
        None
    };
    Ok(test_endpoint(&url, key.as_deref(), model.as_deref()))
}

pub(super) fn patch(core: &mut Core, id: &str, pid: &str, body: &Value) -> Result<Value, ApiError> {
    core.assert_allowed(core.rt(id)?, "providers")?;
    let rt = core.rt_mut(id)?;
    let Some(p) = rt.ws.providers.iter().position(|x| x.id == pid) else {
        return Err(ApiError::not_found("Provider not found"));
    };
    if rt.ws.providers[p].locked == Some(true) {
        return Err(ApiError::forbidden("This provider is managed by org policy"));
    }
    let models: Option<Vec<String>> = body["models"].as_array().map(|a| a.iter().map(|m| m.as_str().unwrap_or_default().trim().to_string()).filter(|m| !m.is_empty()).collect());
    if models.as_ref().is_some_and(|m| m.is_empty()) {
        return Err(ApiError::bad_request("At least one model is required"));
    }
    let mut touched = false;
    if body["isDefault"].as_bool() == Some(true) {
        for x in rt.ws.providers.iter_mut() {
            x.is_default = Some(x.id == pid);
        }
        if let Some(first) = rt.ws.providers[p].models.first() {
            rt.ws.default_model = format!("{pid}/{}", first.id);
        }
    }
    if let Some(name) = body["name"].as_str().filter(|n| !n.is_empty()) {
        rt.ws.providers[p].name = name.to_string();
    }
    if let Some(url) = body["url"].as_str().filter(|u| !u.is_empty()) {
        rt.ws.providers[p].base_url = Some(url.to_string());
        let npm = rt.ws.providers[p].npm.clone().unwrap_or_default();
        rt.ws.providers[p].note = format!("{npm} · {url}");
        touched = true;
    }
    if let Some(models) = models {
        let old = rt.ws.providers[p].models.clone();
        rt.ws.providers[p].models = models
            .iter()
            .map(|m| old.iter().find(|x| x.id == *m).cloned().unwrap_or(ProviderModel { id: m.clone(), ..Default::default() }))
            .collect();
        let prefix = format!("{pid}/");
        let current = rt.ws.default_model.strip_prefix(&prefix).map(str::to_string);
        if current.is_some_and(|c| !models.contains(&c)) {
            rt.ws.default_model = format!("{pid}/{}", models[0]);
        }
        touched = true;
    }
    let key_env = rt.ws.providers[p].key_env.clone();
    if let (Some(key), Some(env)) = (body["key"].as_str().filter(|k| !k.is_empty()), key_env) {
        rt.keychain.set(id, &env, key).map_err(|e| ApiError::new(500, e))?;
        touched = true;
    }
    if touched {
        rt.audit("provider.edit", &format!("{pid} → opencode.json"), "you");
    }
    rt.write_config()?;
    rt.changed()?;
    if touched && rt.engine_running() {
        let _ = rt.restart_engine();
    }
    Ok(json!({ "ok": true }))
}

pub(super) fn remove(core: &mut Core, id: &str, pid: &str) -> Result<Value, ApiError> {
    core.assert_allowed(core.rt(id)?, "providers")?;
    let rt = core.rt_mut(id)?;
    let Some(p) = rt.ws.providers.iter().find(|x| x.id == pid).cloned() else {
        return Err(ApiError::not_found("Provider not found"));
    };
    rt.ws.providers.retain(|x| x.id != p.id);
    if let Some(env) = &p.key_env {
        if !rt.ws.providers.iter().any(|x| x.key_env.as_deref() == Some(env.as_str())) {
            let _ = rt.keychain.delete(id, env);
        }
    }
    if rt.ws.default_model.starts_with(&format!("{}/", p.id)) {
        rt.ws.default_model = rt.ws.providers.first().and_then(|x| x.models.first().map(|m| format!("{}/{}", x.id, m.id))).unwrap_or_default();
    }
    if !rt.ws.providers.is_empty() && !rt.ws.providers.iter().any(|x| x.is_default == Some(true)) {
        rt.ws.providers[0].is_default = Some(true);
    }
    rt.audit("provider.remove", &p.id, "you");
    rt.write_config()?;
    rt.changed()?;
    Ok(json!({ "ok": true }))
}

pub(super) fn web_list() -> Value {
    web_providers()
}

fn mcp_id(body: &Value, raw: &str) -> String {
    let base = match body["id"].as_str().filter(|i| !i.is_empty()) {
        Some(id) => id.to_string(),
        None => raw.split(['/', ' ', '\\']).filter(|s| !s.is_empty()).next_back().unwrap_or("mcp").to_string(),
    };
    let slug: String = base.to_lowercase().chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' { c } else { '-' }).collect();
    let slug: String = slug.chars().take(40).collect();
    let stamp = uid("m");
    let tail: String = stamp.chars().rev().take(4).collect::<Vec<char>>().into_iter().rev().collect();
    format!("{slug}-{tail}")
}

pub(super) fn add_mcp(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    {
        let rt = core.rt(id)?;
        if rt.ws.kind == WorkspaceKind::Org && body["locked"].as_bool() == Some(true) {
            return Err(ApiError::forbidden("Locked by org policy"));
        }
    }
    let command = body["command"].as_str().map(str::trim).filter(|c| !c.is_empty());
    let raw = command.or_else(|| body["name"].as_str().map(str::trim)).unwrap_or_default().to_string();
    if raw.is_empty() {
        return Err(ApiError::bad_request("Name or command required"));
    }
    let is_url = raw.starts_with("http://") || raw.starts_with("https://");
    let server = McpServer {
        id: mcp_id(body, &raw),
        name: body["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(&raw).to_string(),
        kind: if is_url { "MCP · http" } else { "MCP · stdio" }.into(),
        kind_type: if is_url { McpType::Remote } else { McpType::Local },
        command: if is_url { None } else { Some(raw.split_whitespace().map(str::to_string).collect()) },
        url: if is_url { Some(raw.clone()) } else { None },
        on: true,
        ..Default::default()
    };
    let rt = core.rt_mut(id)?;
    rt.ws.mcp.push(server.clone());
    rt.audit("mcp.add", &format!("{} → opencode.json", server.name), "you");
    rt.write_config()?;
    rt.changed()?;
    serde_json::to_value(&server).map_err(|e| ApiError::new(500, e.to_string()))
}

pub(super) fn patch_mcp(core: &mut Core, id: &str, mid: &str, body: &Value) -> Result<Value, ApiError> {
    let rt = core.rt_mut(id)?;
    let Some(m) = rt.ws.mcp.iter_mut().find(|m| m.id == mid) else {
        return Err(ApiError::not_found("Connector not found"));
    };
    if m.locked == Some(true) {
        return Err(ApiError::forbidden("Required by your organization"));
    }
    if let Some(on) = body["on"].as_bool() {
        m.on = on;
    }
    let (name, on) = (m.name.clone(), m.on);
    let out = serde_json::to_value(&*m).map_err(|e| ApiError::new(500, e.to_string()))?;
    rt.audit("mcp.toggle", &format!("{name}{}", if on { " on" } else { " off" }), "you");
    rt.write_config()?;
    rt.changed()?;
    Ok(out)
}

pub(super) fn delete_mcp(core: &mut Core, id: &str, mid: &str) -> Result<Value, ApiError> {
    let rt = core.rt_mut(id)?;
    let Some(m) = rt.ws.mcp.iter().find(|m| m.id == mid) else {
        return Err(ApiError::not_found("Connector not found"));
    };
    if m.locked == Some(true) {
        return Err(ApiError::forbidden("Required by your organization"));
    }
    rt.ws.mcp.retain(|m| m.id != mid);
    rt.write_config()?;
    rt.changed()?;
    Ok(json!({ "ok": true }))
}
