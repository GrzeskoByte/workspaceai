use serde_json::json;
use workspaceai_client::chats::*;

#[test]
fn rename_trims_and_refuses_blank_titles() {
    assert_eq!(rename_body("  Plan A  "), Some(json!({"title": "Plan A"})));
    assert_eq!(rename_body("   "), None);
    assert_eq!(rename_body(""), None);
}

#[test]
fn rename_caps_long_titles_on_a_char_boundary() {
    let long = "ż".repeat(300);
    let body = rename_body(&long).unwrap();
    let t = body["title"].as_str().unwrap();
    assert_eq!(t.chars().count(), 120);
    assert!(t.chars().all(|c| c == 'ż'));
}

#[test]
fn archive_and_full_access_bodies() {
    assert_eq!(archive_body(true), json!({"archived": true}));
    assert_eq!(archive_body(false), json!({"archived": false}));
    assert_eq!(full_access_body(true), json!({"fullAccess": true}));
}

#[test]
fn delete_needs_a_second_click_on_the_same_chat() {
    let mut c = Confirm::default();
    assert!(!c.press("c1"), "first press only arms it");
    assert!(c.press("c1"), "second press on the same chat confirms");
    assert!(!c.press("c1"), "confirming resets it: the next press arms again");
    assert!(!c.press("c2"), "a different chat re-arms instead of confirming");
    assert!(!c.press("c1"));
    c.cancel();
    assert!(!c.press("c1"), "cancel disarms");
}

#[test]
fn move_request_targets_the_chat_and_names_the_project() {
    assert_eq!(move_path("w1", "c1"), "/api/ws/w1/chats/c1/move");
    assert_eq!(move_body("p2"), json!({"project": "p2"}));
}

#[test]
fn launch_reply_gives_argv_env_and_cwd() {
    assert_eq!(launch_path("w 1", "c1"), "/api/ws/w%201/chats/c1/launch");
    let l = parse_launch(&json!({"argv": ["claude", "--resume", "x"], "env": {"A": "1", "N": 5}, "cwd": "/p", "files": []})).unwrap();
    assert_eq!(l.argv, vec!["claude", "--resume", "x"]);
    assert_eq!(l.env, vec![("A".to_string(), "1".to_string())]);
    assert_eq!(l.cwd.as_deref(), Some("/p"));
    assert!(parse_launch(&json!({"argv": []})).is_none());
    assert!(parse_launch(&json!({})).is_none());
}
