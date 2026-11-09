use workspaceai_client::palette::*;

fn item(kind: Kind, id: &str, label: &str) -> Item {
    Item { kind, id: id.into(), label: label.into(), hint: String::new() }
}

fn items() -> Vec<Item> {
    vec![
        item(Kind::Action, "new-chat", "New chat"),
        item(Kind::Workspace, "w1", "Personal"),
        item(Kind::Chat, "c1", "Fix the login bug"),
        item(Kind::Chat, "c2", "Plan the launch"),
        item(Kind::Page, "memory", "Settings: Memory"),
    ]
}

fn labels(q: &str) -> Vec<String> {
    filter(&items(), q).into_iter().map(|i| i.label.clone()).collect()
}

#[test]
fn empty_query_lists_everything_in_order_up_to_the_cap() {
    assert_eq!(labels("").len(), 5);
    let many: Vec<_> = (0..40).map(|i| item(Kind::Chat, &i.to_string(), &format!("chat {i}"))).collect();
    assert_eq!(filter(&many, "").len(), MAX_RESULTS);
}

#[test]
fn matching_is_case_insensitive_and_prefers_prefixes() {
    assert_eq!(labels("PLAN"), vec!["Plan the launch"]);
    // "p" starts Personal and Plan…; both are prefix matches and keep their order.
    assert_eq!(labels("p")[..2], ["Personal".to_string(), "Plan the launch".to_string()]);
}

#[test]
fn word_start_beats_a_match_in_the_middle() {
    let list = vec![item(Kind::Chat, "a", "Rebuild everything"), item(Kind::Chat, "b", "Build the app")];
    assert_eq!(filter(&list, "build")[0].id, "b");
}

#[test]
fn letters_in_order_still_match_when_not_contiguous() {
    assert_eq!(labels("flb"), vec!["Fix the login bug"]);
    assert!(labels("zzz").is_empty());
}

#[test]
fn unicode_queries_do_not_panic() {
    let list = vec![item(Kind::Chat, "a", "Żółw na drodze")];
    assert_eq!(filter(&list, "żół").len(), 1);
    assert_eq!(filter(&list, "ŻÓŁ").len(), 1);
}

#[test]
fn archived_chats_are_not_offered_and_current_chat_is_first_among_chats() {
    use workspaceai_client::types::{Chat, Snapshot, WorkspaceSummary};
    let chat = |id: &str, title: &str, updated: i64, archived: bool| Chat { id: id.into(), project: "p".into(), title: title.into(), archived, updated_at: updated, agent: String::new(), full_access: false };
    let snap = Snapshot { chats: vec![chat("c1", "old", 1, false), chat("c2", "new", 9, false), chat("c3", "gone", 99, true)], ..Default::default() };
    let ws = vec![WorkspaceSummary { id: "w1".into(), name: "Personal".into(), kind: "personal".into(), initials: "P".into(), accent: String::new(), note: String::new() }];
    let all = build(&ws, &snap, false);
    let kinds_ids: Vec<(Kind, &str)> = all.iter().map(|i| (i.kind, i.id.as_str())).collect();
    assert!(kinds_ids.contains(&(Kind::Action, "new-chat")));
    assert!(kinds_ids.contains(&(Kind::Workspace, "w1")));
    assert!(kinds_ids.contains(&(Kind::Page, "engine")));
    let chats: Vec<&str> = all.iter().filter(|i| i.kind == Kind::Chat).map(|i| i.id.as_str()).collect();
    assert_eq!(chats, vec!["c2", "c1"], "newest first, archived hidden");
}
