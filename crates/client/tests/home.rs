use serde_json::json;
use workspaceai_client::approvals;
use workspaceai_client::home::*;
use workspaceai_client::types::{PermissionRequest, Snapshot};

fn snap() -> Snapshot {
    serde_json::from_value(json!({
        "workspace": { "defaultProject": "g", "agents": [{ "id": "claude", "name": "Claude Code" }] },
        "projects": [{ "id": "g", "name": "General" }, { "id": "x", "name": "Old", "archived": true }],
        "chats": [{ "id": "c1", "project": "g", "title": "One", "updatedAt": 1, "agent": "claude" }, { "id": "c2", "project": "g", "title": "Two", "updatedAt": 5 }, { "id": "c3", "project": "g", "updatedAt": 9, "archived": true }],
        "facts": [{ "id": "f", "text": "Prefers tabs", "status": "suggested" }, { "id": "g", "text": "k", "status": "approved" }],
        "routines": [{ "id": "r", "name": "Digest", "project": "g", "trigger": { "kind": "schedule", "cron": "0 9 * * *", "tz": "UTC" }, "what": { "kind": "prompt", "prompt": "p" }, "runs": [{ "id": "1", "status": "waiting", "note": "needs ok" }] }],
        "chatActivity": { "c1": "waiting" } })).unwrap()
}

#[test]
fn greeting_follows_the_hour() {
    assert!(greeting(8).starts_with("Good morning"));
    assert!(greeting(12).starts_with("Good afternoon"));
    assert!(greeting(22).starts_with("Good evening"));
}

#[test]
fn needs_lists_suggestions_and_waiting_runs() {
    let n = needs(&snap());
    assert_eq!(n[0].title, "Review 1 memory suggestion");
    assert_eq!(n[0].go, Go::Memory);
    assert_eq!((n[1].title.as_str(), n[1].sub.as_str()), ("Approve: Digest", "needs ok · General"));
}

#[test]
fn recent_skips_archived_and_is_newest_first() {
    assert_eq!(recent(&snap()).iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["c2", "c1"]);
}

#[test]
fn the_target_project_prefers_picked_then_open_then_default_and_skips_archived() {
    let s = snap();
    assert_eq!(target_project(&s, Some("x"), None).as_deref(), Some("g"));
    assert_eq!(target_project(&s, None, None).as_deref(), Some("g"));
}

#[test]
fn chat_body_prefixes_the_mode_and_ignores_blanks() {
    assert!(chat_body("g", "chat", "  ").is_none());
    let b = chat_body("g", "research", " why? ").unwrap();
    assert_eq!(b["title"], "why?");
    assert!(b["text"].as_str().unwrap().ends_with(": why?"));
    assert_eq!(chat_body("g", "chat", "hi").unwrap()["text"], "hi");
}

#[test]
fn approvals_name_the_agent_and_hide_empty_details() {
    let s = snap();
    let mut perms = std::collections::HashMap::new();
    perms.insert("c1".to_string(), vec![PermissionRequest { id: "p1".into(), title: "bash".into(), detail: "\"\"".into() }]);
    perms.insert("c2".to_string(), vec![PermissionRequest { id: "p2".into(), title: "edit".into(), detail: "a.txt".into() }]);
    let a = approvals::pending(&s, &perms);
    assert_eq!(a.iter().map(|x| (x.agent.as_str(), x.detail.as_str())).collect::<Vec<_>>(), vec![("opencode", "a.txt"), ("Claude Code", "")]);
    assert_eq!(approvals::reply_path("w", "c", "r"), "/api/ws/w/chats/c/permission/r");
}
