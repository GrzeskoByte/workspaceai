//! New artifact: Open Design's pipeline — brief, questions, direction, design system, generate, critique,
//! handoff, memory. Requests and replies of `/api/ws/:id/pipeline`; the window only draws and calls these.

use crate::api::seg;
use crate::workspace::DesignSystem;
use serde::Deserialize;
use serde_json::{json, Value};

pub const STAGES: [&str; 8] = ["Brief", "Questions", "Direction", "Design system", "Generate", "Critique", "Handoff", "Memory"];

/// (type, label, note)
pub const OUTPUTS: [(&str, &str, &str); 8] = [
    ("Doc", "Doc", "report, runbook, memo"),
    ("Deck", "Deck", "slides"),
    ("Prototype", "Prototype", "a working HTML page"),
    ("Sheet", "Sheet", "a table of data"),
    ("LiveArtifact", "Live artifact", "HTML that re-renders from data"),
    ("Image", "Image", "an SVG illustration"),
    ("Video", "Video", "storyboard and shot list"),
    ("DesignSystem", "Design system", "a DESIGN.md"),
];

/// (label, type, title, brief)
pub const TEMPLATES: [(&str, &str, &str, &str); 4] = [
    ("Landing page", "Prototype", "Beta landing page", "A landing page announcing a private beta: headline, how it works in three steps, and a sign-up call to action."),
    ("Teaser deck", "Deck", "Teaser deck", "A 6-slide teaser deck: problem, solution, demo, roadmap, the ask."),
    ("Runbook", "Doc", "Runbook", "A step-by-step runbook with owners, checks and rollback."),
    ("Budget sheet", "Sheet", "Budget", "A budget sheet with items, estimates and actuals."),
];

pub const DEFAULT_SYSTEM: &str = "claude-warm-editorial";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Direction {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub tone: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Role {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub scores: String,
    #[serde(default)]
    pub weight: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Issue {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub issue: String,
    #[serde(default)]
    pub fix: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Critique {
    #[serde(default)]
    pub scores: std::collections::HashMap<String, f64>,
    #[serde(default)]
    pub issues: Vec<Issue>,
    #[serde(default)]
    pub composite: f64,
    #[serde(default)]
    pub threshold: f64,
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub roles: Vec<Role>,
}

impl Critique {
    /// A role's score, 0 when the jury did not give one.
    pub fn score(&self, role: &str) -> f64 {
        self.scores.get(role).copied().unwrap_or(0.0)
    }
}

/// Bar colour for a score: green from 8, amber from 6, red below.
pub fn score_tone(v: f64) -> u32 {
    if v >= 8.0 { 0x7bd88f } else if v >= 6.0 { 0xf5a742 } else { 0xff6b6b }
}

fn base(ws: &str) -> String {
    format!("/api/ws/{}/pipeline", seg(ws))
}
pub fn start_path(ws: &str) -> String {
    base(ws)
}
/// `step`: questions, directions, generate, critique, fix, handoff or remember.
pub fn step_path(ws: &str, chat: &str, step: &str) -> String {
    format!("{}/{}/{step}", base(ws), seg(chat))
}
pub fn design_systems_path(ws: &str) -> String {
    format!("/api/ws/{}/design-systems", seg(ws))
}

fn title_or_type(title: &str, ty: &str) -> String {
    if title.trim().is_empty() { ty.to_string() } else { title.trim().to_string() }
}

/// Opens the pipeline chat. `None` while the brief is blank (the server refuses it too).
pub fn start_body(project: &str, ty: &str, title: &str, brief: &str) -> Option<Value> {
    (!brief.trim().is_empty() && !project.is_empty()).then(|| json!({ "project": project, "type": ty, "title": title_or_type(title, ty), "brief": brief.trim() }))
}

pub fn questions_body(ty: &str, title: &str, brief: &str) -> Value {
    json!({ "type": ty, "title": title_or_type(title, ty), "brief": brief.trim() })
}

pub fn directions_body(ty: &str, title: &str, brief: &str, qa: &[(String, String)]) -> Value {
    json!({ "type": ty, "title": title_or_type(title, ty), "brief": brief.trim(), "answers": qa.iter().map(|(q, a)| json!({ "q": q, "a": a })).collect::<Vec<_>>() })
}

pub fn generate_body(ty: &str, title: &str, direction: &Direction, system: &str, system_title: &str) -> Value {
    json!({ "type": ty, "title": title_or_type(title, ty), "direction": { "name": direction.name, "summary": direction.summary, "tone": direction.tone }, "designSystem": system, "designSystemTitle": system_title })
}

pub fn critique_body(artifact: &str) -> Value {
    json!({ "artifact": artifact })
}

pub fn fix_body(artifact: &str, issues: &[Issue]) -> Value {
    json!({ "artifact": artifact, "issues": issues.iter().map(|i| json!({ "role": i.role, "issue": i.issue, "fix": i.fix })).collect::<Vec<_>>() })
}

/// A free-text tweak is applied through the same route as the jury's issues.
pub fn tweak_issues(text: &str) -> Option<Vec<Issue>> {
    let t = text.trim();
    (!t.is_empty()).then(|| vec![Issue { role: "tweak".into(), issue: "Requested change".into(), fix: t.into() }])
}

pub fn tweak_suggestions(html: bool) -> [&'static str; 4] {
    if html { ["Split hero layout", "Compact density", "Use the accent for the main button only", "Add a mobile menu"] } else { ["Shorter", "Add a summary at the top", "More specific numbers", "Friendlier tone"] }
}

pub fn remember_body(artifact: &str, direction: Option<&Direction>, system_title: &str) -> Value {
    json!({ "artifact": artifact, "direction": direction.map(|d| json!({ "name": d.name, "summary": d.summary, "tone": d.tone })), "designSystemTitle": system_title })
}

pub fn parse_questions(reply: &Value) -> Vec<String> {
    reply["questions"].as_array().map(|l| l.iter().filter_map(|q| q.as_str().map(String::from)).collect()).unwrap_or_default()
}

pub fn parse_directions(reply: &Value) -> Vec<Direction> {
    serde_json::from_value(reply["directions"].clone()).unwrap_or_default()
}

pub fn parse_critique(reply: &Value) -> Option<Critique> {
    serde_json::from_value(reply.clone()).ok()
}

/// The design system the workspace already uses (by title), else the default.
pub fn initial_system(systems: &[DesignSystem], workspace_title: &str) -> String {
    systems.iter().find(|d| d.title == workspace_title).map_or(DEFAULT_SYSTEM.into(), |d| d.id.clone())
}

/// Systems matching the search (title and summary), at most 80.
pub fn filter_systems<'a>(systems: &'a [DesignSystem], query: &str) -> Vec<&'a DesignSystem> {
    let q = query.trim().to_lowercase();
    systems.iter().filter(|d| q.is_empty() || format!("{} {}", d.title, d.summary).to_lowercase().contains(&q)).take(80).collect()
}

pub fn system_note(d: &DesignSystem) -> &str {
    if d.source == "harness" { "WorkspaceAI default" } else { &d.summary }
}

/// Whether the stage button in the side column can be pressed: only stages already reached, plus the
/// generated ones once there is an artifact.
pub fn stage_enabled(i: usize, stage: usize, has_artifact: bool) -> bool {
    i <= stage || (has_artifact && i >= 4)
}
