use workspaceai_client::models::*;
use workspaceai_client::types::Chat;

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
fn efforts_come_from_the_chosen_model() {
    let l = list();
    assert_eq!(l.efforts_for("opencode", "anthropic/opus"), vec!["low", "high"]);
    assert!(l.efforts_for("opencode", "anthropic/haiku").is_empty());
    assert_eq!(l.efforts_for("claude", "sonnet"), vec!["medium", "max"]);
    assert!(l.efforts_for("opencode", "missing").is_empty());
}

#[test]
fn label_names_the_model_or_falls_back() {
    let l = list();
    assert_eq!(l.label("opencode", "anthropic/opus", Some("high")), "Opus · high");
    assert_eq!(l.label("opencode", "anthropic/opus", Some("")), "Opus");
    assert_eq!(l.label("opencode", "", None), "Default model");
    assert_eq!(l.label("opencode", "x/unknown", None), "x/unknown");
    // An ACP agent with no model chosen shows the agent's current one.
    assert_eq!(l.label("claude", "", None), "Sonnet");
}

#[test]
fn an_empty_or_odd_reply_parses_to_nothing_offered() {
    let l: ModelList = serde_json::from_str("{}").unwrap();
    assert!(l.options_for("opencode").is_empty());
}

#[test]
fn chat_carries_agent_model_and_effort() {
    let c: Chat = serde_json::from_str(r#"{"id":"c","project":"p","title":"t","agent":"claude","model":"sonnet","effort":"max"}"#).unwrap();
    assert_eq!((c.agent.as_str(), c.model.as_str(), c.effort.as_deref()), ("claude", "sonnet", Some("max")));
    let bare: Chat = serde_json::from_str(r#"{"id":"c","project":"p"}"#).unwrap();
    assert_eq!((bare.agent.as_str(), bare.model.as_str(), bare.effort), ("", "", None));
}
