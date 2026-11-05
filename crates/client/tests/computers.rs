use chrono::{TimeZone, Utc};
use serde_json::json;
use workspaceai_client::computers::*;
use workspaceai_client::types::{AppState, Computer};

fn computer(v: serde_json::Value) -> Computer {
    let mut base = json!({"id":"pc1","name":"Studio Mac","os":"darwin","status":"online","lastSeen":0,
        "capabilities":{"files":true,"commands":true,"browser":false,"computerUse":false,"wake":false},
        "access":{}, "activity":[]});
    for (k, x) in v.as_object().unwrap() {
        base[k] = x.clone();
    }
    serde_json::from_value(base).unwrap()
}

#[test]
fn app_state_carries_computers_and_tolerates_none() {
    let s: AppState = serde_json::from_value(json!({"workspaces":[],"computers":[{"id":"pc1","name":"A","os":"linux","status":"asleep","lastSeen":9,
        "capabilities":{"files":true,"commands":false,"browser":false,"computerUse":false,"wake":true},"access":{"w1":{"allowed":true,"folders":[{"path":"/x","mode":"ro"}]}},
        "activity":[{"at":1,"text":"Paired"}],"managed":true}]})).unwrap();
    assert_eq!((s.computers[0].name.as_str(), s.computers[0].status.as_str(), s.computers[0].managed), ("A", "asleep", Some(true)));
    assert!(serde_json::from_value::<AppState>(json!({})).unwrap().computers.is_empty());
}

#[test]
fn status_text_adds_last_seen_only_when_not_online() {
    let now = Utc.with_ymd_and_hms(2026, 10, 29, 10, 0, 0).unwrap();
    assert_eq!(status_text(&computer(json!({})), "UTC", now), "Online");
    let seen = Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap().timestamp_millis();
    assert_eq!(status_text(&computer(json!({"status":"offline","lastSeen":seen})), "UTC", now), "Offline · last seen Today 08:30");
    assert_eq!(status_text(&computer(json!({"status":"asleep","lastSeen":seen})), "UTC", now), "Asleep · last seen Today 08:30");
    assert_eq!(status_text(&computer(json!({"status":"weird"})), "UTC", now), "weird");
}

#[test]
fn capabilities_list_only_what_is_on() {
    assert_eq!(capability_text(&computer(json!({})).capabilities), "Files, commands");
    let all = computer(json!({"capabilities":{"files":true,"commands":true,"browser":true,"computerUse":true,"wake":true}}));
    assert_eq!(capability_text(&all.capabilities), "Files, commands, browser, computer use, wake");
    let none = computer(json!({"capabilities":{"files":false,"commands":false,"browser":false,"computerUse":false,"wake":false}}));
    assert_eq!(capability_text(&none.capabilities), "No capabilities");
}

#[test]
fn access_is_per_workspace_and_defaults_to_denied() {
    let c = computer(json!({"access":{"w1":{"allowed":true,"folders":[]}}}));
    assert!(allowed_for(&c, "w1") && !allowed_for(&c, "w2"));
}

#[test]
fn toggling_access_keeps_other_workspaces_and_this_ones_folders() {
    let c = computer(json!({"access":{"w1":{"allowed":true,"folders":[{"path":"/a","mode":"rw"}]},"w2":{"allowed":true,"folders":[]}}}));
    let b = access_body(&c, "w1", false);
    assert_eq!(b["access"]["w1"], json!({"allowed": false, "folders": [{"path":"/a","mode":"rw"}]}));
    assert_eq!(b["access"]["w2"], json!({"allowed": true, "folders": []}));
    let fresh = access_body(&c, "w3", true);
    assert_eq!(fresh["access"]["w3"], json!({"allowed": true, "folders": []}));
    assert!(fresh["access"]["w1"]["folders"].is_array(), "existing entries survive");
}

#[test]
fn wake_is_offered_only_for_sleeping_computers_that_allow_it() {
    assert!(!can_wake(&computer(json!({}))));
    let asleep_no = computer(json!({"status":"asleep"}));
    assert!(!can_wake(&asleep_no));
    let asleep = computer(json!({"status":"asleep","capabilities":{"files":true,"commands":true,"browser":false,"computerUse":false,"wake":true}}));
    assert!(can_wake(&asleep));
}

#[test]
fn paths_encode_the_computer_id() {
    assert_eq!(item_path("pc1"), "/api/computers/pc1");
    assert_eq!(wake_path("a/b"), "/api/computers/a%2Fb/wake");
    assert_eq!(item_path(""), "/api/computers/invalid%20id");
}
