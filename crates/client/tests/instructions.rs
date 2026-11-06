use serde_json::json;
use workspaceai_client::instructions::*;
use workspaceai_client::types::Snapshot;

#[test]
fn layers_leave_out_the_projects_own_file() {
    let l = parse_layers(&json!({ "layers": [{ "name": "Workspace", "file": "a", "text": "x", "kind": "workspace" }, { "name": "This", "file": "b", "text": "y", "kind": "project" }] }));
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].name, "Workspace");
    assert_eq!(parse_agents(&json!({ "text": "rules" })), "rules");
}

fn snap() -> (serde_json::Value, Snapshot) {
    let v = json!({ "workspace": { "defaultModel": "ws-model" }, "projects": [
        { "id": "a", "name": "Work", "parent": null, "model": "opus", "folder": "/w" },
        { "id": "b", "name": "API", "parent": "a", "model": "", "inherit": false } ] });
    (v.clone(), serde_json::from_value(v).unwrap())
}

#[test]
fn meta_reads_inherit_model_and_folder() {
    let (raw, _) = snap();
    assert_eq!(meta(&raw, "a"), Meta { inherit: true, model: "opus".into(), folder: "/w".into() });
    assert!(!meta(&raw, "b").inherit);
    assert_eq!(meta(&raw, "zzz").model, "");
}

#[test]
fn the_model_comes_from_the_nearest_project_that_sets_one() {
    let (raw, s) = snap();
    assert_eq!(model_source(&raw, &s, "a"), "opus (here)");
    assert_eq!(model_source(&raw, &s, "b"), "opus (from Work)");
    let mut s2 = s.clone();
    s2.projects.retain(|p| p.id == "b");
    s2.projects[0].parent = None;
    assert_eq!(model_source(&raw, &s2, "b"), "ws-model (workspace default)");
}

#[test]
fn bodies() {
    assert_eq!(inherit_body(false), json!({ "inherit": false }));
    assert_eq!(model_body(""), json!({ "model": "" }));
    assert_eq!(agents_path("w", "p/q"), "/api/ws/w/projects/p%2Fq/agents");
}

#[test]
fn within_collects_the_subtree_and_survives_cycles() {
    let (_, mut s) = snap();
    assert_eq!(within(&s.projects, "a"), vec!["a", "b"]);
    s.projects[0].parent = Some("b".into());
    assert_eq!(within(&s.projects, "a").len(), 2);
}
