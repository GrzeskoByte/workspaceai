//! Memory facts: grouping for display and the request bodies for `/api/ws/:id/facts`.

use crate::types::{Fact, Project};
use serde_json::{json, Value};

const MAX_FACT: usize = 400;

#[derive(Debug, Clone, PartialEq)]
pub struct Group<'a> {
    /// "Workspace", the project's name, or the raw scope id when the project is gone.
    pub label: String,
    pub scope: String,
    pub suggested: Vec<&'a Fact>,
    pub approved: Vec<&'a Fact>,
}

/// Workspace facts first, then one group per project that has facts (in project order, then orphans).
pub fn group<'a>(facts: &'a [Fact], projects: &[Project]) -> Vec<Group<'a>> {
    let mut scopes: Vec<(String, String)> = vec![("ws".into(), "Workspace".into())];
    scopes.extend(projects.iter().map(|p| (p.id.clone(), p.name.clone())));
    for f in facts {
        if !scopes.iter().any(|(id, _)| *id == f.scope) {
            scopes.push((f.scope.clone(), f.scope.clone()));
        }
    }
    scopes
        .into_iter()
        .filter_map(|(scope, label)| {
            let mine: Vec<&Fact> = facts.iter().filter(|f| f.scope == scope).collect();
            (!mine.is_empty()).then(|| Group {
                label,
                scope,
                suggested: mine.iter().copied().filter(|f| f.status == "suggested").collect(),
                approved: mine.iter().copied().filter(|f| f.status != "suggested").collect(),
            })
        })
        .collect()
}

/// `POST …/facts` body: trimmed, capped at 400 characters, `None` when blank.
pub fn add_body(text: &str, scope: &str) -> Option<Value> {
    let t = text.trim();
    (!t.is_empty()).then(|| json!({ "text": t.chars().take(MAX_FACT).collect::<String>(), "scope": scope }))
}

pub fn approve_body() -> Value {
    json!({ "status": "approved" })
}

pub fn edit_body(text: &str) -> Option<Value> {
    let t = text.trim();
    (!t.is_empty()).then(|| json!({ "text": t.chars().take(MAX_FACT).collect::<String>() }))
}
