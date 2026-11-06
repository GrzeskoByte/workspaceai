//! Routes group 3a: projects, their agents file and instruction layers, and the archived list.

use crate::supervisor::{ApiError, Core};
use serde_json::{json, Value};

pub(super) fn create(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    let rt = core.rt_mut(id)?;
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or_default();
    let parent = body.get("parent").and_then(|v| v.as_str());
    let folder = body.get("folder").and_then(|v| v.as_str());
    serde_json::to_value(rt.create_project(name, parent, folder)?).map_err(|e| ApiError::new(500, e.to_string()))
}

pub(super) fn update(core: &mut Core, id: &str, pid: &str, body: &Value) -> Result<Value, ApiError> {
    let p = core.rt_mut(id)?.update_project(pid, body)?;
    serde_json::to_value(p).map_err(|e| ApiError::new(500, e.to_string()))
}

pub(super) fn archive(core: &mut Core, id: &str, pid: &str) -> Result<Value, ApiError> {
    let hidden = core.rt_mut(id)?.archive_project(pid)?;
    Ok(json!({ "hiddenSubprojects": hidden }))
}

pub(super) fn restore(core: &mut Core, id: &str, pid: &str) -> Result<Value, ApiError> {
    core.rt_mut(id)?.restore_project(pid)?;
    Ok(json!({ "ok": true }))
}

pub(super) fn delete(core: &mut Core, id: &str, pid: &str) -> Result<Value, ApiError> {
    core.rt_mut(id)?.delete_project(pid)?;
    Ok(json!({ "ok": true }))
}

pub(super) fn agents(core: &Core, id: &str, pid: &str) -> Result<Value, ApiError> {
    let rt = core.rt(id)?;
    let file = rt.agents_file(pid)?;
    let text = rt.read_agents(pid)?;
    Ok(json!({ "file": file, "text": text }))
}

pub(super) fn write_agents(core: &mut Core, id: &str, pid: &str, body: &Value) -> Result<Value, ApiError> {
    let text = body.get("text").map(|v| v.as_str().unwrap_or_default()).unwrap_or_default();
    core.rt_mut(id)?.write_agents(pid, text)?;
    Ok(json!({ "ok": true }))
}

pub(super) fn layers(core: &Core, id: &str, pid: &str) -> Result<Value, ApiError> {
    Ok(json!({ "layers": core.rt(id)?.instruction_layers(pid) }))
}

/// `GET /api/ws/:id/archived`: archived projects whose parent is not archived too.
pub(super) fn archived(core: &Core, id: &str) -> Result<Value, ApiError> {
    let rt = core.rt(id)?;
    let projects: Vec<_> = rt
        .state
        .projects
        .iter()
        .filter(|p| p.archived && !p.parent.as_ref().is_some_and(|parent| rt.state.projects.iter().any(|q| &q.id == parent && q.archived)))
        .collect();
    Ok(json!({ "projects": projects }))
}
