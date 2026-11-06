//! A project's instructions page: inherited layers, its own AGENTS.md, and the inherit / model settings.

use crate::api::seg;
use crate::types::Snapshot;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Layer {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub kind: String,
}

/// The layers above the project's own file (workspace, parents); the project's own is edited separately.
pub fn parse_layers(reply: &Value) -> Vec<Layer> {
    serde_json::from_value::<Vec<Layer>>(reply["layers"].clone()).unwrap_or_default().into_iter().filter(|l| l.kind != "project").collect()
}

pub fn parse_agents(reply: &Value) -> String {
    reply["text"].as_str().unwrap_or("").to_string()
}

fn base(ws: &str, project: &str) -> String {
    format!("/api/ws/{}/projects/{}", seg(ws), seg(project))
}
pub fn agents_path(ws: &str, project: &str) -> String {
    format!("{}/agents", base(ws, project))
}
pub fn layers_path(ws: &str, project: &str) -> String {
    format!("{}/layers", base(ws, project))
}
pub fn save_body(text: &str) -> Value {
    json!({ "text": text })
}
pub fn inherit_body(inherit: bool) -> Value {
    json!({ "inherit": inherit })
}
/// An empty model means "use the parent's / the workspace's".
pub fn model_body(model: &str) -> Value {
    json!({ "model": model })
}

/// The project fields the typed `Project` leaves out, read from the raw snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Meta {
    pub inherit: bool,
    pub model: String,
    pub folder: String,
}

pub fn meta(snapshot: &Value, project: &str) -> Meta {
    let p = snapshot["projects"].as_array().and_then(|l| l.iter().find(|p| p["id"] == project));
    Meta { inherit: p.is_none_or(|p| p["inherit"] != false), model: p.and_then(|p| p["model"].as_str()).unwrap_or("").into(), folder: p.and_then(|p| p["folder"].as_str()).unwrap_or("").into() }
}

/// "Opus (here)", "Opus (from Work)" or "the workspace default": where the project's model comes from.
pub fn model_source(snapshot: &Value, snap: &Snapshot, project: &str) -> String {
    let mut id = Some(project.to_string());
    for _ in 0..32 {
        let Some(cur) = id else { break };
        let m = meta(snapshot, &cur);
        if !m.model.is_empty() {
            let name = snap.projects.iter().find(|p| p.id == cur).map_or("", |p| p.name.as_str());
            return if cur == project { format!("{} (here)", m.model) } else { format!("{} (from {name})", m.model) };
        }
        id = snap.projects.iter().find(|p| p.id == cur).and_then(|p| p.parent.clone());
    }
    let d = &snap.workspace.default_model;
    if d.is_empty() { "the workspace default".into() } else { format!("{d} (workspace default)") }
}

/// The project and everything below it.
pub fn within(projects: &[crate::types::Project], root: &str) -> Vec<String> {
    let mut ids = vec![root.to_string()];
    let mut i = 0;
    while i < ids.len() {
        let cur = ids[i].clone();
        for p in projects.iter().filter(|p| p.parent.as_deref() == Some(cur.as_str())) {
            if !ids.contains(&p.id) {
                ids.push(p.id.clone());
            }
        }
        i += 1;
    }
    ids
}
