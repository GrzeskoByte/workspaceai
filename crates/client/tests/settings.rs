use serde_json::json;
use workspaceai_client::settings::*;
use workspaceai_client::types::Snapshot;

#[test]
fn personal_workspaces_have_every_page_unlocked() {
    let pages = pages(false);
    assert!(pages.iter().any(|p| p.id == "general") && pages.iter().any(|p| p.id == "engine"));
    assert!(pages.iter().all(|p| !p.locked));
}

#[test]
fn org_workspaces_lock_policy_pages_but_not_general_or_memory() {
    let pages = pages(true);
    let locked = |id: &str| pages.iter().find(|p| p.id == id).unwrap().locked;
    for id in ["providers", "members", "artifacts", "data", "web", "agents"] {
        assert!(locked(id), "{id}");
    }
    for id in ["general", "memory", "engine"] {
        assert!(!locked(id), "{id}");
    }
}

#[test]
fn missing_switches_mean_on_where_the_supervisor_says_so() {
    assert!(switch_on(None));
    assert!(switch_on(Some(true)));
    assert!(!switch_on(Some(false)));
}

#[test]
fn workspace_patches_are_single_field_objects() {
    assert_eq!(patch("useMemory", json!(false)), json!({"useMemory": false}));
    assert_eq!(patch("instructions", json!("Be brief")), json!({"instructions": "Be brief"}));
}

#[test]
fn engine_buttons_follow_the_engine_state() {
    use EngineAction::*;
    assert_eq!(engine_actions("running"), vec![Restart, Stop]);
    assert_eq!(engine_actions("connected"), vec![Restart, Stop]);
    assert_eq!(engine_actions("stopped"), vec![Start]);
    assert_eq!(engine_actions("error"), vec![Start]);
    assert!(engine_actions("starting").is_empty());
    assert!(engine_actions("restarting").is_empty());
    assert_eq!(engine_actions("something new"), vec![Start], "unknown states offer Start rather than nothing");
}

#[test]
fn engine_action_paths() {
    assert_eq!(EngineAction::Start.path("w1"), "/api/ws/w1/engine/start");
    assert_eq!(EngineAction::Stop.path("w1"), "/api/ws/w1/engine/stop");
    assert_eq!(EngineAction::Restart.path("w1"), "/api/ws/w1/engine/restart");
}

#[test]
fn snapshot_reads_the_workspace_settings_and_engine_status() {
    let s: Snapshot = serde_json::from_str(
        r#"{"workspace":{"id":"w1","name":"Personal","kind":"personal","instructions":"hi","defaultModel":"a/b","useMemory":false,"suggestMemories":true},
            "engine":{"state":"running","url":"http://127.0.0.1:4096","port":4096,"version":"1.2.3","sidecars":[{"name":"od","state":"running"}]},
            "projects":[],"chats":[]}"#,
    )
    .unwrap();
    assert_eq!((s.workspace.name.as_str(), s.workspace.instructions.as_str(), s.workspace.use_memory), ("Personal", "hi", Some(false)));
    assert_eq!(s.workspace.memory_recall, None);
    assert_eq!((s.engine.state.as_str(), s.engine.port, s.engine.sidecars[0].name.as_str()), ("running", 4096, "od"));
    let bare: Snapshot = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(bare.engine.state, "");
}
