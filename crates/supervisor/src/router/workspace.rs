//! `/api/ws/:id`: snapshot, facts, memory-changed, policy refresh, audit.

use crate::supervisor::{ApiError, Core};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use workspaceai_shared::types::FactStatus;
use workspaceai_shared::util::now_ms;

fn json<T: serde::Serialize>(v: &T) -> Result<Value, ApiError> {
    serde_json::to_value(v).map_err(|e| ApiError::new(500, e.to_string()))
}

pub(super) fn snapshot(core: &Core, id: &str) -> Result<Value, ApiError> {
    json(&core.rt(id)?.snapshot())
}

pub(super) fn add_fact(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    let rt = core.rt_mut(id)?;
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or_default();
    let scope = body.get("scope").and_then(|v| v.as_str()).unwrap_or("ws");
    let status = match body.get("status").and_then(|v| v.as_str()) {
        Some("suggested") => FactStatus::Suggested,
        _ => FactStatus::Approved,
    };
    let source = body.get("source").and_then(|v| v.as_str()).map(str::to_string);
    json(&rt.add_fact(text, scope, status, source)?)
}

pub(super) fn update_fact(core: &mut Core, id: &str, fid: &str, body: &Value) -> Result<Value, ApiError> {
    core.rt(id)?.update_fact(fid, body)?;
    Ok(json!({ "ok": true }))
}

pub(super) fn remove_fact(core: &mut Core, id: &str, fid: &str) -> Result<Value, ApiError> {
    core.rt(id)?.remove_fact(fid)?;
    Ok(json!({ "ok": true }))
}

/// Deviation from TS (documented in PORT-STATUS): wiping every fact needs an explicit `confirm: true`.
pub(super) fn clear_facts(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    if body.get("confirm").and_then(|v| v.as_bool()) != Some(true) {
        return Err(ApiError::bad_request("Deleting every fact needs {\"confirm\": true} in the body"));
    }
    core.rt(id)?.clear_facts()?;
    Ok(json!({ "ok": true }))
}

pub(super) fn memory_changed(core: &mut Core, id: &str) -> Result<Value, ApiError> {
    let rt = core.rt_mut(id)?;
    rt.audit("memory.suggest", "from agent", "agent");
    rt.changed()?;
    Ok(json!({ "ok": true }))
}

pub(super) fn refresh_policy(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    let tamper = body.get("tamper").and_then(|v| v.as_bool()).unwrap_or(false);
    core.refresh_policy(id, tamper)
}

pub(super) fn audit(core: &Core, id: &str, query: &BTreeMap<String, String>) -> Result<Value, ApiError> {
    let limit = query.get("limit").and_then(|v| v.parse::<usize>().ok()).unwrap_or(300);
    Ok(json!({ "events": core.rt(id)?.audit_log(limit) }))
}

pub(super) fn upload_audit(core: &mut Core, id: &str) -> Result<Value, ApiError> {
    let sent = core.rt_mut(id)?.upload_audit(now_ms(), true)?;
    Ok(json!({ "sent": sent }))
}
