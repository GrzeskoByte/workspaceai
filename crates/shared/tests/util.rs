//! Port of `packages/shared/test/scroll.test.ts` (`followsBottom` is pure) plus checks of the other helpers.
//! `session-reads.test.ts` is N/A: it tests `readsFromTools` over chat tool rows, and chats are terminals now.
use workspaceai_shared::types::Project;
use workspaceai_shared::util::*;

#[test]
fn follows_bottom_lets_go_on_any_move_up_even_a_small_one_near_the_bottom() {
    assert!(!follows_bottom(true, 1000.0, 960.0, 40.0));
}

#[test]
fn follows_bottom_stays_let_go_while_the_reader_moves_down_far_from_the_bottom() {
    assert!(!follows_bottom(false, 100.0, 500.0, 2000.0));
}

#[test]
fn follows_bottom_follows_again_once_the_reader_comes_back_near_the_bottom() {
    assert!(follows_bottom(false, 900.0, 960.0, 40.0));
}

#[test]
fn follows_bottom_keeps_following_when_new_text_grows_the_gap_and_the_view_jumps_down() {
    assert!(follows_bottom(true, 1000.0, 1000.0, 300.0));
}

fn proj(id: &str, parent: Option<&str>, created: i64, general: bool) -> Project {
    Project { id: id.into(), name: id.to_uppercase(), parent: parent.map(String::from), created_at: created, is_general: general.then_some(true), ..Default::default() }
}

#[test]
fn project_tree_helpers() {
    let ps = vec![proj("b", None, 5, false), proj("g", None, 9, true), proj("c", Some("b"), 6, false), proj("d", Some("c"), 7, false)];
    assert_eq!(project_path_name(&ps, Some("d")), "B › C › D");
    assert_eq!(descendants(&ps, "b"), vec!["c", "d"]);
    assert!(is_within(&ps, "d", "b") && !is_within(&ps, "b", "d"));
    let flat: Vec<(&str, usize)> = flat_projects(&ps).iter().map(|(p, d)| (p.id.as_str(), *d)).collect();
    assert_eq!(flat, vec![("g", 0), ("b", 0), ("c", 1), ("d", 2)]);
}

#[test]
fn names_slugs_and_folders() {
    assert_eq!(slugify("BSC ai factory!", "workspace"), "bsc-ai-factory");
    assert_eq!(slugify("***", "workspace"), "workspace");
    assert_eq!(initials_of("  ada   lovelace byron"), "AL");
    assert_eq!(initials_of(""), "?");
    assert_eq!(title_case_segment("/home/me/web/"), "Web");
    let n = nest_folders(&["/a/b", "/a", "/a/b/c", "/x"]);
    assert_eq!(n, vec![("/a".into(), None), ("/x".into(), None), ("/a/b".into(), Some("/a".into())), ("/a/b/c".into(), Some("/a/b".into()))]);
}

#[test]
fn time_ago_and_fuzzy() {
    let now = 10_000_000_000;
    assert_eq!(time_ago(now - 5_000, now), "just now");
    assert_eq!(time_ago(now - 5 * 60_000, now), "5 min ago");
    assert_eq!(time_ago(now - 26 * 3_600_000, now), "yesterday");
    assert_eq!(fuzzy_score("ab", "abc", ""), 3);
    assert_eq!(fuzzy_score("bc", "abc", ""), 2);
    assert_eq!(fuzzy_score("ac", "abc", ""), 1);
    assert_eq!(fuzzy_score("z", "abc", ""), 0);
}

#[test]
fn uid_is_unique_and_prefixed() {
    let (a, b) = (uid("chat"), uid("chat"));
    assert!(a.starts_with("chat_") && a != b);
}
