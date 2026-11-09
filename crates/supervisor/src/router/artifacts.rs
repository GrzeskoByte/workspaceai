//! Artifact routes for the UI and the agent tools (`extensions.ts`, artifact section).

use crate::artifacts::{artifact_guide, tool_type, NewArtifact};
use crate::formats::exports_for;
use crate::supervisor::{ApiError, Core, UiEvent};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use workspaceai_shared::util::project_path_name;

const GUIDE_TYPES: [&str; 5] = ["doc", "deck", "prototype", "sheet", "design-system"];

fn s(body: &Value, k: &str) -> Option<String> {
    body.get(k).and_then(Value::as_str).map(str::to_string)
}

/// `?v=` as `Number(v) || undefined`.
pub(super) fn version(query: &BTreeMap<String, String>) -> Option<i64> {
    query.get("v").and_then(|v| v.parse::<f64>().ok()).filter(|v| *v != 0.0 && v.is_finite()).map(|v| v as i64)
}

fn new_artifact(body: &Value, chat: Option<String>, project: Option<String>) -> NewArtifact {
    NewArtifact {
        project,
        chat,
        kind: s(body, "type").unwrap_or_default(),
        title: s(body, "title").unwrap_or_default(),
        content: body.get("content").map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| if c.is_null() { String::new() } else { c.to_string() })).unwrap_or_default(),
        note: s(body, "note"),
        format: s(body, "format"),
    }
}

fn content(body: &Value) -> String {
    new_artifact(body, None, None).content
}

pub(super) fn agent_guide(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    let rt = core.rt_mut(id)?;
    let kind = s(body, "type").filter(|t| GUIDE_TYPES.contains(&t.as_str())).unwrap_or_else(|| "doc".into());
    let chat = rt.calling_session(s(body, "chat").as_deref());
    rt.mark_guided(chat.as_deref(), &kind);
    Ok(json!({ "guide": artifact_guide(&kind) }))
}

fn need_guide(rt: &crate::Runtime, chat: Option<&str>, kind: &str) -> Result<(), ApiError> {
    if rt.is_guided(chat, kind) {
        return Ok(());
    }
    Err(ApiError::bad_request(format!(
        "Read Open Design's reference first: call artifact_guide with type \"{kind}\", follow the design system it returns, then call this tool again."
    )))
}

pub(super) fn agent_create(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    let rt = core.rt_mut(id)?;
    let chat = rt.calling_session(s(body, "chat").as_deref());
    need_guide(rt, chat.as_deref(), &s(body, "type").unwrap_or_else(|| "doc".into()))?;
    let project = if chat.is_some() { None } else { Some(rt.ws.default_project.clone()) };
    let a = rt.create_artifact(new_artifact(body, chat, project))?;
    if let Some(c) = &a.chat {
        rt.events().emit(UiEvent::Artifact { workspace: rt.id.clone(), chat: c.clone(), artifact: a.id.clone(), version: a.version });
    }
    let mut out = serde_json::to_value(&a).map_err(|e| ApiError::new(500, e.to_string()))?;
    out["projectPath"] = json!(project_path_name(&rt.state.projects, Some(&a.project)));
    Ok(out)
}

pub(super) fn agent_revise(core: &mut Core, id: &str, aid: &str, body: &Value) -> Result<Value, ApiError> {
    let rt = core.rt_mut(id)?;
    let chat = rt.calling_session(s(body, "chat").as_deref());
    need_guide(rt, chat.as_deref(), tool_type(&rt.artifact(aid)?.kind))?;
    let a = rt.revise_artifact(aid, &content(body), chat.clone(), s(body, "note"), s(body, "title"), s(body, "format"))?;
    if let Some(c) = chat {
        rt.events().emit(UiEvent::Artifact { workspace: rt.id.clone(), chat: c, artifact: a.id.clone(), version: a.version });
    }
    Ok(json!(a))
}

pub(super) fn create(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    let (chat, project) = (s(body, "chat"), s(body, "project"));
    if chat.is_none() && project.is_none() {
        return Err(ApiError::bad_request("Artifacts made from the library must pick a project"));
    }
    Ok(json!(core.rt_mut(id)?.create_artifact(new_artifact(body, chat, project))?))
}

pub(super) fn revise(core: &mut Core, id: &str, aid: &str, body: &Value) -> Result<Value, ApiError> {
    Ok(json!(core.rt_mut(id)?.revise_artifact(aid, &content(body), s(body, "chat"), s(body, "note"), None, None)?))
}

pub(super) fn restore(core: &mut Core, id: &str, aid: &str, body: &Value) -> Result<Value, ApiError> {
    let v = body.get("v").and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok())).ok_or_else(|| ApiError::bad_request("Pick a version"))?;
    Ok(json!(core.rt_mut(id)?.restore_artifact(aid, v)?))
}

pub(super) fn move_to(core: &mut Core, id: &str, aid: &str, body: &Value) -> Result<Value, ApiError> {
    Ok(json!(core.rt_mut(id)?.move_artifact(aid, &s(body, "project").unwrap_or_default())?))
}

pub(super) fn remove(core: &mut Core, id: &str, aid: &str) -> Result<Value, ApiError> {
    core.rt_mut(id)?.remove_artifact(aid)?;
    Ok(json!({ "ok": true }))
}

pub(super) fn exports(core: &mut Core, id: &str, aid: &str) -> Result<Value, ApiError> {
    Ok(json!({ "formats": exports_for(core.rt(id)?.artifact(aid)?) }))
}

pub(super) fn share(core: &mut Core, id: &str, aid: &str) -> Result<Value, ApiError> {
    let base = crate::server::loopback_info(&core.loopback).map(|l| l.url);
    core.rt(id)?.share_artifact(aid, base.as_deref())
}
