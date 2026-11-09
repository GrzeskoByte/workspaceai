use workspaceai_client::models::*;

const REPLY: &str = r#"{
  "models": ["anthropic/opus"],
  "groups": [
    {"provider":"anthropic","name":"Anthropic","note":"API key","models":[
      {"id":"anthropic/opus","name":"Opus","efforts":["low","high"]},
      {"id":"anthropic/haiku","name":"Haiku"}]},
    {"provider":"local","name":"Local","note":"no key needed","models":[{"id":"local/llama","name":"Llama"}]}
  ],
  "agents": {"claude": {"models":[{"id":"sonnet","name":"Sonnet","efforts":["medium","max"]}],"current":"sonnet"}}
}"#;

fn list() -> ModelList {
    serde_json::from_str(REPLY).unwrap()
}

#[test]
fn opencode_chats_offer_every_connected_providers_models() {
    let ids: Vec<_> = list().options_for("opencode").iter().map(|m| m.id.clone()).collect();
    assert_eq!(ids, vec!["anthropic/opus", "anthropic/haiku", "local/llama"]);
}

#[test]
fn agent_chats_offer_that_agents_own_models_and_unknown_agents_none() {
    let l = list();
    assert_eq!(l.options_for("claude").iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), vec!["Sonnet"]);
    assert!(l.options_for("nobody").is_empty());
}

#[test]
fn an_empty_or_odd_reply_parses_to_nothing_offered() {
    let l: ModelList = serde_json::from_str("{}").unwrap();
    assert!(l.options_for("opencode").is_empty());
}
