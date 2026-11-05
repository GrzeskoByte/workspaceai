use workspaceai_client::api::{seg, INVALID_ID};
use workspaceai_client::routines::item_path;
use workspaceai_client::files::search_path;
use workspaceai_client::settings::EngineAction;

#[test]
fn plain_ids_pass_through() {
    for id in ["ws_mw0lmzjh1omoe", "ses_1-x", "f_ab12", "A.b~c"] {
        assert_eq!(seg(id), id);
    }
}

#[test]
fn separators_and_specials_are_encoded() {
    assert_eq!(seg("a/b"), "a%2Fb");
    assert_eq!(seg("a?b#c%d"), "a%3Fb%23c%25d");
    assert_eq!(seg("a b"), "a%20b");
    assert_eq!(seg("zażółć"), "za%C5%BC%C3%B3%C5%82%C4%87");
}

#[test]
fn dot_segments_and_empty_ids_become_an_id_no_route_has() {
    // %2E%2E is read as ".." by many HTTP stacks, and an empty segment would turn /facts/{id} into the bulk /facts.
    for bad in ["..", ".", ""] {
        assert_eq!(seg(bad), INVALID_ID, "{bad:?}");
    }
    assert_eq!(seg("..hidden"), "..hidden", "only a bare dot segment is special");
}

#[test]
fn invalid_id_is_a_plain_segment_that_cannot_exist() {
    assert_eq!(INVALID_ID, "invalid%20id");
    assert!(!INVALID_ID.contains('/') && INVALID_ID != "." && INVALID_ID != "..");
}

#[test]
fn built_paths_never_contain_a_dot_or_empty_segment() {
    for id in ["", ".", "..", "a/../b", "%2e%2e", "x/", "/", "\u{0}"] {
        let path = item_path("ws1", id);
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        assert!(segments.iter().all(|s| !s.is_empty() && *s != "." && *s != ".."), "{id:?} -> {path}");
        assert_eq!(segments.len(), 5, "{id:?} -> {path}: the id must stay one segment");
    }
}

#[test]
fn request_paths_built_in_the_client_encode_their_ids() {
    assert_eq!(search_path("w/1", "..", "q"), "/api/ws/w%2F1/projects/invalid%20id/files?q=q");
    assert_eq!(EngineAction::Start.path("../x"), "/api/ws/..%2Fx/engine/start");
}
