use chrono::{TimeZone, Utc};
use serde_json::json;
use workspaceai_client::routines::*;
use workspaceai_client::types::{Routine, Snapshot};

const TZ: &str = "Europe/Warsaw";

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 29, 10, 0, 0).unwrap()
}

fn routine(trigger: serde_json::Value, what: serde_json::Value) -> Routine {
    serde_json::from_value(json!({"id":"r1","name":"Daily","phrase":"p","trigger":trigger,"what":what,"project":"p1","paused":false,"runs":[],"createdAt":1})).unwrap()
}

#[test]
fn snapshot_carries_routines_with_runs() {
    let s: Snapshot = serde_json::from_value(json!({"routines":[{
        "id":"r1","name":"Standup","phrase":"every weekday at 9",
        "trigger":{"kind":"schedule","cron":"0 9 * * 1-5","tz":"Europe/Warsaw"},
        "what":{"kind":"prompt","prompt":"Summarize yesterday"},
        "project":"p1","paused":true,"createdAt":5,"lastFired":7,
        "runs":[{"id":"x","at":9,"status":"ok","note":"done"}]}]}))
    .unwrap();
    let r = &s.routines[0];
    assert!(r.paused);
    assert_eq!(r.runs[0].status, "ok");
    assert!(matches!(r.what, workspaceai_client::types::RoutineWhat::Prompt { ref prompt } if prompt == "Summarize yesterday"));
}

#[test]
fn other_triggers_and_actions_still_parse() {
    let r = routine(json!({"kind":"event","event":"git.push","detail":"main"}), json!({"kind":"graph","graph":"g1"}));
    assert!(matches!(r.trigger, workspaceai_client::types::RoutineTrigger::Event { .. }));
    let r2 = routine(json!({"kind":"schedule","cron":"0 9 * * *","tz":"UTC","once":5}), json!({"kind":"artifact","artifact":"a1"}));
    assert!(matches!(r2.what, workspaceai_client::types::RoutineWhat::Artifact { .. }));
}

#[test]
fn schedule_text_shows_the_next_run_in_the_routines_zone() {
    let r = routine(json!({"kind":"schedule","cron":"0 9 * * 1-5","tz":TZ}), json!({"kind":"prompt","prompt":"x"}));
    assert_eq!(trigger_text(&r, now()), "Next: Tomorrow 09:00");
}

#[test]
fn one_off_and_event_and_broken_triggers() {
    let once = Utc.with_ymd_and_hms(2026, 10, 30, 8, 0, 0).unwrap().timestamp_millis();
    let r = routine(json!({"kind":"schedule","cron":"0 9 30 10 *","tz":TZ,"once":once}), json!({"kind":"prompt","prompt":"x"}));
    assert_eq!(trigger_text(&r, now()), "Once: Tomorrow 09:00");
    let e = routine(json!({"kind":"event","event":"git.push","detail":"main"}), json!({"kind":"prompt","prompt":"x"}));
    assert_eq!(trigger_text(&e, now()), "On git.push (main)");
    let bad = routine(json!({"kind":"schedule","cron":"nonsense","tz":TZ}), json!({"kind":"prompt","prompt":"x"}));
    assert_eq!(trigger_text(&bad, now()), "Not scheduled");
}

#[test]
fn last_run_summary() {
    let mut r = routine(json!({"kind":"event","event":"webhook","detail":"default"}), json!({"kind":"prompt","prompt":"x"}));
    assert_eq!(last_run_text(&r), "Never run");
    r.runs = serde_json::from_value(json!([{"id":"1","at":1,"status":"error","note":"boom"},{"id":"2","at":5,"status":"ok","note":"fine"}])).unwrap();
    assert_eq!(last_run_text(&r), "Last run: ok (fine)");
    r.paused = true;
    assert!(last_run_text(&r).starts_with("Paused · "));
}

#[test]
fn create_needs_a_phrase_and_a_prompt() {
    let b = create_body("  every weekday at 9 ", " Summarize ", TZ).unwrap();
    assert_eq!(b["phrase"], "every weekday at 9");
    assert_eq!(b["what"], json!({"kind":"prompt","prompt":"Summarize"}));
    assert_eq!(b["tz"], TZ);
    assert_eq!(b["name"], "every weekday at 9");
    assert!(create_body("", "x", TZ).is_none() && create_body("x", "  ", TZ).is_none());
    assert_eq!(create_body(&"a".repeat(200), "x", TZ).unwrap()["name"].as_str().unwrap().chars().count(), 80);
}

#[test]
fn preview_says_how_the_phrase_was_understood() {
    let p = preview("every weekday at 9", TZ, now());
    assert!(p.ok);
    assert_eq!(p.text, "Every weekdays at 09:00 · Next: Tomorrow 09:00");
    let bad = preview("whenever", TZ, now());
    assert!(!bad.ok && !bad.text.is_empty());
    assert!(!preview("", TZ, now()).ok);
}

#[test]
fn routine_action_paths_and_bodies() {
    assert_eq!(run_path("w/1", "r1"), "/api/ws/w%2F1/routines/r1/run");
    assert_eq!(item_path("w1", "../x"), "/api/ws/w1/routines/..%2Fx");
    assert_eq!(pause_body(true), json!({"paused": true}));
}
