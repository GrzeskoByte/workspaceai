use workspaceai_shared::commands::*;

fn native(name: &str, description: Option<&str>, hint: Option<&str>) -> NativeCommand {
    NativeCommand { name: name.into(), description: description.map(Into::into), hint: hint.map(Into::into) }
}

#[test]
fn mention_at_finds_the_reference_being_typed() {
    assert_eq!(mention_at("@", 1), Some(Mention { start: 0, query: "".into() }));
    assert_eq!(mention_at("look at @src/ma", 15), Some(Mention { start: 8, query: "src/ma".into() }));
    assert_eq!(mention_at("look at @src now", 16), None);
    assert_eq!(mention_at("look at @src now", 12), Some(Mention { start: 8, query: "src".into() }));
    assert_eq!(mention_at("mail me@example.com", 19), None);
}

#[test]
fn insert_mention_says_where_the_caret_goes() {
    assert_eq!(insert_mention("see @ma please", 4, 7, "src/main.ts", false), ("see @src/main.ts  please".to_string(), 17));
    assert_eq!(insert_mention("@sr", 0, 3, "src/", true), ("@src/".to_string(), 5));
    assert_eq!(insert_mention("@my", 0, 3, "my file.md", false).0, "@\"my file.md\" ");
}

#[test]
fn multibyte_before_caret() {
    let text = "héllo @sr";
    assert_eq!(mention_at(text, text.len()), Some(Mention { start: 7, query: "sr".into() }));
    // A caret inside a character is not a position; no panic, no mention.
    assert_eq!(mention_at(text, 2), None);
    assert_eq!(insert_mention(text, 7, text.len(), "src/", true), ("héllo @src/".to_string(), 7 + 5));
    assert_eq!(insert_mention(text, 2, text.len(), "src/", true).0, text);
}

#[test]
fn command_action_maps_each_harness_command_to_the_agents_equivalent() {
    let status = find_harness_command("status").unwrap();
    assert_eq!(command_action(status, "claude"), Some(CommandAction::Ui { action: UiAction::Status, then: Some("/context".into()) }));
    assert_eq!(command_action(status, "grok"), Some(CommandAction::Ui { action: UiAction::Status, then: Some("/session-info".into()) }));
    assert_eq!(command_action(status, "opencode"), Some(CommandAction::Ui { action: UiAction::Status, then: None }));
    let review = find_harness_command("review").unwrap();
    assert_eq!(command_action(review, "claude"), Some(CommandAction::Native { command: "/code-review".into() }));
    assert_eq!(command_action(review, "grok"), Some(CommandAction::Native { command: "/review".into() }));
    assert_eq!(command_action(review, "opencode"), Some(CommandAction::OcCommand { name: "review".into() }));
    assert_eq!(command_action(find_harness_command("compact").unwrap(), "opencode"), Some(CommandAction::OcCompact));
    // No equivalent: not offered.
    assert_eq!(command_action(find_harness_command("init").unwrap(), "grok"), None);
    assert_eq!(command_action(find_harness_command("loop").unwrap(), "opencode"), None);
    // Aliases.
    assert_eq!(find_harness_command("clear").unwrap().name, "new");
    assert_eq!(find_harness_command("cost").unwrap().name, "usage");
}

#[test]
fn chat_commands_list_harness_first_then_uncovered_agent_commands() {
    let list = chat_commands(
        "claude",
        "Claude Code",
        &[native("compact", Some("theirs"), None), native("model", None, None), native("clear", None, None), native("simplify", Some("Clean up code"), Some("[target]"))],
    );
    let compact: Vec<_> = list.iter().filter(|c| c.name == "compact").collect();
    assert_eq!(compact.len(), 1);
    assert_eq!(compact[0].source, CommandSource::Harness);
    assert_eq!(compact[0].runs_as.as_deref(), Some("Claude Code /compact"));
    assert!(!list.iter().any(|c| c.source == CommandSource::Agent && (c.name == "model" || c.name == "clear")));
    assert_eq!(
        list.last().unwrap(),
        &ChatCommand { name: "simplify".into(), description: "Clean up code".into(), hint: Some("[target]".into()), source: CommandSource::Agent, runs_as: None, sheet: false }
    );
    assert_eq!(list.iter().find(|c| c.name == "status").unwrap().runs_as.as_deref(), Some("status card + Claude Code /context"));
    assert!(!chat_commands("grok", "Grok", &[]).iter().any(|c| c.name == "init"));
}

#[test]
fn parse_slash_splits_command_and_arguments() {
    assert_eq!(parse_slash("/effort high"), Some(("effort".to_string(), "high".to_string())));
    assert_eq!(parse_slash("  /data:analyze  why did it drop?\nmore "), Some(("data:analyze".to_string(), "why did it drop?\nmore".to_string())));
    assert_eq!(parse_slash("not a /command"), None);
    assert_eq!(parse_slash("/"), None);
    assert_eq!(parse_slash("/usr/bin is a path"), None);
}

#[test]
fn parse_slash_empty_input_is_none() {
    assert_eq!(parse_slash(""), None);
    assert_eq!(parse_slash("   "), None);
}
