//! Port of the opencode-free part of `test/security.test.ts` (R-34, R-36): org policy and safe workspace ids.
//! Audit upload lives in `audit.rs`, the keychain backends in `keychain.rs`.

mod common;

use common::{create, setup};
use serde_json::{json, Value};
use workspaceai_supervisor::paths::valid_id;

fn join_org(f: &common::Fixture, name: &str, email: &str, extra: Value) -> String {
    let mut body = json!({ "name": name, "email": email });
    if let (Some(target), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in extra {
            target.insert(k.clone(), v.clone());
        }
    }
    f.ok("POST", "/api/workspaces/join-org", Some(body))["id"].as_str().unwrap().to_string()
}

#[test]
fn org_workspace_offers_only_policy_providers_and_locks_changes() {
    let f = setup();
    let org = join_org(&f, "BSC AI Factory", "me@org.test", json!({}));

    let snap = f.ok("GET", &format!("/api/ws/{org}/snapshot"), None);
    let ids: Vec<&str> = snap["policy"]["allowedProviders"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["ai-factory", "azure"]);
    let models = snap["policy"]["allowedModels"].as_array().unwrap();
    assert_eq!(models.len(), 3);
    assert!(models.iter().any(|m| m == "ai-factory/llama-4-maverick"));
    let note = f.ok("GET", "/api/state", None)["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == org)
        .map(|w| w["note"].as_str().unwrap().to_string())
        .unwrap();
    assert!(note.contains("AI Factory gateway"), "note: {note}");
    assert!(note.contains("EU data only"), "note: {note}");

    // A model outside the policy is refused; one inside it is accepted.
    let err = f.call("PATCH", &format!("/api/ws/{org}"), Some(json!({ "defaultModel": "anthropic/claude-opus-5" }))).unwrap_err();
    assert_eq!(err.status, 403);
    assert_eq!(err.message, "That model is not allowed by your organization");
    let snap = f.ok("PATCH", &format!("/api/ws/{org}"), Some(json!({ "defaultModel": "ai-factory/qwen3-coder" })));
    assert_eq!(snap["workspace"]["defaultModel"], "ai-factory/qwen3-coder");

    // Locked keys are refused and audited.
    for (key, body) in [("name", json!({ "name": "Renamed" })), ("web", json!({ "web": { "mode": "exa", "searxngUrl": "https://search.[ORG-HOST]" } }))] {
        let err = f.call("PATCH", &format!("/api/ws/{org}"), Some(body)).unwrap_err();
        assert_eq!(err.status, 403, "locked key {key}");
        assert_eq!(err.message, "Locked by org policy");
    }
    let audit = f.ok("GET", &format!("/api/ws/{org}/audit"), None);
    let actions: Vec<&str> = audit["events"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert!(actions.contains(&"policy.block"));
    assert!(actions.contains(&"policy.accept"));
    assert!(actions.contains(&"sso.signin"));

    // A web patch that repeats the policy values is a no-op (R-21 keeps the per-chat toggles usable).
    let web = &f.ok("GET", &format!("/api/ws/{org}/snapshot"), None)["workspace"]["web"];
    let noop = json!({ "web": { "mode": web["mode"].clone(), "searxngUrl": web["searxngUrl"].clone() } });
    f.ok("PATCH", &format!("/api/ws/{org}"), Some(noop));
}

#[test]
fn a_policy_that_fails_verification_blocks_sending_until_refreshed() {
    let f = setup();
    let org = join_org(&f, "BSC AI Factory", "me@org.test", json!({}));

    let tampered = f.ok("POST", &format!("/api/ws/{org}/policy/refresh"), Some(json!({ "tamper": true })));
    assert_eq!(tampered["ok"], false);
    assert!(tampered["reason"].as_str().unwrap().contains("signature"));
    let blocked = f.sup.with_core(|core| core.send_allowed(core.rt(&org).unwrap())).unwrap();
    assert_eq!(blocked.unwrap_err().status, 423);
    assert_eq!(f.ok("GET", &format!("/api/ws/{org}/snapshot"), None)["workspace"]["org"]["policyVerified"], false);

    let fresh = f.ok("POST", &format!("/api/ws/{org}/policy/refresh"), Some(json!({})));
    assert_eq!(fresh["ok"], true);
    assert_eq!(f.ok("GET", &format!("/api/ws/{org}/snapshot"), None)["workspace"]["org"]["policyVerified"], true);
    f.sup.with_core(|core| core.send_allowed(core.rt(&org).unwrap())).unwrap().unwrap();

    // A non-org workspace has no policy to refresh.
    let personal = create(&f, "Personal", "personal");
    assert_eq!(f.ok("POST", &format!("/api/ws/{personal}/policy/refresh"), None)["ok"], true);
}

#[test]
fn valid_id_rejects_path_like_segments() {
    for bad in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
        assert!(!valid_id(bad), "should reject {bad:?}");
    }
    for good in ["ok-id", "ws_abc123", "org_bsc-ai-factory", "general_ws1", "a b"] {
        assert!(valid_id(good), "should accept {good:?}");
    }
}

#[test]
fn workspaces_without_harness_json_are_ignored() {
    let f = setup();
    let personal = create(&f, "Personal", "personal");
    std::fs::create_dir_all(f.path().join("ws").join("stray-folder")).unwrap();
    let reopened = workspaceai_supervisor::Supervisor::new(f.path().to_path_buf()).unwrap();
    let state = reopened.handle("GET", "/api/state", &Default::default(), None).unwrap();
    let ids: Vec<&str> = state["workspaces"].as_array().unwrap().iter().map(|w| w["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [personal.as_str()]);
}
