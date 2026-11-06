//! Acceptance: `state.json`, `harness.json` (workspace) and `app.json` as the TS supervisor writes them
//! round-trip through serde without losing a field. The fixtures are hand-made in the shape of a demo
//! workspace; set `WORKSPACEAI_LIVE_HARNESS` to a `.harness` folder to run the same check on real files in place.
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::path::Path;
use workspaceai_shared::types::*;

fn round_trip<T: DeserializeOwned + Serialize>(text: &str, what: &str) -> T {
    let before: Value = serde_json::from_str(text).unwrap();
    let typed: T = serde_json::from_str(text).unwrap_or_else(|e| panic!("{what}: {e}"));
    let after = serde_json::to_value(&typed).unwrap();
    assert_eq!(before, after, "{what} lost or changed a field");
    typed
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

#[test]
fn workspace_json_round_trips() {
    let w: Workspace = round_trip(&fixture("workspace.json"), "workspace.json");
    assert_eq!((w.kind, w.memory, w.keep_chats), (WorkspaceKind::Personal, MemoryBackend::Both, KeepChats::Forever));
    assert_eq!(w.extra.get("someFutureSetting"), Some(&Value::String("kept".into())));
    assert_eq!(w.providers[0].base_url.as_deref(), Some("http://localhost:11434/v1"));
    assert_eq!(w.mcp[1].extra["futureField"], serde_json::json!({ "x": [1, 2, null] }));
}

#[test]
fn state_json_round_trips() {
    let s: PersistedState = round_trip(&fixture("state.json"), "state.json");
    assert_eq!((s.projects.len(), s.chats.len(), s.artifacts.len(), s.routines.len()), (3, 2, 1, 2));
    assert_eq!(s.chats[0].web, None);
    assert_eq!(s.chats[1].transcript.as_ref().unwrap()[1].artifact, Some(None)); // explicit null kept
    assert_eq!(s.chats[1].transcript.as_ref().unwrap()[0].artifact, None); // absent stays absent
    assert_eq!(s.artifacts[0].versions[0].od, Some(None));
    assert!(matches!(s.routines[1].trigger, Trigger::Event { .. }));
    assert!(s.extra.contains_key("agentModels") && s.extra.contains_key("agentCommands"));
}

#[test]
fn app_json_round_trips() {
    let a: AppSettings = round_trip(&fixture("app.json"), "app.json");
    assert!(a.onboarded && a.active_workspace.as_deref() == Some("ws_demo1"));
}

#[test]
fn live_harness_round_trips_when_pointed_at() {
    let Ok(home) = std::env::var("WORKSPACEAI_LIVE_HARNESS") else {
        eprintln!("skipped: set WORKSPACEAI_LIVE_HARNESS=<.harness folder> to check real files");
        return;
    };
    let home = Path::new(&home);
    round_trip::<AppSettings>(&std::fs::read_to_string(home.join("app.json")).unwrap(), "app.json");
    let mut n = 0;
    for ws in std::fs::read_dir(home.join("ws")).unwrap().flatten() {
        if let Ok(t) = std::fs::read_to_string(ws.path().join("config/harness.json")) {
            round_trip::<Workspace>(&t, "harness.json");
            n += 1;
        }
        if let Ok(t) = std::fs::read_to_string(ws.path().join("data/state.json")) {
            round_trip::<PersistedState>(&t, "state.json");
            n += 1;
        }
    }
    eprintln!("round-tripped {n} live files");
}
