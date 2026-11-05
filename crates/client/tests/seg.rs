use workspaceai_client::api::seg;
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
fn dot_segments_cannot_climb_directories() {
    assert_eq!(seg(".."), "%2E%2E");
    assert_eq!(seg("."), "%2E");
    assert_eq!(seg("..hidden"), "..hidden", "only a bare dot segment is special");
}

#[test]
fn empty_id_stays_empty() {
    assert_eq!(seg(""), "");
}

#[test]
fn request_paths_built_in_the_client_encode_their_ids() {
    assert_eq!(search_path("w/1", "..", "q"), "/api/ws/w%2F1/projects/%2E%2E/files?q=q");
    assert_eq!(EngineAction::Start.path("../x"), "/api/ws/..%2Fx/engine/start");
}
