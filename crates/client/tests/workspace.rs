use serde_json::json;
use workspaceai_client::types::*;
use workspaceai_client::workspace::*;

fn member(name: &str, role: &str) -> Member {
    Member { name: name.into(), email: None, role: role.into() }
}

#[test]
fn nested_settings_are_partial_objects() {
    assert_eq!(nested("web", "mode", json!("exa")), json!({"web": {"mode": "exa"}}));
    assert_eq!(web_test_body("searxng", "Brave", "http://s").get("url"), Some(&json!("http://s")));
}

#[test]
fn check_text_prefers_note_then_error() {
    assert_eq!(check_text(&json!({"ok": true})), "OK");
    assert_eq!(check_text(&json!({"ok": true, "note": "3 results"})), "3 results");
    assert_eq!(check_text(&json!({"ok": false, "error": "no key"})), "no key");
    assert_eq!(check_text(&json!({})), "Failed");
}

#[test]
fn the_owner_keeps_their_role_and_invites_are_deduplicated() {
    let ms = vec![member("Ann", "Admin"), member("Bo", "Member")];
    assert!(role_body(&ms, 0, "Viewer").is_none());
    assert!(role_body(&ms, 1, "Boss").is_none());
    assert_eq!(role_body(&ms, 1, "Viewer").unwrap()["members"][1]["role"], "Viewer");
    assert!(invite_body(&ms, "  ").is_none());
    assert!(invite_body(&ms, "Bo").is_none());
    let b = invite_body(&ms, " c@x.org ").unwrap();
    assert_eq!(b["members"][2], json!({"name": "c@x.org", "email": "c@x.org", "role": "Member"}));
}

#[test]
fn vaults_add_and_remove() {
    let vs = vec![Vault { id: "v1".into(), name: "A".into(), path: "/a".into() }];
    assert!(add_vault_body(&vs, "B", " ").is_none());
    assert_eq!(add_vault_body(&vs, " B ", "/b").unwrap()["vaults"][1], json!({"id": "", "name": "B", "path": "/b"}));
    assert_eq!(remove_vault_body(&vs, "v1"), json!({"vaults": []}));
}

#[test]
fn skills_toggle_and_add_from_a_folder() {
    let sk = vec![Skill { id: "a".into(), name: "A".into(), note: "".into(), on: true }];
    assert_eq!(toggle_skill_body(&sk, "a")["skills"][0]["on"], false);
    let b = add_skill_body(&sk, "~/skills/my-skill/", 36 * 36).unwrap();
    assert_eq!(b["skills"][1], json!({"id": "sk-100", "name": "my-skill", "note": "~/skills/my-skill/", "on": true}));
    assert!(add_skill_body(&sk, " ", 1).is_none());
}

#[test]
fn audit_filter_matches_any_field_in_any_case() {
    let ev = |a: &str, d: &str| AuditEvent { at: 1, actor: "you".into(), action: a.into(), detail: d.into() };
    let all = vec![ev("provider.add", "x"), ev("export", "Policy copy")];
    assert_eq!(filter_audit(&all, "").len(), 2);
    assert_eq!(filter_audit(&all, "POLICY").len(), 1);
    assert_eq!(filter_audit(&all, "nope").len(), 0);
}

#[test]
fn engine_connection_requests() {
    assert!(remote_body("localhost:4096", "u", "").is_none());
    assert_eq!(remote_body("https://h", "u", "").unwrap(), json!({"mode": "remote", "url": "https://h", "username": "u"}));
    assert_eq!(remote_body("https://h", "u", "pw").unwrap()["password"], "pw");
    assert_eq!(version_body("v2.2.1").unwrap()["version"], "2.2.1");
    assert_eq!(version_body("system").unwrap()["version"], "system");
    assert!(version_body("  ").is_none());
    assert_eq!(version_note("system", "2.0", true), "Follows the opencode on PATH · running 2.0 · held by your organization");
    assert_eq!(log_text(&json!({"text": "a\nb\nc\n"}), 2), "b\nc");
    assert_eq!(log_text(&json!({"text": ""}), 2), "(empty)");
}

#[test]
fn open_design_state_parses_and_counts_missing() {
    let st: OdState = serde_json::from_value(json!({
        "install": {"installed": true, "installing": false, "progress": 0},
        "daemon": {"state": "starting"},
        "settings": {"engine": "open-design", "lint": true},
        "artifacts": {"total": 5, "inOpenDesign": 3, "lintFindings": 1}
    }))
    .unwrap();
    assert!(st.busy());
    assert_eq!(st.missing(), 2);
    assert_eq!(daemon_label(&st.daemon.state), "starting…");
    assert_eq!(od_path("w 1", ""), "/api/ws/w%201/opendesign");
}

#[test]
fn design_system_list_is_capped_and_keeps_the_chosen_one() {
    let all: Vec<DesignSystem> = (0..30).map(|i| DesignSystem { id: i.to_string(), title: format!("DS {i}"), summary: "warm".into(), swatches: vec![], source: String::new() }).collect();
    assert_eq!(visible_systems(&all, "", "").len(), 12);
    assert_eq!(visible_systems(&all, "DS 29", "zzz").len(), 1);
    assert_eq!(visible_systems(&all, "", "ds 2").len(), 11);
}

#[test]
fn editing_a_provider_keeps_the_stored_key_unless_one_is_typed() {
    let b = edit_provider_body(" P ", "https://h/v1", "a, b,,", "").unwrap();
    assert_eq!(b, json!({"name": "P", "url": "https://h/v1", "models": ["a", "b"]}));
    assert_eq!(edit_provider_body("P", "https://h", "a", " k ").unwrap()["key"], "k");
    assert!(edit_provider_body("", "https://h", "a", "").is_err());
    assert!(edit_provider_body("P", "ftp://h", "a", "").is_err());
    assert!(edit_provider_body("P", "https://h", " , ", "").is_err());
}

#[test]
fn snapshot_reads_workspace_settings() {
    let s: Snapshot = serde_json::from_value(json!({
        "workspace": {"web": {"mode": "exa", "searxngUrl": "u", "fetch": true}, "members": [{"name": "A", "role": "Admin"}],
            "engine": {"mode": "remote", "version": "2.0"}, "publicLinks": true, "keepChats": "90 days"},
        "audit": [{"at": 5, "actor": "a", "action": "x", "detail": "d"}]
    }))
    .unwrap();
    assert_eq!(s.workspace.web.searxng_url, "u");
    assert!(s.workspace.public_links && s.workspace.engine.mode == "remote");
    assert_eq!(s.audit.len(), 1);
}
