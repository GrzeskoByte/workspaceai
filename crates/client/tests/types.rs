use workspaceai_client::types::*;

#[test]
fn app_state_lists_workspaces_and_ignores_the_rest() {
    let s: AppState = serde_json::from_str(r#"{"settings":{"x":1,"activeWorkspace":"w1"},"computers":[],"workspaces":[{"id":"w1","name":"Personal","kind":"personal","initials":"P","accent":"blue","note":"n"}]}"#).unwrap();
    assert_eq!(s.settings.active_workspace.as_deref(), Some("w1"));
    assert_eq!(s.workspaces.len(), 1);
    assert_eq!((s.workspaces[0].id.as_str(), s.workspaces[0].initials.as_str()), ("w1", "P"));
}

#[test]
fn snapshot_reads_projects_and_sessions() {
    let s: Snapshot = serde_json::from_str(
        r#"{"workspace":{"id":"w1"},"projects":[{"id":"p1","name":"General","parent":null,"archived":false,"isGeneral":true},{"id":"p2","name":"Sub","parent":"p1","archived":true}],
            "chats":[{"id":"c1","project":"p1","title":"Hi","archived":false,"updatedAt":5,"createdAt":1}],
            "artifacts":[]}"#,
    )
    .unwrap();
    assert_eq!(s.projects[0].is_general, Some(true));
    assert_eq!(s.projects[1].parent.as_deref(), Some("p1"));
    assert_eq!((s.chats[0].updated_at, s.chats[0].full_access), (5, false));
}

#[test]
fn snapshot_tolerates_missing_optional_parts() {
    let s: Snapshot = serde_json::from_str(r#"{"projects":[],"chats":[]}"#).unwrap();
    assert!(s.chats.is_empty());
}

#[test]
fn session_reads_agent_and_full_access() {
    let s: Chat = serde_json::from_str(r#"{"id":"c","project":"p","agent":"claude","fullAccess":true,"sid":"x","launched":true}"#).unwrap();
    assert_eq!((s.agent.as_str(), s.full_access), ("claude", true));
}

#[test]
fn app_state_without_settings_has_no_active_workspace() {
    let s: AppState = serde_json::from_str(r#"{"workspaces":[]}"#).unwrap();
    assert_eq!(s.settings.active_workspace, None);
}
