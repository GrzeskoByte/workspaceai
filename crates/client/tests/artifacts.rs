use serde_json::json;
use workspaceai_client::artifacts::*;
use workspaceai_client::types::Artifact;

fn art() -> Artifact {
    serde_json::from_value(json!({ "id": "a", "type": "Doc", "title": "T", "version": 2, "versions": [
        { "v": 1, "at": 1, "note": "first" },
        { "v": 2, "at": 2, "note": "second", "format": "html", "chat": "c1", "od": { "lint": [{ "severity": "P1", "message": "low contrast" }, {}] } } ] })).unwrap()
}

#[test]
fn versions_list_newest_first_with_formats_and_lint() {
    let r = version_rows(&art());
    assert_eq!(r.iter().map(|v| v.v).collect::<Vec<_>>(), vec![2, 1]);
    assert!(r[0].is_latest && !r[1].is_latest);
    assert_eq!((r[0].format.as_str(), r[1].format.as_str()), ("html", "markdown"));
    assert_eq!((r[0].lint, r[1].lint), (Some(2), None));
    assert_eq!(lint_text(Some(0)), "Saved by Open Design · lint clean");
    assert_eq!(lint_text(Some(1)), "Saved by Open Design · lint 1 finding");
    assert_eq!(lint_lines(&art(), 2)[0], "P1 low contrast");
    assert!(lint_lines(&art(), 1).is_empty());
}

#[test]
fn routes_encode_ids_and_formats() {
    assert_eq!(export_path("w", "a/b", "pdf&x", 0), "/api/ws/w/artifacts/a%2Fb/export?format=pdf%26x");
    assert_eq!(export_path("w", "a", "md", 3), "/api/ws/w/artifacts/a/export?format=md&v=3");
    assert_eq!(restore_body(4), json!({ "v": 4 }));
    assert_eq!(parse_formats(&json!({ "formats": [{ "label": "Markdown", "ext": ".md", "format": "md" }] }))[0].ext, ".md");
}

#[test]
fn share_reports_the_note_and_the_link_to_copy() {
    assert_eq!(share_result(&json!({ "note": "Link copied", "url": "http://x/y" })), ("Link copied · http://x/y".into(), Some("http://x/y".into())));
    assert_eq!(share_result(&json!({ "note": "Public links are off" })), ("Public links are off".into(), None));
}
