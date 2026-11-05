use workspaceai_client::types::*;

#[test]
fn app_state_lists_workspaces_and_ignores_the_rest() {
    let s: AppState = serde_json::from_str(r#"{"settings":{"x":1,"activeWorkspace":"w1"},"computers":[],"workspaces":[{"id":"w1","name":"Personal","kind":"personal","initials":"P","accent":"blue","note":"n"}]}"#).unwrap();
    assert_eq!(s.settings.active_workspace.as_deref(), Some("w1"));
    assert_eq!(s.workspaces.len(), 1);
    assert_eq!((s.workspaces[0].id.as_str(), s.workspaces[0].initials.as_str()), ("w1", "P"));
}

#[test]
fn snapshot_reads_projects_chats_and_activity() {
    let s: Snapshot = serde_json::from_str(
        r#"{"workspace":{"id":"w1"},"projects":[{"id":"p1","name":"General","parent":null,"archived":false,"isGeneral":true},{"id":"p2","name":"Sub","parent":"p1","archived":true}],
            "chats":[{"id":"c1","project":"p1","title":"Hi","archived":false,"updatedAt":5,"createdAt":1}],
            "chatActivity":{"c1":"working"},"artifacts":[]}"#,
    )
    .unwrap();
    assert_eq!(s.projects[0].is_general, Some(true));
    assert_eq!(s.projects[1].parent.as_deref(), Some("p1"));
    assert_eq!((s.chats[0].updated_at, s.chat_activity["c1"].as_str()), (5, "working"));
}

#[test]
fn snapshot_tolerates_missing_optional_parts() {
    let s: Snapshot = serde_json::from_str(r#"{"projects":[],"chats":[]}"#).unwrap();
    assert!(s.chat_activity.is_empty());
}

#[test]
fn chat_message_with_tools_error_and_unicode() {
    let m: ChatMessage = serde_json::from_str(
        r#"{"id":"m1","role":"assistant","text":"Zażółć gęślą jaźń","createdAt":9,"error":"boom","tools":[{"name":"read","status":"ok","detail":"a.ts"}]}"#,
    )
    .unwrap();
    assert_eq!(m.text, "Zażółć gęślą jaźń");
    assert_eq!(m.error.as_deref(), Some("boom"));
    assert_eq!((m.tools[0].name.as_str(), m.tools[0].status.as_str()), ("read", "ok"));
    let plain: ChatMessage = serde_json::from_str(r#"{"id":"m2","role":"user","text":"hi","createdAt":1}"#).unwrap();
    assert!(plain.tools.is_empty() && plain.error.is_none());
}

#[test]
fn app_state_without_settings_has_no_active_workspace() {
    let s: AppState = serde_json::from_str(r#"{"workspaces":[]}"#).unwrap();
    assert_eq!(s.settings.active_workspace, None);
}
