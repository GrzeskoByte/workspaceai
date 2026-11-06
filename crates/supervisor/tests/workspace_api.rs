//! Route tests for `PATCH /api/ws/:id`, facts and audit (the rest of `core.test.ts` is in `core.rs`).

mod common;

use common::{create, setup};
use serde_json::json;

#[test]
fn workspace_patch_rejects_unknown_keys() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let err = f.call("PATCH", &format!("/api/ws/{a}"), Some(json!({ "bogus": 1 }))).unwrap_err();
    assert_eq!(err.status, 400);
    assert_eq!(err.message, "Unknown setting bogus");
}

#[test]
fn workspace_patch_updates_allowed_fields_and_snapshot() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let snap = f.ok("PATCH", &format!("/api/ws/{a}"), Some(json!({ "name": "Renamed Workspace", "suggestMemories": false })));
    assert_eq!(snap["workspace"]["name"], "Renamed Workspace");
    assert_eq!(snap["workspace"]["initials"], "RW");
    assert_eq!(snap["workspace"]["suggestMemories"], false);
    let state = f.ok("GET", "/api/state", None);
    assert_eq!(state["workspaces"][0]["name"], "Renamed Workspace");
}

#[test]
fn facts_lifecycle_and_delete_needs_confirm() {
    let f = setup();
    let a = create(&f, "Personal", "personal");

    let first = f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Remember me", "scope": "ws" })));
    let fid = first["id"].as_str().unwrap().to_string();
    assert_eq!(first["status"], "approved");
    let second = f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Forget me", "scope": "ws" })));
    let sid = second["id"].as_str().unwrap().to_string();

    f.ok("PATCH", &format!("/api/ws/{a}/facts/{fid}"), Some(json!({ "text": "Remember me too" })));
    let snap = f.ok("GET", &format!("/api/ws/{a}/snapshot"), None);
    let texts: Vec<&str> = snap["facts"].as_array().unwrap().iter().map(|x| x["text"].as_str().unwrap()).collect();
    assert!(texts.contains(&"Remember me too"));

    f.ok("DELETE", &format!("/api/ws/{a}/facts/{sid}"), None);
    let snap = f.ok("GET", &format!("/api/ws/{a}/snapshot"), None);
    assert_eq!(snap["facts"].as_array().unwrap().len(), 1);

    let err = f.call("DELETE", &format!("/api/ws/{a}/facts"), None).unwrap_err();
    assert_eq!(err.status, 400);
    assert!(err.message.contains("confirm"), "message: {}", err.message);
    // Nothing was removed by the rejected call.
    assert_eq!(f.ok("GET", &format!("/api/ws/{a}/snapshot"), None)["facts"].as_array().unwrap().len(), 1);

    f.ok("DELETE", &format!("/api/ws/{a}/facts"), Some(json!({ "confirm": true })));
    let snap = f.ok("GET", &format!("/api/ws/{a}/snapshot"), None);
    assert!(snap["facts"].as_array().unwrap().is_empty());
}

#[test]
fn memory_changed_is_audited_and_emits_nothing_surprising() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    f.ok("POST", &format!("/api/ws/{a}/memory-changed"), None);
    let audit = f.ok("GET", &format!("/api/ws/{a}/audit"), None);
    let actions: Vec<&str> = audit["events"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert!(actions.contains(&"memory.suggest"));
}

#[test]
fn audit_route_returns_events_newest_first_with_limit() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    // create_workspace + fact writes leave a few audit lines behind.
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "one", "scope": "ws" })));
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "two", "scope": "ws" })));

    let all = f.ok("GET", &format!("/api/ws/{a}/audit"), None);
    let events = all["events"].as_array().unwrap();
    assert!(events.len() >= 3);
    assert_eq!(events[0]["action"], "memory.add");

    let limited = f.call_with_query("GET", &format!("/api/ws/{a}/audit"), &[("limit", "1")]).unwrap();
    assert_eq!(limited["events"].as_array().unwrap().len(), 1);
}

