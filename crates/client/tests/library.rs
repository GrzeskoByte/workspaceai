use chrono::{TimeZone, Utc};
use serde_json::json;
use workspaceai_client::library::*;
use workspaceai_client::types::{Artifact, Project, Snapshot};

fn art(id: &str, title: &str, kind: &str, project: &str, updated: i64, versions: usize) -> Artifact {
    serde_json::from_value(json!({"id":id,"project":project,"chat":null,"type":kind,"title":title,"version":versions,
        "versions":(1..=versions).map(|v| json!({"v":v,"at":0,"note":"","format":"markdown"})).collect::<Vec<_>>(),"createdAt":1,"updatedAt":updated})).unwrap()
}
fn project(id: &str, name: &str) -> Project {
    Project { id: id.into(), name: name.into(), parent: None, archived: false, is_general: None }
}

#[test]
fn snapshot_carries_artifacts() {
    let s: Snapshot = serde_json::from_value(json!({"artifacts":[{"id":"a1","project":"p","type":"Deck","title":"Pitch","version":2,"versions":[{"v":1,"at":0,"note":"first","format":"html"},{"v":2,"at":5}],"updatedAt":9}]})).unwrap();
    let a = &s.artifacts[0];
    assert_eq!((a.kind.as_str(), a.title.as_str(), a.version, a.versions.len(), a.updated_at), ("Deck", "Pitch", 2, 2, 9));
    assert_eq!(a.versions[0].format.as_deref(), Some("html"));
}

#[test]
fn rows_are_newest_first_with_project_names() {
    let arts = vec![art("a", "Old", "Doc", "p1", 1, 1), art("b", "New", "Deck", "p2", 9, 3), art("c", "Mid", "Sheet", "gone", 5, 1)];
    let rows = rows(&arts, &[project("p1", "Alpha"), project("p2", "Beta")], "");
    assert_eq!(rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), vec!["New", "Mid", "Old"]);
    assert_eq!((rows[0].project.as_str(), rows[0].versions), ("Beta", 3));
    assert_eq!(rows[1].project, "—", "an unknown project shows a dash, not its raw id");
}

#[test]
fn search_matches_title_type_and_project_case_insensitively() {
    let arts = vec![art("a", "Quarterly report", "Doc", "p1", 1, 1), art("b", "Launch deck", "Deck", "p2", 2, 1)];
    let ps = [project("p1", "Alpha"), project("p2", "Beta")];
    let ids = |q: &str| rows(&arts, &ps, q).into_iter().map(|r| r.id).collect::<Vec<_>>();
    assert_eq!(ids("REPORT"), vec!["a"]);
    assert_eq!(ids("deck"), vec!["b"]);
    assert_eq!(ids("beta"), vec!["b"]);
    assert_eq!(ids("  "), vec!["b", "a"]);
    assert!(ids("zzz").is_empty());
}

#[test]
fn view_reply_parses_content_and_format() {
    let v = parse_view(&json!({"artifact":{},"content":"# Hi","format":"markdown","html":"<h1>"})).unwrap();
    assert_eq!((v.content.as_str(), v.format.as_str()), ("# Hi", "markdown"));
    assert!(parse_view(&json!({})).is_none());
    assert_eq!(parse_view(&json!({"content":"x"})).unwrap().format, "markdown", "a missing format reads as Markdown");
}

#[test]
fn body_decides_how_each_format_is_shown() {
    assert_eq!(body_for("markdown", "# Hi"), Body::Markdown("# Hi".into()));
    assert_eq!(body_for("csv", "a,b\n1,2"), Body::Text("a,b\n1,2".into()));
    assert_eq!(body_for("json", "{}"), Body::Text("{}".into()));
    assert_eq!(body_for("svg", "<svg/>"), Body::Text("<svg/>".into()));
    assert!(matches!(body_for("html", "<p>x</p>"), Body::Unsupported(m) if m.contains("web view")));
    assert!(matches!(body_for("png", ""), Body::Unsupported(_)));
}

#[test]
fn very_long_text_is_cut_for_display() {
    let big = "x".repeat(MAX_SHOWN + 1000);
    let Body::Text(t) = body_for("csv", &big) else { panic!() };
    assert!(t.chars().count() <= MAX_SHOWN + 40, "{}", t.chars().count());
    assert!(t.ends_with("(shortened)"));
}

#[test]
fn updated_text_and_paths() {
    let now = Utc.with_ymd_and_hms(2026, 10, 29, 10, 0, 0).unwrap();
    let ms = Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap().timestamp_millis();
    assert_eq!(updated_text(ms, "UTC", now), "Today 08:30");
    assert_eq!(content_path("w1", "a1", None), "/api/ws/w1/artifacts/a1/content");
    assert_eq!(content_path("w1", "a1", Some(3)), "/api/ws/w1/artifacts/a1/content?v=3");
    assert_eq!(content_path("w1", "..", None), "/api/ws/w1/artifacts/invalid%20id/content");
}

#[test]
fn markdown_artifacts_are_capped_too() {
    let big = "line\n\n".repeat(workspaceai_client::markdown::MAX_INPUT);
    let Body::Markdown(m) = body_for("markdown", &big) else { panic!() };
    assert!(m.chars().count() <= workspaceai_client::markdown::MAX_INPUT + 40);
    assert!(m.ends_with("(shortened)"));
}
