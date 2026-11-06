//! Facts file format (new Rust-side checks; the TS side had no test file for `facts.ts`).
use workspaceai_shared::facts::*;
use workspaceai_shared::types::{Fact, FactStatus};

fn fact(id: &str, text: &str, status: FactStatus, source: Option<&str>) -> Fact {
    Fact { id: id.into(), text: text.into(), scope: "ws".into(), status, source: source.map(String::from), created_at: 42, ..Default::default() }
}

#[test]
fn facts_round_trip_through_markdown() {
    let facts = vec![fact("f1", "uses pnpm", FactStatus::Approved, None), fact("f2", "likes tabs\nand more <!-- x -->", FactStatus::Suggested, Some("chat é/1"))];
    let md = serialize_facts(&facts, "Workspace facts");
    assert!(md.starts_with("# Workspace facts\n\n<!-- Managed by WorkspaceAI."));
    let back = parse_facts(&md, "ws");
    assert_eq!(back.len(), 2);
    assert_eq!((back[0].id.as_str(), back[0].text.as_str(), back[0].created_at), ("f1", "uses pnpm", 42));
    assert_eq!(back[1].status, FactStatus::Suggested);
    assert_eq!(back[1].text, "likes tabs and more  x");
    assert_eq!(back[1].source.as_deref(), Some("chat é/1"));
}

#[test]
fn facts_without_meta_get_positional_ids() {
    let f = parse_facts("- a <!-- status:suggested -->\n- b <!-- at:7 -->\nnot a fact", "p1");
    assert_eq!(f.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), vec!["f_0", "f_1"]);
    assert_eq!((f[0].status, f[1].created_at, f[0].scope.as_str()), (FactStatus::Suggested, 7, "p1"));
}

#[test]
fn search_ranks_by_matching_terms_and_hides_suggestions() {
    let facts = vec![fact("1", "uses pnpm and vitest", FactStatus::Approved, None), fact("2", "uses pnpm", FactStatus::Approved, None), fact("3", "pnpm vitest", FactStatus::Suggested, None)];
    let ids = |q: &str| search_facts(&facts, q).iter().map(|f| f.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids("pnpm vitest"), vec!["1", "2"]);
    assert_eq!(ids("zzz"), Vec::<String>::new());
    assert_eq!(ids("a"), vec!["1", "2"]); // single-letter terms are ignored: everything approved
}
