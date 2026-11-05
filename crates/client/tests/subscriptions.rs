use serde_json::json;
use workspaceai_client::subscriptions::*;

fn sub(v: serde_json::Value) -> workspaceai_client::types::Subscription {
    let mut base = json!({"id":"claude","name":"Claude Code","installed":true,"signedIn":true,"version":"2.1.0","account":"me@x.example","dataDir":"/d","command":"claude","connected":false,"agentOn":false,"models":[],"detail":"Ready"});
    for (k, x) in v.as_object().unwrap() {
        base[k] = x.clone();
    }
    serde_json::from_value(base).unwrap()
}

#[test]
fn the_list_reply_is_an_array_or_an_object_with_the_array() {
    let one = json!({"id":"grok","name":"Grok"});
    assert_eq!(parse_list(&json!([one.clone()])).len(), 1);
    assert_eq!(parse_list(&json!({"subscriptions":[one]})).len(), 1);
    assert!(parse_list(&json!({})).is_empty());
    assert!(parse_list(&json!("nope")).is_empty());
    assert_eq!(parse_list(&json!([{"id":"a","name":"A"}, {"broken": true}])).len(), 1, "a bad row is skipped, not fatal");
}

#[test]
fn summary_says_installed_signed_in_and_connected() {
    assert_eq!(summary(&sub(json!({}))), "Installed 2.1.0 · signed in as me@x.example · Not connected to this workspace");
    assert_eq!(summary(&sub(json!({"installed":false,"signedIn":false,"version":null,"account":null}))), "Not installed · Not connected to this workspace");
    assert_eq!(summary(&sub(json!({"connected":true}))), "Installed 2.1.0 · signed in as me@x.example · Connected to this workspace");
    assert_eq!(summary(&sub(json!({"signedIn":false,"account":null}))), "Installed 2.1.0 · not signed in · Not connected to this workspace");
}

#[test]
fn actions_follow_installed_and_connected() {
    use SubAction::*;
    assert_eq!(actions(&sub(json!({}))), vec![Connect]);
    assert_eq!(actions(&sub(json!({"connected":true}))), vec![Disconnect]);
    assert!(actions(&sub(json!({"installed":false}))).is_empty());
    // A connected one whose tool was uninstalled can still be disconnected.
    assert_eq!(actions(&sub(json!({"installed":false,"connected":true}))), vec![Disconnect]);
}

#[test]
fn an_expired_login_is_called_out() {
    assert!(needs_sign_in(&sub(json!({"connected":true,"signedIn":false}))));
    assert!(!needs_sign_in(&sub(json!({"connected":true}))));
    assert!(!needs_sign_in(&sub(json!({"connected":false,"signedIn":false}))));
}

#[test]
fn paths_encode_ids() {
    assert_eq!(list_path("w1"), "/api/ws/w1/subscriptions");
    assert_eq!(action_path("w1", "claude", SubAction::Connect), "/api/ws/w1/subscriptions/claude/connect");
    assert_eq!(action_path("w/1", "a b", SubAction::Disconnect), "/api/ws/w%2F1/subscriptions/a%20b/disconnect");
    assert_eq!(action_path("w1", "..", SubAction::Connect), "/api/ws/w1/subscriptions/invalid%20id/connect");
}
