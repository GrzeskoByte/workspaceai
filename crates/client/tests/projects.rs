use serde_json::json;
use workspaceai_client::projects::*;
use workspaceai_client::types::Project;

fn p(id: &str, name: &str, parent: Option<&str>, archived: bool, general: bool) -> Project {
    Project { id: id.into(), name: name.into(), parent: parent.map(Into::into), archived, is_general: general.then_some(true) }
}

fn names(nodes: &[Node]) -> Vec<(String, usize)> {
    nodes.iter().map(|n| (n.name.clone(), n.depth)).collect()
}

#[test]
fn tree_nests_children_and_puts_general_first() {
    let ps = vec![p("b", "Work", None, false, false), p("g", "General", None, false, true), p("c", "API", Some("b"), false, false), p("d", "Deep", Some("c"), false, false)];
    assert_eq!(names(&tree(&ps, false)), vec![("General".into(), 0), ("Work".into(), 0), ("API".into(), 1), ("Deep".into(), 2)]);
}

#[test]
fn archived_projects_are_hidden_unless_asked_for() {
    let ps = vec![p("a", "Live", None, false, false), p("b", "Old", None, true, false), p("c", "Child of old", Some("b"), false, false)];
    assert_eq!(names(&tree(&ps, false)), vec![("Live".into(), 0)]);
    let all = tree(&ps, true);
    assert_eq!(names(&all), vec![("Live".into(), 0), ("Old".into(), 0), ("Child of old".into(), 1)]);
    assert!(all[1].archived && !all[0].archived);
}

#[test]
fn a_project_whose_parent_is_missing_is_shown_at_the_top() {
    let ps = vec![p("a", "Orphan", Some("gone"), false, false)];
    assert_eq!(names(&tree(&ps, false)), vec![("Orphan".into(), 0)]);
}

#[test]
fn a_parent_cycle_cannot_loop_forever() {
    let ps = vec![p("a", "A", Some("b"), false, false), p("b", "B", Some("a"), false, false)];
    let t = tree(&ps, false);
    assert!(t.len() <= 2);
}

#[test]
fn only_general_is_protected_from_deletion() {
    let ps = vec![p("g", "General", None, false, true), p("a", "Other", None, false, false)];
    let t = tree(&ps, false);
    assert!(!t[0].can_delete && t[1].can_delete);
}

#[test]
fn create_and_rename_bodies() {
    assert_eq!(create_body("  Billing  ", None), Some(json!({"name": "Billing", "parent": null})));
    assert_eq!(create_body("Sub", Some("p1")), Some(json!({"name": "Sub", "parent": "p1"})));
    assert_eq!(create_body("   ", None), None);
    assert_eq!(rename_body(" New "), Some(json!({"name": "New"})));
    assert_eq!(rename_body(""), None);
    assert_eq!(create_body(&"ż".repeat(300), None).unwrap()["name"].as_str().unwrap().chars().count(), 80);
}

#[test]
fn paths_encode_their_ids() {
    assert_eq!(create_path("w1"), "/api/ws/w1/projects");
    assert_eq!(item_path("w/1", "p1"), "/api/ws/w%2F1/projects/p1");
    assert_eq!(archive_path("w1", ".."), "/api/ws/w1/projects/invalid%20id/archive");
    assert_eq!(restore_path("w1", "p1"), "/api/ws/w1/projects/p1/restore");
}
