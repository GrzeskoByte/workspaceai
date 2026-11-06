//! `opencode-config.ts` had no test file of its own; these pin the behaviour the supervisor tests rely on.
use serde_json::{json, Map, Value};
use workspaceai_shared::defaults::*;
use workspaceai_shared::opencode_config::*;
use workspaceai_shared::types::*;

fn ws() -> Workspace {
    let mut w = make_workspace("ws_1", "Demo", WorkspaceKind::Personal, "/h");
    w.providers = provider_presets().into_iter().filter(|(k, _)| *k == "anthropic" || *k == "ollama").map(|(_, p)| p).collect();
    w.default_model = "anthropic/claude-sonnet-5".into();
    w
}

#[test]
fn keys_are_env_references_never_literals() {
    let b = build_opencode_json(&ws(), None, &Map::new());
    assert_eq!(b.json["provider"]["anthropic"], json!({ "options": { "apiKey": "{env:ANTHROPIC_API_KEY}" } }));
    assert_eq!(b.json["provider"]["ollama"]["npm"], "@ai-sdk/openai-compatible");
    assert_eq!(b.json["model"], "anthropic/claude-sonnet-5");
    assert!(!contains_literal_secret(&b.json));
    assert_eq!(referenced_env(&b.json), vec!["ANTHROPIC_API_KEY"]);
    assert_eq!(b.env.get("OPENCODE_ENABLE_EXA").map(String::as_str), Some("true"));
    let mut bad = b.json.clone();
    bad.insert("x".into(), json!({ "apiKey": "sk-abcdefghijklmnop" }));
    assert!(contains_literal_secret(&bad));
}

#[test]
fn ask_first_gates_edit_and_bash_and_web_follows_the_mode() {
    let mut w = ws();
    w.web.mode = WebMode::Searxng;
    w.web.ask = true;
    let b = build_opencode_json(&w, None, &Map::new());
    assert_eq!(b.json["permission"]["edit"], "ask");
    assert_eq!(b.json["permission"]["searxng_*"], "ask");
    assert_eq!(b.json["mcp"]["searxng"]["environment"]["SEARXNG_URL"], "http://localhost:8888");
    w.acp_ask_first = false;
    assert!(build_opencode_json(&w, None, &Map::new()).json["permission"].get("edit").is_none());
}

#[test]
fn org_workspaces_get_only_what_policy_allows() {
    let mut w = make_workspace("ws_o", "Org", WorkspaceKind::Org, "/h");
    w.providers = vec![Provider { id: "mine".into(), ..Default::default() }];
    let p = demo_org_policy();
    let b = build_opencode_json(&w, Some(&p), &Map::new());
    let prov = b.json["provider"].as_object().unwrap();
    assert!(prov.contains_key("ai-factory") && !prov.contains_key("mine"));
    assert_eq!(prov["azure"]["options"]["baseURL"], "https://llm.[ORG-HOST]/v1"); // gateway wins
    assert!(b.json["mcp"].get("keycloak").is_some());
}

#[test]
fn web_preview_text() {
    let mut w = ws();
    w.web.mode = WebMode::Mcp;
    let t = web_config_preview(&w);
    assert!(t.starts_with("// opencode.json\n\"mcp\": {\n  \"tavily\": {\n    \"type\": \"local\","), "{t}");
    assert!(t.ends_with("\"permission\": {\"webfetch\":\"allow\",\"websearch\":\"deny\",\"tavily_*\":\"allow\"}"), "{t}");
    let _: Value = json!(null);
}

#[test]
fn defaults_match_the_ts_constants() {
    assert_eq!(default_agents(WorkspaceKind::Org).len(), 1);
    assert_eq!(default_agents(WorkspaceKind::Team).len(), 5);
    let o = od_settings(&Workspace { open_design: None, ..Default::default() });
    assert!(o.lint && o.auto_start && !o.agent_tools);
    assert_eq!(agents_template("Software", "X").unwrap(), "# X\n- Explain the plan before large edits.\n- Run the tests before calling a change done.\n- Keep commits small and described.\n");
    assert!(memory_rule(MemoryBackend::Both).starts_with("## Memory (Graphify)\n"));
    assert!(memory_rule(MemoryBackend::Facts).contains("`memory_search`") && !memory_rule(MemoryBackend::Facts).contains("graphify_query"));
    let g = general_project(&ws(), "/h");
    assert_eq!((g.id.as_str(), g.folder.as_str()), ("general_ws_1", "/h/ws/ws_1/general"));
    assert_eq!(provider_presets().iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec!["opencode", "anthropic", "openrouter", "ollama", "litellm", "custom"]);
}
