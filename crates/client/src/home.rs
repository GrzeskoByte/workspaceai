//! Home: the greeting, the composer's modes, and the "needs you / continue / coming up" columns.

use crate::api::seg;
use crate::types::{Chat, Routine, Snapshot};
use serde_json::{json, Value};

/// (id, label, placeholder), as in the web app.
pub const MODES: [(&str, &str, &str); 6] = [
    ("chat", "Chat", "Ask anything — it stays in the project you pick"),
    ("doc", "Doc", "Describe the document you need"),
    ("deck", "Deck", "What is the deck about, and who is it for?"),
    ("proto", "Prototype", "Describe the screen or page"),
    ("research", "Research", "What should I find out?"),
    ("loop", "Loop", "Describe a task to repeat until it’s done"),
];

pub fn mode_prefix(mode: &str) -> &'static str {
    match mode {
        "doc" => "Create a document artifact — read Open Design’s reference with artifact_guide, then save it with artifact_create — for this: ",
        "deck" => "Create a slide deck artifact — read Open Design’s reference with artifact_guide, then save it with artifact_create — for this: ",
        "proto" => "Create an interactive HTML prototype artifact — read Open Design’s reference with artifact_guide (and load the skill that fits), then save it with artifact_create — for this: ",
        "research" => "Research this using web search where available, and cite the pages you used: ",
        _ => "",
    }
}

pub fn placeholder(mode: &str) -> &'static str {
    MODES.iter().find(|m| m.0 == mode).map_or("", |m| m.2)
}

pub fn greeting(hour: u32) -> String {
    format!("{}. What are we making?", if hour < 12 { "Good morning" } else if hour < 19 { "Good afternoon" } else { "Good evening" })
}

pub fn suggestions(is_org: bool, project: &str) -> [String; 3] {
    [
        if is_org { "Draft a runbook for rotating the realm signing keys".into() } else { format!("Make a 5-slide deck about {project}") },
        if is_org { "Summarise last week’s incidents".into() } else { "Write a one-page plan for this week".into() },
        "Build a sheet tracking my open tasks".into(),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Go {
    Memory,
    Routines,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    pub title: String,
    pub sub: String,
    pub go: Go,
}

/// Memory suggestions to review and routine runs waiting for an approval.
pub fn needs(s: &Snapshot) -> Vec<Need> {
    let suggested: Vec<_> = s.facts.iter().filter(|f| f.status == "suggested").collect();
    let mut out = Vec::new();
    if let Some(f) = suggested.first() {
        out.push(Need { title: format!("Review {} memory suggestion{}", suggested.len(), if suggested.len() == 1 { "" } else { "s" }), sub: f.text.chars().take(60).collect(), go: Go::Memory });
    }
    for r in s.routines.iter().filter(|r| r.runs.first().is_some_and(|x| x.status == "waiting")) {
        let project = s.projects.iter().find(|p| p.id == r.project).map_or("—", |p| p.name.as_str());
        out.push(Need { title: format!("Approve: {}", r.name), sub: format!("{} · {project}", r.runs[0].note), go: Go::Routines });
    }
    out
}

/// The three newest chats.
pub fn recent(s: &Snapshot) -> Vec<&Chat> {
    let mut v: Vec<&Chat> = s.chats.iter().filter(|c| !c.archived).collect();
    v.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
    v.truncate(3);
    v
}

/// The first two routines that are not paused.
pub fn upcoming(s: &Snapshot) -> Vec<&Routine> {
    s.routines.iter().filter(|r| !r.paused).take(2).collect()
}

/// The project a new chat goes to: the one picked, else the open one, else the workspace default, else the first.
pub fn target_project(s: &Snapshot, picked: Option<&str>, open: Option<&str>) -> Option<String> {
    let live = |id: &&str| s.projects.iter().any(|p| p.id == *id && !p.archived);
    picked.filter(live).or(open.filter(live)).or(Some(s.workspace.default_project.as_str()).filter(live)).map(String::from).or_else(|| s.projects.iter().find(|p| !p.archived).map(|p| p.id.clone()))
}

/// `POST …/chats` body for a message typed on Home; `None` for a blank one.
pub fn chat_body(project: &str, mode: &str, text: &str) -> Option<Value> {
    let t = text.trim();
    (!t.is_empty()).then(|| json!({ "project": project, "title": t.chars().take(60).collect::<String>(), "text": format!("{}{t}", mode_prefix(mode)) }))
}

pub fn chats_path(workspace: &str) -> String {
    format!("/api/ws/{}/chats", seg(workspace))
}
