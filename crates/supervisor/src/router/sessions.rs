//! Routes group 4: sessions (the TS chats routes minus send/command/stop/permission), launch spec, attach,
//! scan/import (opencode and claude copy-in), and the onboarding remote test.

use crate::engine::test_remote;
use crate::sessions::NewSession;
use crate::supervisor::{ApiError, Core};
use serde_json::{json, Value};
use std::path::PathBuf;

fn to_json<T: serde::Serialize>(v: &T) -> Result<Value, ApiError> {
    serde_json::to_value(v).map_err(|e| ApiError::new(500, e.to_string()))
}

fn s(body: &Value, k: &str) -> Option<String> {
    body[k].as_str().map(str::to_string)
}

fn ids(body: &Value) -> Option<Vec<String>> {
    body["ids"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
}

/// The user's own Claude Code folder, read only to copy chosen transcripts out of it.
fn user_claude_home() -> PathBuf {
    PathBuf::from(crate::paths::expand_home("~/.claude"))
}

pub(super) fn create(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    core.send_allowed(core.rt(id)?)?;
    let text = s(body, "text");
    let title = s(body, "title").filter(|t| !t.is_empty()).or_else(|| text.as_ref().map(|t| t.chars().take(60).collect())).unwrap_or_default();
    let o = NewSession { project: s(body, "project"), title, agent: s(body, "agent"), instructions: s(body, "instructions").unwrap_or_default(), prompt: text };
    to_json(&core.rt_mut(id)?.create_session(o)?)
}

pub(super) fn messages(core: &mut Core, id: &str, cid: &str) -> Result<Value, ApiError> {
    Ok(json!({ "messages": core.rt_mut(id)?.session_messages(cid)?, "streaming": false }))
}

pub(super) fn launch(core: &mut Core, id: &str, cid: &str) -> Result<Value, ApiError> {
    core.send_allowed(core.rt(id)?)?;
    to_json(&core.rt_mut(id)?.launch_spec(cid)?)
}

pub(super) fn fork(core: &mut Core, id: &str, cid: &str) -> Result<Value, ApiError> {
    to_json(&core.rt_mut(id)?.fork_session(cid)?)
}

pub(super) fn move_to(core: &mut Core, id: &str, cid: &str, body: &Value) -> Result<Value, ApiError> {
    let project = s(body, "project").unwrap_or_default();
    Ok(json!({ "movedArtifacts": core.rt_mut(id)?.move_session(cid, &project)? }))
}

pub(super) fn patch(core: &mut Core, id: &str, cid: &str, body: &Value) -> Result<Value, ApiError> {
    to_json(&core.rt_mut(id)?.patch_session(cid, body)?)
}

/// `{ "confirm": true }` also removes a claude transcript (the second step of the UI's delete).
pub(super) fn delete(core: &mut Core, id: &str, cid: &str, body: &Value) -> Result<Value, ApiError> {
    core.rt_mut(id)?.delete_session(cid, body["confirm"].as_bool() == Some(true))?;
    Ok(json!({ "ok": true }))
}

pub(super) fn attach(core: &Core, id: &str, cid: Option<&str>) -> Result<Value, ApiError> {
    Ok(json!({ "command": core.rt(id)?.attach_command(cid)? }))
}

pub(super) fn scan(core: &mut Core, id: &str) -> Result<Value, ApiError> {
    Ok(json!({ "sessions": core.rt_mut(id)?.scan_sessions()? }))
}

pub(super) fn import(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    let rt = core.rt_mut(id)?;
    let all = rt.scan_sessions()?;
    let wanted = ids(body);
    let chosen: Vec<Value> = all.into_iter().filter(|x| wanted.as_ref().is_none_or(|w| w.iter().any(|i| x["id"] == i.as_str()))).collect();
    rt.import_sessions(&chosen)
}

pub(super) fn scan_claude(core: &Core, id: &str) -> Result<Value, ApiError> {
    Ok(json!({ "sessions": core.rt(id)?.scan_claude_sessions(&user_claude_home()) }))
}

pub(super) fn import_claude(core: &mut Core, id: &str, body: &Value) -> Result<Value, ApiError> {
    let wanted = ids(body).unwrap_or_default();
    core.rt_mut(id)?.import_claude_sessions(&user_claude_home(), &wanted)
}

pub(super) fn test_remote_route(body: &Value) -> Result<Value, ApiError> {
    let url = s(body, "url").unwrap_or_default();
    let username = s(body, "username").unwrap_or_else(|| "opencode".into());
    let password = s(body, "password").unwrap_or_default();
    Ok(test_remote(&url, &username, &password).to_json())
}
