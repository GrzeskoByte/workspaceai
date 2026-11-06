//! Port of the secrets parts of `test/security.test.ts` and `vendors.test.ts` (R-34):
//! values go to the keychain, names are normalised, and no route ever echoes a value.

mod common;

use common::{create, setup};
use serde_json::json;

#[test]
fn a_secret_round_trips_through_the_api_without_ever_being_echoed() {
    let f = setup();
    let ws = create(&f, "Secrets", "personal");
    let value = "sk-live-0123456789abcdef";
    let put = f.ok("PUT", &format!("/api/ws/{ws}/secrets/MY_KEY"), Some(json!({ "value": value })));
    assert_eq!(put, json!({ "ok": true }));

    let listed = f.ok("GET", &format!("/api/ws/{ws}/secrets"), None);
    assert_eq!(listed["names"], json!(["MY_KEY"]));
    assert_eq!(listed["backend"], "memory");
    assert!(!listed.to_string().contains(value), "GET /secrets leaked the value");

    for path in [format!("/api/ws/{ws}/snapshot"), format!("/api/ws/{ws}/opencode-json"), format!("/api/ws/{ws}/audit")] {
        let body = f.ok("GET", &path, None);
        assert!(!body.to_string().contains(value), "{path} leaked the value");
    }
    assert_eq!(f.sup.with_core(|core| core.keychain.get(&ws, "MY_KEY")).unwrap().as_deref(), Some(value));

    f.ok("DELETE", &format!("/api/ws/{ws}/secrets/MY_KEY"), None);
    assert_eq!(f.ok("GET", &format!("/api/ws/{ws}/secrets"), None)["names"], json!([]));
    assert_eq!(f.sup.with_core(|core| core.keychain.get(&ws, "MY_KEY")).unwrap(), None);
}

#[test]
fn names_are_normalised_and_puts_are_audited() {
    let f = setup();
    let ws = create(&f, "Names", "personal");
    f.ok("PUT", &format!("/api/ws/{ws}/secrets/my-key"), Some(json!({ "value": "abc" })));
    let listed = f.ok("GET", &format!("/api/ws/{ws}/secrets"), None);
    assert_eq!(listed["names"], json!(["MY_KEY"]));
    let events = f.ok("GET", &format!("/api/ws/{ws}/audit"), None);
    let actions: Vec<&str> = events["events"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert!(actions.contains(&"secret.set"));
    // An empty value still becomes a (blank) secret, as the TS route does.
    f.ok("PUT", &format!("/api/ws/{ws}/secrets/EMPTY"), Some(json!({})));
    assert_eq!(f.sup.with_core(|core| core.keychain.get(&ws, "EMPTY")).unwrap().as_deref(), Some(""));
}

#[test]
fn an_org_that_blocks_personal_keys_refuses_them_unless_marked_org() {
    let f = setup();
    let org = f.ok("POST", "/api/workspaces/join-org", Some(json!({ "name": "BSC AI Factory", "email": "me@org.test" })))["id"].as_str().unwrap().to_string();
    let err = f.call("PUT", &format!("/api/ws/{org}/secrets/OPENAI_API_KEY"), Some(json!({ "value": "sk-x" }))).unwrap_err();
    assert_eq!(err.status, 403);
    assert_eq!(err.message, "Personal keys can’t be used in this organization");
    assert_eq!(f.ok("GET", &format!("/api/ws/{org}/secrets"), None)["names"], json!([]));

    f.ok("PUT", &format!("/api/ws/{org}/secrets/AI_FACTORY_TOKEN"), Some(json!({ "value": "org-token", "org": true })));
    assert_eq!(f.ok("GET", &format!("/api/ws/{org}/secrets"), None)["names"], json!(["AI_FACTORY_TOKEN"]));
}
