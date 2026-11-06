//! `PATCH /api/ws/:id`: the allowed key list, org locks and the merge rules of `routes.ts`.

use super::super::supervisor::{ApiError, Core};
use serde_json::{json, Value};
use workspaceai_shared::types::WorkspaceKind;
use workspaceai_shared::util::initials_of;

/// Every field the route accepts, in `routes.ts` order.
const ALLOWED: [&str; 28] = [
    "name",
    "accent",
    "instructions",
    "defaultModel",
    "defaultProject",
    "memory",
    "memoryMcpCommand",
    "graphifyFolders",
    "graphifyWatch",
    "vaults",
    "suggestMemories",
    "designSystem",
    "openDesign",
    "useMemory",
    "memoryRecall",
    "leanCtx",
    "web",
    "agents",
    "defaultAgent",
    "acpAskFirst",
    "members",
    "skills",
    "visibility",
    "publicLinks",
    "keepChats",
    "routineAskFirst",
    "routineIfAsleep",
    "engine",
];

/// Fields an org workspace locks; the per-chat `web` toggles stay usable.
const LOCKED_IN_ORG: [&str; 7] = ["name", "web", "agents", "defaultAgent", "engine", "keepChats", "publicLinks"];

/// Settings the TS spreads over the existing object instead of replacing it.
const MERGE: [&str; 4] = ["web", "engine", "openDesign", "leanCtx"];

/// A change here changes `opencode.json`, so a stopped engine rewrites it now.
const REWRITE_CONFIG: [&str; 9] = ["web", "mcp", "providers", "engine", "defaultModel", "memory", "useMemory", "openDesign", "leanCtx"];

pub(super) fn patch(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    let Some(obj) = body.as_object() else {
        return Err(ApiError::bad_request("Expected a JSON object"));
    };
    let (kind, web_mode, web_url, allowed_models) = {
        let rt = core.rt(id)?;
        (rt.ws.kind, rt.ws.web.mode.as_str().to_string(), rt.ws.web.searxng_url.clone(), rt.policy.as_ref().map(|p| p.allowed_models.clone()))
    };
    for (key, value) in obj {
        if !ALLOWED.contains(&key.as_str()) {
            return Err(ApiError::bad_request(format!("Unknown setting {key}")));
        }
        if kind == WorkspaceKind::Org && LOCKED_IN_ORG.contains(&key.as_str()) {
            let web_noop = key == "web"
                && value.get("mode").and_then(|v| v.as_str()) == Some(web_mode.as_str())
                && value.get("searxngUrl").and_then(|v| v.as_str()) == Some(web_url.as_str());
            if !web_noop {
                core.rt(id)?.audit("policy.block", &format!("change {key}"), "workspaceai");
                return Err(ApiError::forbidden("Locked by org policy"));
            }
        }
        if key == "defaultModel" && kind == WorkspaceKind::Org {
            if let Some(models) = &allowed_models {
                if !value.as_str().is_some_and(|m| models.iter().any(|a| a == m)) {
                    return Err(ApiError::forbidden("That model is not allowed by your organization"));
                }
            }
        }
        if key == "openDesign" {
            if let Some(engine) = value.get("engine").and_then(|v| v.as_str()) {
                if !["open-design", "builtin"].contains(&engine) {
                    return Err(ApiError::bad_request("Artifact engine must be open-design or builtin"));
                }
            }
        }
        // ponytail: `vaults` skips `cleanVaults`/`pruneAttachments`; they arrive with the vault routes (B12).
    }

    let snapshot = {
        let rt = core.rt_mut(id)?;
        let mut ws = serde_json::to_value(&rt.ws).map_err(|e| ApiError::new(500, e.to_string()))?;
        for (key, value) in obj {
            if MERGE.contains(&key.as_str()) {
                if let (Some(target), Some(patch)) = (ws.get_mut(key).and_then(|v| v.as_object_mut()), value.as_object()) {
                    for (k, v) in patch {
                        target.insert(k.clone(), v.clone());
                    }
                }
            } else {
                ws[key.as_str()] = value.clone();
            }
        }
        if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
            ws["initials"] = json!(initials_of(name));
        }
        rt.ws = serde_json::from_value(ws).map_err(|e| ApiError::bad_request(format!("Bad workspace setting: {e}")))?;
        let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        rt.audit("settings.update", &keys.join(", "), "you");
        rt.changed()?;
        if REWRITE_CONFIG.iter().any(|k| obj.contains_key(*k)) {
            rt.write_config()?;
        }
        serde_json::to_value(rt.snapshot()).map_err(|e| ApiError::new(500, e.to_string()))?
    };
    core.save_settings()?;
    Ok(snapshot)
}
