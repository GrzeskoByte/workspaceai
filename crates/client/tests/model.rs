use workspaceai_client::model::*;
use workspaceai_client::types::*;

fn project(id: &str, name: &str, parent: Option<&str>, archived: bool, general: bool) -> Project {
    Project { id: id.into(), name: name.into(), parent: parent.map(Into::into), archived, is_general: general.then_some(true) }
}
fn chat(id: &str, project: &str, title: &str, updated: i64, archived: bool) -> Chat {
    Chat { id: id.into(), project: project.into(), title: title.into(), archived, updated_at: updated }
}
fn msg(id: &str, role: &str, text: &str) -> ChatMessage {
    ChatMessage { id: id.into(), role: role.into(), text: text.into(), tools: vec![], error: None, created_at: 0 }
}
fn open_model() -> Model {
    let mut m = Model::default();
    m.active_ws = Some("w1".into());
    m.snapshot.chats = vec![chat("c1", "p1", "One", 1, false), chat("c2", "p1", "Two", 2, false)];
    m.open_chat("c1".into(), vec![msg("u1", "user", "hi")]);
    m
}
fn delta(ws: &str, c: &str, id: &str, d: &str) -> UiEvent {
    UiEvent::ChatDelta { workspace: ws.into(), chat: c.into(), message_id: id.into(), delta: d.into() }
}

#[test]
fn delta_starts_then_extends_the_assistant_message_of_the_open_chat() {
    let mut m = open_model();
    m.apply(delta("w1", "c1", "a1", "Hel"));
    m.apply(delta("w1", "c1", "a1", "lo ✓"));
    assert_eq!(m.messages.len(), 2);
    assert_eq!((m.messages[1].role.as_str(), m.messages[1].text.as_str()), ("assistant", "Hello ✓"));
}

#[test]
fn delta_for_another_chat_or_workspace_leaves_the_open_chat_alone() {
    let mut m = open_model();
    m.apply(delta("w1", "c2", "a1", "x"));
    m.apply(delta("other", "c1", "a2", "y"));
    assert_eq!(m.messages.len(), 1);
    assert_eq!(m.snapshot.chat_activity.get("c2").map(String::as_str), Some("working"));
}

#[test]
fn tool_steps_replace_the_same_tool_and_append_new_ones() {
    let mut m = open_model();
    let tool = |name: &str, status: &str| ToolStep { name: name.into(), status: status.into(), detail: None };
    let ev = |t| UiEvent::ChatTool { workspace: "w1".into(), chat: "c1".into(), message_id: "a1".into(), tool: t };
    m.apply(ev(tool("bash", "running")));
    m.apply(ev(tool("bash", "ok")));
    m.apply(ev(tool("read", "ok")));
    let tools = &m.messages[1].tools;
    assert_eq!(tools.iter().map(|t| (t.name.as_str(), t.status.as_str())).collect::<Vec<_>>(), vec![("bash", "ok"), ("read", "ok")]);
}

#[test]
fn done_clears_activity_and_asks_for_a_reload() {
    let mut m = open_model();
    m.apply(delta("w1", "c1", "a1", "x"));
    m.apply(UiEvent::ChatDone { workspace: "w1".into(), chat: "c1".into(), outcome: "ok".into() });
    assert!(!m.snapshot.chat_activity.contains_key("c1"));
    assert!(m.needs.snapshot && m.needs.messages);
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
    let before = m.messages.clone();
    m.apply(UiEvent::Other);
    assert_eq!(m.messages, before);
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
    m.snapshot.chat_activity.insert("c2".into(), "working".into());
    let flat: Vec<(String, usize, bool)> = m.sidebar().into_iter().map(|r| (r.title, r.depth, r.working)).collect();
    assert_eq!(
        flat,
        vec![
            ("General".to_string(), 0, false),
            ("gen".to_string(), 1, false),
            ("Work".to_string(), 0, false),
            ("newer".to_string(), 1, true),
            ("older".to_string(), 1, false),
            ("Sub".to_string(), 1, false),
            ("in sub".to_string(), 2, false),
        ]
    );
}

#[test]
fn opening_a_chat_resets_messages_and_flags() {
    let mut m = open_model();
    m.needs.messages = true;
    m.open_chat("c2".into(), vec![]);
    assert_eq!(m.open_chat_id.as_deref(), Some("c2"));
    assert!(m.messages.is_empty() && !m.needs.messages);
}
