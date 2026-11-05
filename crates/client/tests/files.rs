use serde_json::json;
use workspaceai_client::files::*;

#[test]
fn file_search_reply_is_a_list_of_paths() {
    assert_eq!(parse_files(&json!({"files": ["src/a.ts", "README.md"]})), vec!["src/a.ts", "README.md"]);
}

#[test]
fn odd_replies_give_no_files_not_a_panic() {
    assert!(parse_files(&json!({})).is_empty());
    assert!(parse_files(&json!({"files": "nope"})).is_empty());
    assert_eq!(parse_files(&json!({"files": ["ok", 5, null, "also ok"]})), vec!["ok", "also ok"]);
}

#[test]
fn search_path_encodes_the_query() {
    assert_eq!(search_path("w1", "p1", "src/ma in"), "/api/ws/w1/projects/p1/files?q=src%2Fma%20in");
    assert_eq!(search_path("w1", "p1", ""), "/api/ws/w1/projects/p1/files?q=");
    assert_eq!(search_path("w1", "p1", "żółw&x=1"), "/api/ws/w1/projects/p1/files?q=%C5%BC%C3%B3%C5%82w%26x%3D1");
}

#[test]
fn only_a_new_query_needs_a_search() {
    assert!(needs_search(None, Some("a")));
    assert!(needs_search(Some("a"), Some("ab")));
    assert!(!needs_search(Some("ab"), Some("ab")));
    assert!(!needs_search(Some("ab"), None), "no mention being typed: nothing to search");
}
