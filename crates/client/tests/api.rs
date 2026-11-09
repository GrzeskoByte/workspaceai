use serde_json::json;
use std::sync::Arc;
use workspaceai_client::api::{split_request, Api};
use workspaceai_client::types::UiEvent;
use workspaceai_supervisor::Supervisor;

fn api(name: &str) -> Api {
    let dir = std::env::temp_dir().join(format!("wsai-api-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("HARNESS_KEYCHAIN", "memory");
    Api::new(Arc::new(Supervisor::new(dir).unwrap()))
}

#[test]
fn get_calls_the_supervisor_in_process() {
    let v = api("state").get("/api/state").unwrap();
    assert!(v["workspaces"].is_array(), "{v}");
}

#[test]
fn unknown_routes_are_404_with_a_message() {
    let e = api("404").get("/nope").unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (404, "Not found"));
    let e = api("post").post("/api/ws/invalid%20id/chats", Some(&json!({}))).unwrap_err();
    assert_eq!(e.status, 404);
}

#[test]
fn paths_are_decoded_and_unsafe_segments_cannot_resplit() {
    let (p, q) = split_request("/api/ws/a%20b/files?q=x%26y&n=1");
    assert_eq!(p, "/api/ws/a b/files");
    assert_eq!((q["q"].as_str(), q["n"].as_str()), ("x&y", "1"));
    assert_eq!(split_request("/api/ws/..%2Ffacts/x").0, "/api/ws/invalid%20id/x");
    assert_eq!(split_request("/api/ws/%2E%2E/x").0, "/api/ws/invalid%20id/x");
}

#[test]
fn supervisor_events_reach_the_channel() {
    let api = api("events");
    let (tx, rx) = std::sync::mpsc::channel();
    api.events(tx);
    api.sup.with_core(|c| c.emit(workspaceai_supervisor::UiEvent::Toast { text: "hi".into() })).unwrap();
    assert_eq!(rx.try_recv().unwrap(), UiEvent::Toast { text: "hi".into() });
}
