//! Projects: the tree for display and the requests for create / rename / archive / restore / delete.

use crate::api::seg;
use crate::types::Project;
use serde_json::{json, Value};

const MAX_NAME: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub depth: usize,
    pub archived: bool,
    pub is_general: bool,
    /// Every project but General can be deleted.
    pub can_delete: bool,
}

/// Projects as a tree, General first. Archived ones (and their subtrees) only when `include_archived`.
/// A project whose parent is missing shows at the top level.
pub fn tree(projects: &[Project], include_archived: bool) -> Vec<Node> {
    let visible: Vec<&Project> = projects.iter().filter(|p| include_archived || !p.archived).collect();
    let known = |id: &str| projects.iter().any(|p| p.id == id);
    let mut roots: Vec<&Project> = visible.iter().copied().filter(|p| p.parent.as_deref().is_none_or(|par| !known(par))).collect();
    roots.sort_by_key(|p| p.is_general != Some(true)); // stable: General first
    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for r in roots {
        push(r, 0, &visible, &mut out, &mut seen);
    }
    out
}

fn push<'a>(p: &'a Project, depth: usize, all: &[&'a Project], out: &mut Vec<Node>, seen: &mut Vec<&'a str>) {
    if seen.contains(&p.id.as_str()) {
        return; // a parent cycle would otherwise recurse forever
    }
    seen.push(&p.id);
    let general = p.is_general == Some(true);
    out.push(Node { id: p.id.clone(), name: p.name.clone(), depth, archived: p.archived, is_general: general, can_delete: !general });
    for child in all.iter().copied().filter(|c| c.parent.as_deref() == Some(p.id.as_str())) {
        push(child, depth + 1, all, out, seen);
    }
}

fn clean(name: &str) -> Option<String> {
    let t = name.trim();
    (!t.is_empty()).then(|| t.chars().take(MAX_NAME).collect())
}

/// `POST …/projects` body; `parent` is a project id or `None` for the top level.
pub fn create_body(name: &str, parent: Option<&str>) -> Option<Value> {
    clean(name).map(|n| json!({ "name": n, "parent": parent }))
}

pub fn rename_body(name: &str) -> Option<Value> {
    clean(name).map(|n| json!({ "name": n }))
}

pub fn create_path(workspace: &str) -> String {
    format!("/api/ws/{}/projects", seg(workspace))
}

pub fn item_path(workspace: &str, project: &str) -> String {
    format!("{}/{}", create_path(workspace), seg(project))
}

pub fn archive_path(workspace: &str, project: &str) -> String {
    format!("{}/archive", item_path(workspace, project))
}

pub fn restore_path(workspace: &str, project: &str) -> String {
    format!("{}/restore", item_path(workspace, project))
}
