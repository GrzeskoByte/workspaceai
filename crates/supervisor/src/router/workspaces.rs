//! `/api/workspaces`: create, join an org through SSO, activate, delete.

use crate::supervisor::{ApiError, Core, CreateOptions, PolicyOverrides, Sso};
use serde_json::{json, Value};
use workspaceai_shared::types::WorkspaceKind;

fn json<T: serde::Serialize>(v: &T) -> Result<Value, ApiError> {
    serde_json::to_value(v).map_err(|e| ApiError::new(500, e.to_string()))
}

pub(super) fn create(core: &mut Core, body: &Value) -> Result<Value, ApiError> {
    if body.get("kind").and_then(|v| v.as_str()) == Some("org") {
        return Err(ApiError::bad_request("Organizations are joined with SSO"));
    }
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or_default();
    let kind = if body.get("kind").and_then(|v| v.as_str()) == Some("personal") { WorkspaceKind::Personal } else { WorkspaceKind::Team };
    let id = core.create_workspace(name, kind, CreateOptions::default())?;
    core.activate(&id)?;
    Ok(json!({ "id": id, "port": 0 }))
}

pub(super) fn join_org(core: &mut Core, body: &Value) -> Result<Value, ApiError> {
    let defaults = Sso::default();
    let field = |k: &str| body.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let sso = Sso {
        issuer: field("issuer").unwrap_or(defaults.issuer),
        subject: field("email").unwrap_or(defaults.subject),
        idp: field("idp").unwrap_or(defaults.idp),
    };
    let overrides = PolicyOverrides {
        audit_url: field("auditUrl"),
        audit_every_minutes: body.get("auditEveryMinutes").and_then(|v| v.as_i64()),
        log_content: body.get("logContent").and_then(|v| v.as_bool()),
        gateway: field("gateway"),
    };
    let name = field("name").unwrap_or_else(|| "BSC AI Factory".into());
    let id = core.join_organization(&name, sso, overrides)?;
    core.activate(&id)?;
    Ok(json!({ "id": id }))
}

pub(super) fn activate(core: &mut Core, id: &str) -> Result<Value, ApiError> {
    core.activate(id)?;
    json(&core.app_state())
}

pub(super) fn delete(core: &mut Core, id: &str) -> Result<Value, ApiError> {
    core.delete_workspace(id)?;
    json(&core.app_state())
}
