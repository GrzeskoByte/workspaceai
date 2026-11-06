//! Port of the opencode-free part of `test/core.test.ts`: workspaces, state, isolation, workspace patch, facts.

mod common;

use common::{create, setup, Fixture};
use serde_json::json;

#[test]
fn creates_workspaces_and_organizations_only_join_through_sso() {
    let f = setup();
    let personal = create(&f, "Personal", "personal");
    let team = create(&f, "Castform Labs", "team");
    assert!(personal.starts_with("ws_"), "personal id: {personal}");

    let err = f.call("POST", "/api/workspaces", Some(json!({ "name": "Evil Org", "kind": "org" }))).unwrap_err();
    assert_eq!(err.status, 400);
    assert!(err.message.contains("SSO"), "message: {}", err.message);

    let org = f.ok("POST", "/api/workspaces/join-org", Some(json!({ "name": "BSC AI Factory", "email": "me@org.test" })));
    let org = org["id"].as_str().unwrap().to_string();
    assert!(org.starts_with("org_bsc-ai-factory"), "org id: {org}");
    assert_ne!(org, personal);
    assert_ne!(team, personal);

    let state = f.ok("GET", "/api/state", None);
    let kinds: Vec<&str> = state["workspaces"].as_array().unwrap().iter().map(|w| w["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["personal", "org", "team"]);

    let first = &state["workspaces"][0];
    assert_eq!(first["id"], personal);
    assert_eq!(first["initials"], "P");
    assert_eq!(first["accent"], "default");
    let note = first["note"].as_str().unwrap();
    assert!(note.contains("connectors"), "note: {note}");
    assert!(note.contains("Graphify + facts"), "note: {note}");
    assert_eq!(state["settings"]["theme"], "claude");
}

#[test]
fn keeps_everything_per_workspace() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let b = create(&f, "Other", "team");

    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Only A knows this", "scope": "ws" })));
    f.ok("POST", &format!("/api/ws/{b}/facts"), Some(json!({ "text": "Only B knows this", "scope": "ws" })));

    let snap_a = f.ok("GET", &format!("/api/ws/{a}/snapshot"), None);
    let snap_b = f.ok("GET", &format!("/api/ws/{b}/snapshot"), None);
    let facts = |snap: &serde_json::Value| -> Vec<String> {
        snap["facts"].as_array().unwrap().iter().map(|x| x["text"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(facts(&snap_a), ["Only A knows this"]);
    assert_eq!(facts(&snap_b), ["Only B knows this"]);
    assert_eq!(snap_a["workspace"]["id"], a);
    assert_eq!(snap_b["workspace"]["id"], b);

    f.sup
        .with_core(|core| {
            let ra = core.rt(&a).unwrap();
            let rb = core.rt(&b).unwrap();
            assert_ne!(ra.paths.root, rb.paths.root);
            assert!(ra.paths.xdg_data.starts_with(&ra.paths.root));
            assert!(ra.paths.xdg_config.starts_with(&ra.paths.root));
            assert_ne!(ra.facts_file("ws").unwrap(), rb.facts_file("ws").unwrap());
            assert!(ra.facts_file("ws").unwrap().starts_with(&ra.paths.root));
            assert_ne!(ra.paths.opencode_json, rb.paths.opencode_json);
            assert_ne!(ra.ws.default_project, rb.ws.default_project);
            // Each workspace has its own memory manifest and config file.
            ra.write_config().unwrap();
            assert!(ra.paths.opencode_json.exists());
            assert!(!rb.paths.opencode_json.exists());
            // Secrets are keyed per workspace and never land in another workspace's slot.
            core.keychain.set(&ra.id, "MY_KEY", "sk-test-aaaaaaaaaaaaaaaa").unwrap();
            assert_eq!(core.keychain.get(&ra.id, "MY_KEY").as_deref(), Some("sk-test-aaaaaaaaaaaaaaaa"));
            assert_eq!(core.keychain.get(&rb.id, "MY_KEY"), None);
            assert_eq!(core.keychain.list(&ra.id), vec!["MY_KEY".to_string()]);
        })
        .unwrap();
}

#[test]
fn the_last_personal_workspace_cannot_be_deleted() {
    let f = setup();
    let personal = create(&f, "Personal", "personal");
    let team = create(&f, "Castform Labs", "team");

    let err = f.call("DELETE", &format!("/api/workspaces/{personal}"), None).unwrap_err();
    assert_eq!(err.status, 400);
    assert!(err.message.contains("personal workspace"), "message: {}", err.message);
    assert!(f.path().join("ws").join(&personal).exists());

    // A team workspace can go; its folder and runtime disappear.
    let state = f.ok("DELETE", &format!("/api/workspaces/{team}"), None);
    let ids: Vec<&str> = state["workspaces"].as_array().unwrap().iter().map(|w| w["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [personal.as_str()]);
    assert!(!f.path().join("ws").join(&team).exists());
}

#[test]
fn settings_patch_and_activate() {
    let f = setup();
    let personal = create(&f, "Personal", "personal");
    let team = create(&f, "Castform Labs", "team");

    let state = f.ok("PATCH", "/api/settings", Some(json!({ "theme": "dark", "accent": "teal", "reduceMotion": true, "followSystem": true })));
    assert_eq!(state["settings"]["theme"], "dark");
    assert_eq!(state["settings"]["accent"], "teal");
    assert_eq!(state["settings"]["reduceMotion"], true);
    assert_eq!(state["settings"]["followSystem"], true);

    let state = f.ok("POST", &format!("/api/workspaces/{team}/activate"), None);
    assert_eq!(state["settings"]["activeWorkspace"], team);
    let state = f.ok("POST", &format!("/api/workspaces/{personal}/activate"), None);
    assert_eq!(state["settings"]["activeWorkspace"], personal);

    let err = f.call("POST", "/api/workspaces/missing/activate", None).unwrap_err();
    assert_eq!(err.status, 404);
    assert_eq!(err.message, "Workspace not found");
}

#[test]
fn unknown_routes_and_methods_are_404() {
    let f: Fixture = setup();
    let err = f.call("GET", "/api/nope", None).unwrap_err();
    assert_eq!(err.status, 404);
    // A known path with the wrong method is not routed either.
    let err = f.call("POST", "/api/state", None).unwrap_err();
    assert_eq!(err.status, 404);
}

#[test]
fn supervisor_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<workspaceai_supervisor::Supervisor>();
}
