use serde_json::json;
use workspaceai_client::facts::*;
use workspaceai_client::types::{Fact, Project, Snapshot};

fn fact(id: &str, scope: &str, status: &str, text: &str) -> Fact {
    Fact { id: id.into(), text: text.into(), scope: scope.into(), status: status.into(), source: None, created_at: 0 }
}
fn project(id: &str, name: &str) -> Project {
    Project { id: id.into(), name: name.into(), parent: None, archived: false, is_general: None }
}

#[test]
fn snapshot_carries_facts() {
    let s: Snapshot = serde_json::from_str(r#"{"facts":[{"id":"f1","text":"Prefers tabs","scope":"ws","status":"suggested","source":"chat","createdAt":5}]}"#).unwrap();
    assert_eq!((s.facts[0].text.as_str(), s.facts[0].status.as_str(), s.facts[0].source.as_deref()), ("Prefers tabs", "suggested", Some("chat")));
    assert!(serde_json::from_str::<Snapshot>("{}").unwrap().facts.is_empty());
}

#[test]
fn groups_put_the_workspace_first_then_projects_with_facts_only() {
    let facts = vec![fact("a", "p2", "approved", "two"), fact("b", "ws", "approved", "ws one"), fact("c", "p1", "approved", "one")];
    let projects = vec![project("p1", "Alpha"), project("p2", "Beta"), project("p3", "Empty")];
    let g = group(&facts, &projects);
    assert_eq!(g.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), vec!["Workspace", "Alpha", "Beta"]);
}

#[test]
fn suggested_facts_are_separated_from_approved_ones() {
    let facts = vec![fact("a", "ws", "approved", "kept"), fact("b", "ws", "suggested", "maybe")];
    let g = group(&facts, &[]);
    assert_eq!(g[0].approved.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), vec!["a"]);
    assert_eq!(g[0].suggested.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), vec!["b"]);
}

#[test]
fn facts_of_an_unknown_project_still_show_under_their_id() {
    let facts = [fact("a", "gone", "approved", "orphan")];
    let g = group(&facts, &[]);
    assert_eq!(g[0].label, "gone");
}

#[test]
fn add_body_trims_caps_and_refuses_blank() {
    assert_eq!(add_body("  Likes Rust  ", "ws"), Some(json!({"text": "Likes Rust", "scope": "ws"})));
    assert_eq!(add_body("   ", "ws"), None);
    let long = "ł".repeat(500);
    assert_eq!(add_body(&long, "ws").unwrap()["text"].as_str().unwrap().chars().count(), 400);
}

#[test]
fn approve_and_edit_bodies() {
    assert_eq!(approve_body(), json!({"status": "approved"}));
    assert_eq!(edit_body(" new "), Some(json!({"text": "new"})));
    assert_eq!(edit_body(""), None);
}
