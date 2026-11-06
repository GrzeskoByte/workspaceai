use serde_json::json;
use std::path::Path;
use workspaceai_client::spaces::*;
use workspaceai_client::types::WorkspaceSummary;

fn w(id: &str, kind: &str) -> WorkspaceSummary {
    WorkspaceSummary { id: id.into(), name: id.into(), kind: kind.into(), initials: String::new(), accent: String::new(), note: String::new() }
}

#[test]
fn dialog_requests_by_kind() {
    assert!(create_request("team", "  ", "", "").is_none());
    assert_eq!(create_request("team", " Lab ", "", "").unwrap(), ("/api/workspaces", json!({"name": "Lab", "kind": "team"})));
    assert_eq!(create_request("personal", "Me", "", "").unwrap().1["kind"], "personal");
    let (path, body) = create_request("org", "Org", " me@x.org ", "https://sso").unwrap();
    assert_eq!(path, "/api/workspaces/join-org");
    assert_eq!(body, json!({"name": "Org", "email": "me@x.org", "issuer": "https://sso"}));
    assert_eq!(submit_label("org", false), "Continue with SSO");
    assert_eq!(submit_label("team", true), "Working…");
    assert_eq!(created_toast("org", "O"), "Joined O · policy verified");
}

#[test]
fn the_only_personal_workspace_stays() {
    let one = vec![w("p", "personal"), w("t", "team")];
    assert!(!can_delete("personal", &one));
    assert!(can_delete("team", &one));
    assert!(can_delete("personal", &[w("p", "personal"), w("q", "personal")]));
    assert_eq!(next_workspace(&one, "p").as_deref(), Some("t"));
    assert_eq!(next_workspace(&[w("p", "personal")], "p"), None);
    assert_eq!(remove_label("org"), "Leave organization");
    assert_eq!(remove_path("a/b"), "/api/workspaces/a%2Fb");
    assert_eq!(remove_button("org", true), "Click again to leave");
}

#[test]
fn download_names_are_safe_and_never_overwrite() {
    assert_eq!(filename_from_disposition("attachment; filename=\"ws.tar.gz\""), Some("ws.tar.gz".into()));
    assert_eq!(filename_from_disposition("attachment"), None);
    assert_eq!(safe_filename("../../etc/passwd"), "passwd");
    assert_eq!(safe_filename("..\\x.tgz"), "x.tgz");
    assert_eq!(safe_filename(".."), "export");
    assert_eq!(safe_filename(".bashrc"), "bashrc");
    let taken = ["/d/a.tar.gz", "/d/a (2).tar.gz"];
    let exists = |p: &Path| taken.iter().any(|t| Path::new(t) == p);
    assert_eq!(unique_path(Path::new("/d"), "a.tar.gz", exists), Path::new("/d/a (3).tar.gz"));
    assert_eq!(unique_path(Path::new("/d"), "b.tar.gz", exists), Path::new("/d/b.tar.gz"));
    assert_eq!(export_path("w 1"), "/api/ws/w%201/export");
}

#[test]
fn model_choices_keep_the_current_one_first() {
    let listed = vec!["a/x".to_string(), "b/y".to_string(), "a/x".to_string()];
    assert_eq!(model_choices("b/y", &listed), vec!["b/y", "a/x"]);
    assert_eq!(model_choices("gone/z", &listed)[0], "gone/z");
    assert_eq!(model_choices("", &[]), Vec::<String>::new());
}
