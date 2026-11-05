use serde_json::json;
use workspaceai_client::command::*;
use workspaceai_shared::commands::{ChatCommand, CommandSource};

#[test]
fn ran_and_new_and_help() {
    assert_eq!(interpret(&json!({"ran": "/compact"})), Outcome::Ran("/compact".into()));
    assert_eq!(interpret(&json!({"ui": "new", "args": ""})), Outcome::NewChat);
    assert_eq!(interpret(&json!({"ui": "help"})), Outcome::Help);
}

#[test]
fn status_becomes_readable_lines() {
    let o = interpret(&json!({"ui": "status", "status": {"agentName": "Claude Code", "model": "opus", "effort": "high", "project": "Work / API", "session": "ses_1", "engine": "running"}}));
    let Outcome::Text { title, body } = o else { panic!("{o:?}") };
    assert_eq!(title, "Status");
    for want in ["Claude Code", "opus", "high", "Work / API"] {
        assert!(body.contains(want), "{body}");
    }
}

#[test]
fn sheet_shows_the_agents_text_then_any_note() {
    let o = interpret(&json!({"ui": "sheet", "sheet": {"command": "usage", "title": "Usage", "text": "cost $1", "note": "still answering"}}));
    assert_eq!(o, Outcome::Text { title: "Usage".into(), body: "cost $1\n\nstill answering".into() });
}

#[test]
fn sheet_without_text_falls_back_to_the_status() {
    let o = interpret(&json!({"ui": "sheet", "sheet": {"title": "Context", "status": {"agentName": "opencode", "model": "m"}}}));
    let Outcome::Text { title, body } = o else { panic!("{o:?}") };
    assert_eq!(title, "Context");
    assert!(body.contains("opencode"));
}

#[test]
fn dialogs_the_native_app_does_not_have_yet_are_named() {
    for ui in ["model", "effort", "graph"] {
        assert_eq!(interpret(&json!({"ui": ui})), Outcome::Unsupported(ui.into()));
    }
}

#[test]
fn unknown_or_empty_replies_do_not_panic() {
    assert_eq!(interpret(&json!({})), Outcome::Ran(String::new()));
    assert_eq!(interpret(&json!("text")), Outcome::Ran(String::new()));
    assert_eq!(interpret(&json!({"ui": 5})), Outcome::Ran(String::new()));
}

fn cmd(name: &str) -> ChatCommand {
    ChatCommand { name: name.into(), description: String::new(), hint: None, source: CommandSource::Harness, runs_as: None, sheet: false }
}

#[test]
fn suggestions_match_a_typed_prefix_only_while_typing_the_name() {
    let list = vec![cmd("status"), cmd("simplify"), cmd("model"), cmd("Search")];
    let names = |t: &str| suggestions(t, &list).iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    assert_eq!(names("/"), vec!["status", "simplify", "model", "Search"]);
    assert_eq!(names("/s"), vec!["status", "simplify", "Search"]);
    assert_eq!(names("/ST"), vec!["status"]);
    assert!(names("/status ").is_empty(), "once arguments start, stop suggesting");
    assert!(names("hello").is_empty());
    assert!(names("").is_empty());
}

#[test]
fn suggestions_are_capped() {
    let list: Vec<_> = (0..30).map(|i| cmd(&format!("c{i}"))).collect();
    assert_eq!(suggestions("/", &list).len(), 8);
}

#[test]
fn the_servers_command_list_json_deserializes() {
    let list: Vec<ChatCommand> = serde_json::from_str(
        r#"[{"name":"status","description":"Agent, model","source":"harness","runsAs":"status card + Claude Code /context","sheet":true},
            {"name":"simplify","description":"","hint":"[target]","source":"agent"}]"#,
    )
    .unwrap();
    assert_eq!(list[0].runs_as.as_deref(), Some("status card + Claude Code /context"));
    assert!(list[0].sheet && !list[1].sheet);
    assert_eq!(list[1].source, CommandSource::Agent);
}
