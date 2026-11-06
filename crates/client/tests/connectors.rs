use serde_json::json;
use workspaceai_client::connectors::*;
use workspaceai_client::types::{McpServer, Snapshot};

fn mcp(v: serde_json::Value) -> McpServer {
    let mut base = json!({"id":"fs-1","name":"Files","kind":"MCP · stdio","type":"local","command":["npx","-y","server-fs"],"on":true});
    for (k, x) in v.as_object().unwrap() {
        base[k] = x.clone();
    }
    serde_json::from_value(base).unwrap()
}

#[test]
fn workspace_carries_its_mcp_servers() {
    let s: Snapshot = serde_json::from_value(json!({"workspace":{"mcp":[{"id":"a","name":"A","type":"remote","url":"https://m.example/mcp","on":false,"locked":true,"builtin":"memory"}]}})).unwrap();
    let m = &s.workspace.mcp[0];
    assert_eq!((m.kind_of.as_str(), m.url.as_deref(), m.on, m.locked, m.builtin.as_deref()), ("remote", Some("https://m.example/mcp"), false, Some(true), Some("memory")));
}

#[test]
fn describe_shows_the_command_or_url() {
    assert_eq!(describe(&mcp(json!({}))), "npx -y server-fs");
    assert_eq!(describe(&mcp(json!({"type":"remote","url":"https://m.example/mcp","command":null}))), "https://m.example/mcp");
    assert_eq!(describe(&mcp(json!({"type":"connector","command":null}))), "Connector");
    let long = mcp(json!({"command":["x".repeat(200)]}));
    assert_eq!(describe(&long).chars().count(), 80);
    assert!(describe(&mcp(json!({"builtin":"memory"}))).starts_with("Built in · "));
}

#[test]
fn only_user_added_unlocked_connectors_can_be_removed() {
    assert!(can_remove(&mcp(json!({}))));
    assert!(!can_remove(&mcp(json!({"locked":true}))));
    assert!(!can_remove(&mcp(json!({"builtin":"playwright"}))));
}

#[test]
fn toggling_is_refused_for_locked_ones() {
    assert!(can_toggle(&mcp(json!({}))));
    assert!(!can_toggle(&mcp(json!({"locked":true}))));
    assert!(can_toggle(&mcp(json!({"builtin":"search"}))), "built-in ones can be switched, just not removed");
}

#[test]
fn body_and_paths() {
    assert_eq!(toggle_body(false), json!({"on": false}));
    assert_eq!(item_path("w1", "fs-1"), "/api/ws/w1/mcp/fs-1");
    assert_eq!(item_path("w1", ""), "/api/ws/w1/mcp/invalid%20id");
}

#[test]
fn add_body_needs_a_target_and_names_it_by_default() {
    assert_eq!(add_body("", "  "), None);
    assert_eq!(add_body(" Files ", " npx -y server-fs "), Some(json!({"name":"Files","command":"npx -y server-fs"})));
    assert_eq!(add_body("", "https://m.example/mcp"), Some(json!({"name":"https://m.example/mcp","command":"https://m.example/mcp"})));
    assert_eq!(add_path("a/b"), "/api/ws/a%2Fb/mcp");
}
