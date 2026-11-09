use workspaceai_client::model::*;
use workspaceai_client::types::*;

fn project(id: &str, name: &str, parent: Option<&str>, archived: bool, general: bool) -> Project {
    Project { id: id.into(), name: name.into(), parent: parent.map(Into::into), archived, is_general: general.then_some(true) }
}
fn chat(id: &str, project: &str, title: &str, updated: i64, archived: bool) -> Chat {
    Chat { id: id.into(), project: project.into(), title: title.into(), archived, updated_at: updated, agent: String::new(), full_access: false }
}
fn open_model() -> Model {
    let mut m = Model::default();
    m.active_ws = Some("w1".into());
    m.snapshot.chats = vec![chat("c1", "p1", "One", 1, false), chat("c2", "p1", "Two", 2, false)];
    m.open_chat("c1".into());
    m
}
#[test]
fn snapshot_and_state_events_set_refresh_flags_only_for_the_active_workspace() {
    let mut m = open_model();
    m.apply(UiEvent::Snapshot { workspace: "other".into() });
    assert!(!m.needs.snapshot);
    m.apply(UiEvent::Snapshot { workspace: "w1".into() });
    assert!(m.needs.snapshot);
    m.apply(UiEvent::State { workspace: None });
    assert!(m.needs.state);
}

#[test]
fn toast_is_kept_and_unknown_events_do_nothing() {
    let mut m = open_model();
    m.apply(UiEvent::Toast { text: "hi".into() });
    assert_eq!(m.toast.as_deref(), Some("hi"));
    let before = m.toast.clone();
    m.apply(UiEvent::Other);
    assert_eq!(m.toast, before);
}

#[test]
fn sidebar_nests_projects_hides_archived_and_orders_chats_newest_first() {
    let mut m = Model::default();
    m.snapshot.projects = vec![
        project("p2", "Work", None, false, false),
        project("p1", "General", None, false, true),
        project("p3", "Sub", Some("p2"), false, false),
        project("p4", "Old", None, true, false),
    ];
    m.snapshot.chats = vec![
        chat("c1", "p2", "older", 1, false),
        chat("c2", "p2", "newer", 5, false),
        chat("c3", "p2", "gone", 9, true),
        chat("c4", "p3", "in sub", 3, false),
        chat("c5", "p1", "gen", 2, false),
        chat("c6", "p4", "in archived", 7, false),
    ];
    let flat: Vec<(String, usize)> = m.sidebar().into_iter().map(|r| (r.title, r.depth)).collect();
    assert_eq!(
        flat,
        vec![
            ("General".to_string(), 0),
            ("gen".to_string(), 1),
            ("Work".to_string(), 0),
            ("newer".to_string(), 1),
            ("older".to_string(), 1),
            ("Sub".to_string(), 1),
            ("in sub".to_string(), 2),
        ]
    );
}

fn perm(id: &str) -> PermissionRequest {
    PermissionRequest { id: id.into(), title: format!("Run {id}"), detail: String::new() }
}
fn ask(ws: &str, c: &str, id: &str) -> UiEvent {
    UiEvent::Approval { workspace: ws.into(), chat: c.into(), request: perm(id) }
}

#[test]
fn permission_requests_are_kept_per_chat_without_duplicates() {
    let mut m = open_model();
    m.apply(ask("w1", "c1", "r1"));
    m.apply(ask("w1", "c1", "r1"));
    m.apply(ask("w1", "c1", "r2"));
    m.apply(ask("w1", "c2", "r3"));
    assert_eq!(m.pending_permissions().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["r1", "r2"]);
    assert_eq!(m.permissions["c2"].len(), 1);
    assert!(m.toast.is_none(), "approvals have real cards now, no stall toast");
}

#[test]
fn permission_requests_from_another_workspace_are_ignored() {
    let mut m = open_model();
    m.apply(ask("other", "c1", "r1"));
    assert!(m.pending_permissions().is_empty());
}

#[test]
fn answered_requests_can_be_dropped() {
    let mut m = open_model();
    m.apply(ask("w1", "c1", "r1"));
    m.drop_permission("c1", "r1");
    assert!(m.pending_permissions().is_empty());
}

#[test]
fn archived_chats_list_newest_first_skipping_archived_projects() {
    let mut m = Model::default();
    m.snapshot.projects = vec![project("p1", "A", None, false, true), project("p2", "B", None, true, false)];
    m.snapshot.chats = vec![chat("c1", "p1", "old", 1, true), chat("c2", "p1", "new", 2, true), chat("c3", "p1", "live", 3, false), chat("c4", "p2", "gone", 4, true)];
    let ids: Vec<_> = m.archived_chats().into_iter().map(|c| c.0).collect();
    assert_eq!(ids, vec!["c2", "c1"]);
}

#[test]
fn ctrl_number_picks_the_nth_workspace() {
    let mut m = Model::default();
    m.workspaces = ["a", "b"].iter().map(|id| serde_json::from_value(serde_json::json!({"id": id, "name": id})).unwrap()).collect();
    assert_eq!(m.workspace_for_shortcut("2"), Some("b"));
    assert_eq!(m.workspace_for_shortcut("3"), None);
    assert_eq!(m.workspace_for_shortcut("0"), None);
    assert_eq!(m.workspace_for_shortcut("k"), None);
}
