//! Port of `recall.test.ts` (the memory-recall part): strip the recall block, look up the words that matter,
//! and brief a session on the facts it already has. Graph recall is deferred with the Graphify backend.

mod common;

use common::{create, setup};
use serde_json::{json, Value};
use workspaceai_shared::types::{Fact, Project, Session as Chat, Workspace};
use workspaceai_supervisor::recall::{chat_memory, memory_brief, memory_recall, query_terms, strip_recall};

fn scopes(f: &common::Fixture, ws: &str) -> (Workspace, Vec<Project>, Vec<Fact>) {
    let snap = f.ok("GET", &format!("/api/ws/{ws}/snapshot"), None);
    (
        serde_json::from_value(snap["workspace"].clone()).unwrap(),
        serde_json::from_value(snap["projects"].clone()).unwrap(),
        serde_json::from_value(snap["facts"].clone()).unwrap(),
    )
}

#[test]
fn looks_up_the_words_that_matter() {
    assert_eq!(
        query_terms("Why does the refreshToken in session.ts expire so fast?"),
        ["refreshtoken", "session.ts", "expire", "fast"]
    );
    assert!(query_terms("why is it so").is_empty());
    assert_eq!(query_terms("src/auth/session.ts"), ["src/auth/session.ts"]);
}

#[test]
fn keeps_the_recall_block_out_of_the_transcript() {
    assert_eq!(strip_recall("fix it\n\n<harness-memory>\nFacts:\n- x\n</harness-memory>"), "fix it");
    assert_eq!(strip_recall("plain"), "plain");
    assert_eq!(
        strip_recall("go\n<harness-memory>\n- a\n</harness-memory>\n\n<harness-vault>\n- b\n</harness-vault>"),
        "go"
    );
    assert_eq!(strip_recall("<harness-memory>\n- a\n</harness-memory>"), "");
}

#[test]
fn hands_the_agent_approved_facts_for_this_message_only() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let p = f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Castform" })))["id"].as_str().unwrap().to_string();
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Default mold wall is 8 mm.", "scope": p })));
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Wall paint is green.", "scope": "ws", "status": "suggested" })));
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Wall height is 30 cm.", "scope": "ws" })));

    let (ws, projects, facts) = scopes(&f, &a);
    let block = memory_recall(&ws, &projects, &facts, &p, "What mold wall do we use?");
    assert!(block.starts_with("\n\n<harness-memory>"), "block: {block}");
    assert!(block.contains("Default mold wall is 8 mm."));
    assert!(block.contains("Wall height is 30 cm."));
    assert!(!block.contains("Wall paint is green."), "suggested facts are not recalled");
    assert!(block.contains("call graphify_query and memory_search for more"));
    assert!(block.ends_with("</harness-memory>"));

    // Nothing to say: no block at all (empty text, or no matching fact).
    assert_eq!(memory_recall(&ws, &projects, &facts, &p, "zebra xylophone"), "");
    assert_eq!(memory_recall(&ws, &projects, &facts, &p, ""), "");
}

#[test]
fn recall_respects_the_memory_switches() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Default mold wall is 8 mm.", "scope": "ws" })));

    f.ok("PATCH", &format!("/api/ws/{a}"), Some(json!({ "memory": "mcp" })));
    let (ws, projects, facts) = scopes(&f, &a);
    assert_eq!(memory_recall(&ws, &projects, &facts, "ws", "mold wall"), "");

    f.ok("PATCH", &format!("/api/ws/{a}"), Some(json!({ "memory": "facts", "memoryRecall": false })));
    let (ws, projects, facts) = scopes(&f, &a);
    assert_eq!(memory_recall(&ws, &projects, &facts, "ws", "mold wall"), "");

    f.ok("PATCH", &format!("/api/ws/{a}"), Some(json!({ "memoryRecall": true, "useMemory": false })));
    let (ws, projects, facts) = scopes(&f, &a);
    assert_eq!(memory_recall(&ws, &projects, &facts, "ws", "mold wall"), "");
    assert_eq!(memory_brief(&ws, &projects, &facts, "ws"), "");

    // `graph` alone has no facts to hand over; the graph backend is not ported.
    f.ok("PATCH", &format!("/api/ws/{a}"), Some(json!({ "useMemory": true, "memory": "graph" })));
    let (ws, projects, facts) = scopes(&f, &a);
    assert_eq!(memory_recall(&ws, &projects, &facts, "ws", "mold wall"), "");
}

#[test]
fn briefs_a_new_session_with_the_facts_it_already_knows() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let root = f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Castform" })))["id"].as_str().unwrap().to_string();
    let leaf = f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Sim", "parent": root })))["id"].as_str().unwrap().to_string();
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Workspace fact.", "scope": "ws" })));
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Root fact.", "scope": root })));
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Leaf fact.", "scope": leaf })));
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Hidden suggestion.", "scope": leaf, "status": "suggested" })));

    let (ws, projects, facts) = scopes(&f, &a);
    let brief = memory_brief(&ws, &projects, &facts, &leaf);
    assert!(brief.starts_with("### What memory holds now\nFacts you already know:"));
    // Narrow to broad: the project's own facts first, the workspace last.
    let leaf_at = brief.find("Leaf fact.").unwrap();
    let root_at = brief.find("Root fact.").unwrap();
    let ws_at = brief.find("Workspace fact.").unwrap();
    assert!(leaf_at < root_at && root_at < ws_at, "brief: {brief}");
    assert!(!brief.contains("Hidden suggestion."));
}

#[test]
fn chat_memory_is_empty_without_the_graph_backend() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    f.ok("PATCH", &format!("/api/ws/{a}"), Some(json!({ "memory": "graph" })));
    let (ws, projects, _) = scopes(&f, &a);
    let chat = Chat { id: "chat_1".into(), ..Default::default() };
    let nodes: Vec<Value> = chat_memory(&ws, &projects, &chat);
    assert!(nodes.is_empty());
}
