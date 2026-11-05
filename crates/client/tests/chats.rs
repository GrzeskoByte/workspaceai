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
    assert_eq!(full_access_body(true), json!({"yolo": true}));
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
fn only_one_change_per_chat_is_in_flight() {
    let mut f = InFlight::default();
    assert!(f.begin("c1"), "first change starts");
    assert!(!f.begin("c1"), "a second click while waiting is ignored");
    assert!(f.is_pending("c1") && !f.is_pending("c2"));
    assert!(f.begin("c2"), "another chat is independent");
    f.finish("c1");
    assert!(f.begin("c1"), "after the server answers it can change again");
    f.finish_all();
    assert!(f.begin("c2"));
}
