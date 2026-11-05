use serde_json::json;
use workspaceai_client::providers::*;
use workspaceai_client::types::Snapshot;

fn provider(v: serde_json::Value) -> workspaceai_client::types::Provider {
    let mut base = json!({"id":"anthropic","name":"Anthropic","note":"API key","keyEnv":"ANTHROPIC_API_KEY","models":[{"id":"opus","name":"Opus"}]});
    for (k, x) in v.as_object().unwrap() {
        base[k] = x.clone();
    }
    serde_json::from_value(base).unwrap()
}

#[test]
fn workspace_carries_its_providers() {
    let s: Snapshot = serde_json::from_value(json!({"workspace":{"id":"w","providers":[{"id":"local","name":"Local","models":[{"id":"a"},{"id":"b"}],"isDefault":true}]}})).unwrap();
    let p = &s.workspace.providers[0];
    assert_eq!((p.id.as_str(), p.models.len(), p.is_default, p.key_env.as_deref()), ("local", 2, Some(true), None));
    assert!(serde_json::from_value::<Snapshot>(json!({})).unwrap().workspace.providers.is_empty());
}

#[test]
fn key_state_comes_from_the_stored_secret_names() {
    let names = vec!["ANTHROPIC_API_KEY".to_string()];
    assert_eq!(key_state(&provider(json!({})), &names), KeyState::Stored);
    assert_eq!(key_state(&provider(json!({"keyEnv":"OTHER_KEY"})), &names), KeyState::Missing);
    assert_eq!(key_state(&provider(json!({"keyEnv":null})), &names), KeyState::NotNeeded);
    assert_eq!(key_state(&provider(json!({"keyEnv":""})), &names), KeyState::NotNeeded);
}

#[test]
fn key_state_text() {
    assert_eq!(KeyState::Stored.text(), "Key stored in the keychain");
    assert_eq!(KeyState::Missing.text(), "No key yet");
    assert_eq!(KeyState::NotNeeded.text(), "No key needed");
}

#[test]
fn secret_body_trims_and_refuses_blank_without_exposing_it_in_debug_output() {
    assert_eq!(secret_body("  sk-abc \n").unwrap().to_string(), r#"{"value":"sk-abc"}"#);
    assert!(secret_body("   ").is_none());
    assert!(secret_body(&"x".repeat(5000)).is_none(), "an absurdly long key is refused, not truncated");
}

#[test]
fn secret_names_are_upper_snake_like_the_server_makes_them() {
    assert_eq!(secret_name("anthropic_api_key"), Some("ANTHROPIC_API_KEY".to_string()));
    assert_eq!(secret_name("my-key.v2"), Some("MY_KEY_V2".to_string()));
    assert_eq!(secret_name(""), None);
    assert_eq!(secret_name("../x"), Some("___X".to_string()));
}

#[test]
fn paths_encode_ids_and_names() {
    assert_eq!(secret_path("w1", "ANTHROPIC_API_KEY"), "/api/ws/w1/secrets/ANTHROPIC_API_KEY");
    assert_eq!(secret_path("w1", ".."), "/api/ws/w1/secrets/invalid%20id");
    assert_eq!(provider_path("w/1", "p1"), "/api/ws/w%2F1/providers/p1");
    assert_eq!(secrets_list_path("w1"), "/api/ws/w1/secrets");
}
