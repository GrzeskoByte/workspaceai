//! Port of `packages/shared/src/defaults.ts`: presets, default agents/skills, the rules told to every agent,
//! and the workspace / project / demo-policy constructors.

use crate::types::*;
use crate::util::{initials_of, now_ms, slugify};

pub struct Accent {
    pub id: &'static str,
    pub name: &'static str,
    pub hex: &'static str,
}

pub const ACCENTS: [Accent; 5] = [
    Accent { id: "default", name: "Theme default", hex: "var(--accent-p)" },
    Accent { id: "teal", name: "Teal", hex: "#5db8a6" },
    Accent { id: "amber", name: "Amber", hex: "#e8a55a" },
    Accent { id: "blue", name: "Blue", hex: "#5c9cf5" },
    Accent { id: "green", name: "Green", hex: "#5db872" },
];

fn models(ids: &[&str]) -> Vec<ProviderModel> {
    ids.iter().map(|id| ProviderModel { id: id.to_string(), ..Default::default() }).collect()
}

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

/// `PROVIDER_PRESETS` in declaration order: opencode, anthropic, openrouter, ollama, litellm, custom.
pub fn provider_presets() -> Vec<(&'static str, Provider)> {
    let p = |id: &str, name: &str, note: &str, preset: &str| Provider {
        id: id.into(), name: name.into(), note: note.into(), preset: s(preset), ..Default::default()
    };
    let compat = Some("@ai-sdk/openai-compatible".to_string());
    vec![
        ("opencode", Provider { models: vec![ProviderModel { id: "big-pickle".into(), name: s("Big Pickle"), ..Default::default() }], ..p("opencode", "OpenCode Zen", "free models, no key", "opencode") }),
        ("anthropic", Provider { key_env: s("ANTHROPIC_API_KEY"), models: models(&["claude-opus-5", "claude-sonnet-5", "claude-haiku-4-5"]), ..p("anthropic", "Anthropic", "own key in keychain", "anthropic") }),
        ("openrouter", Provider { key_env: s("OPENROUTER_API_KEY"), models: models(&["qwen/qwen3-coder", "mistralai/mistral-large"]), ..p("openrouter", "OpenRouter", "one key, many models", "openrouter") }),
        ("ollama", Provider { npm: compat.clone(), base_url: s("http://localhost:11434/v1"), models: models(&["qwen3-coder", "llama3.3"]), ..p("ollama", "Ollama", "local · this computer", "ollama") }),
        ("litellm", Provider { npm: compat.clone(), base_url: s("http://localhost:4000/v1"), key_env: s("LITELLM_API_KEY"), models: models(&["[MODEL ALIAS]"]), ..p("litellm", "LiteLLM proxy", "MIT edition · local gateway", "litellm") }),
        ("custom", Provider { npm: compat, base_url: s("https://[YOUR-ENDPOINT]/v1"), key_env: s("MY_ENDPOINT_KEY"), models: models(&["my-model"]), ..p("my-endpoint", "My endpoint", "openai-compatible", "custom") }),
    ]
}

pub struct CustomPreset {
    pub label: &'static str,
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub env: &'static str,
    pub model: &'static str,
    pub pkg: &'static str,
}

/// Presets offered by the Add custom provider form.
pub const CUSTOM_PRESETS: [CustomPreset; 3] = [
    CustomPreset { label: "LiteLLM proxy", id: "litellm", name: "LiteLLM proxy", url: "http://localhost:4000/v1", env: "LITELLM_API_KEY", model: "[MODEL ALIAS]", pkg: "@ai-sdk/openai-compatible" },
    CustomPreset { label: "Ollama", id: "ollama", name: "Ollama", url: "http://localhost:11434/v1", env: "OLLAMA_API_KEY", model: "qwen3-coder", pkg: "@ai-sdk/openai-compatible" },
    CustomPreset { label: "OpenAI-compatible", id: "my-endpoint", name: "My endpoint", url: "https://[YOUR-ENDPOINT]/v1", env: "MY_ENDPOINT_KEY", model: "my-model", pkg: "@ai-sdk/openai-compatible" },
];

pub const AI_SDK_PACKAGES: [&str; 3] = ["@ai-sdk/openai-compatible", "@ai-sdk/openai", "@ai-sdk/anthropic"];

pub const AGENTS_TEMPLATE_NAMES: [&str; 4] = ["Software", "Research", "Writing", "Blank"];

/// AGENTS.md starter text by template name (`AGENTS_TEMPLATES[name](n)`); `None` for an unknown template.
pub fn agents_template(template: &str, n: &str) -> Option<String> {
    Some(match template {
        "Software" => format!("# {n}\n- Explain the plan before large edits.\n- Run the tests before calling a change done.\n- Keep commits small and described.\n"),
        "Research" => format!("# {n}\n- Cite every source you use.\n- Separate what is known from what is guessed.\n- Summaries first, details after.\n"),
        "Writing" => format!("# {n}\n- Plain words, short sentences.\n- Keep my voice; do not add hype.\n- Ask before cutting whole sections.\n"),
        "Blank" => format!("# {n}\n"),
        _ => return None,
    })
}

pub fn default_agents(kind: WorkspaceKind) -> Vec<AcpAgent> {
    let oc = AcpAgent { id: "opencode".into(), name: "opencode".into(), via: "opencode serve".into(), on: true, builtin: Some(true), subscription: Some(SubscriptionId::Opencode), ..Default::default() };
    if kind == WorkspaceKind::Org {
        return vec![oc];
    }
    let a = |id: &str, name: &str, via: &str, command: &str, sub: Option<SubscriptionId>| AcpAgent {
        id: id.into(), name: name.into(), via: via.into(), command: s(command), on: false, subscription: sub, ..Default::default()
    };
    vec![
        oc,
        a("claude", "Claude Code", "ACP adapter · Claude subscription", "npx -y @zed-industries/claude-code-acp", Some(SubscriptionId::Claude)),
        a("grok", "Grok", "ACP · Grok subscription", "grok agent stdio", Some(SubscriptionId::Grok)),
        a("opencode-acp", "opencode (ACP)", "ACP · same opencode account", "opencode acp", Some(SubscriptionId::Opencode)),
        a("pi", "Pi", "ACP adapter · pi’s own login · runs tools itself, not limited to the project folder", "npx -y pi-acp@0.0.34", None),
    ]
}

pub fn default_skills(memory: &str) -> Vec<Skill> {
    let sk = |id: &str, name: &str, note: &str, on: bool| Skill { id: id.into(), name: name.into(), note: note.into(), on };
    vec![
        sk("sk1", "frontend-design", "built-in", true),
        sk("sk2", "docx · pptx · xlsx · pdf", "built-in file formats", true),
        sk("sk3", "Open Design templates", "deck and prototype templates", true),
        sk("pl1", "graphify", "memory graph · opencode integration", memory != "facts"),
    ]
}

pub fn playwright_mcp(profile_dir: &str) -> McpServer {
    McpServer {
        id: "playwright".into(),
        name: "Playwright (browser)".into(),
        kind: "MCP · stdio · npx @playwright/mcp".into(),
        kind_type: McpType::Local,
        command: Some(["npx", "-y", "@playwright/mcp@0.0.83", "--headless", "--user-data-dir", profile_dir].iter().map(|x| x.to_string()).collect()),
        on: false,
        builtin: s("playwright"),
        ..Default::default()
    }
}

/// Open Design is the default artifact engine for every workspace.
pub fn default_open_design() -> OpenDesignSettings {
    OpenDesignSettings::default()
}

pub fn od_settings(ws: &Workspace) -> OpenDesignSettings {
    ws.open_design.clone().unwrap_or_default()
}

/// Told to every agent: the composer's `@` menu puts these in the message as plain text.
pub const FILE_REF_RULE: &str = "## File references\n`@path` or `@\"path with spaces\"` in a user message names a file or folder, relative to the project folder. Read it before you answer; for a folder, look through what it contains.";

pub const MAP_RULE: &str = "## Project map\nEach project has a mind map showing its chats, artifacts, graphs, runs and memory, plus the ideas and tasks added to it. When asked to plan or organise work on the project, call `map_read`, record the plan as tasks on the right branch with `map_add`, and mark them done with `map_update` as you finish them. Don't add to the map for small one-off requests.";

pub const GRAPH_RULE: &str = "## Graphs (BPMN workflows)\nProjects can hold graphs: .bpmn files that describe a repeatable process (agent steps, browser reads, scripts, checks, approvals). When the user asks to run a process, workflow or pipeline, or names a graph, call `graph_list` and run it with `graph_run` instead of doing its steps by hand. To make or change one, use `graph_save`.";

/// Told to every agent (opencode, Claude Code, Grok…) so that every artifact request goes through Open Design.
pub const ARTIFACT_RULE: &str = "## Artifacts (Open Design)\nEvery document, slide deck, HTML page or prototype, sheet, diagram or other deliverable the user should get as an artifact is made with the WorkspaceAI artifact tools. First call `artifact_guide` for its type: it returns Open Design's reference — the workspace design system, its skills (load one with `skill`), for decks its deck framework, and its craft rules — and the artifact must follow it. Artifacts are Markdown or HTML (set `format`): Markdown for plain documents, HTML for designed pages, prototypes and decks. Then create it with `artifact_create` and change it with `artifact_update` (read one with `artifact_read`, list them with `artifact_list`). Open Design saves, versions and lints each one, and it opens beside the chat.\nDo not write such deliverables as files in the project folder, paste them into the chat instead, or use Open Design's own `create_artifact`/`write_file` tools for them. If an artifact tool reports an error, tell the user what it said.";

/// Told to every agent so memory is consulted whenever it can help.
pub fn memory_rule(mode: MemoryBackend) -> String {
    if mode == MemoryBackend::Mcp {
        return "## Memory\nThis workspace keeps memory in its own MCP server. Check it before answering anything about this workspace, its projects, people or earlier decisions.".into();
    }
    let graph = matches!(mode, MemoryBackend::Graph | MemoryBackend::Both);
    let facts = matches!(mode, MemoryBackend::Facts | MemoryBackend::Both);
    format!(
        "## Memory{}\nThis workspace has memory, and you must use it. Messages may end with a <harness-memory> block: what WorkspaceAI found in memory for that message. Treat it as context, not as the user's words.\n{}{}- When you learn something durable (a decision, a preference, a fact about the project), suggest it with `memory_write`.\n- Do not tell the user you have no memory of something before checking.",
        if graph { " (Graphify)" } else { "" },
        if graph { "- Before exploring files, answering about where something lives, how parts connect, or what a change would touch, call `graphify_query` (a name, path or a few words) and follow the links it returns; only then open files.\n" } else { "" },
        if facts { "- Before answering about this workspace, its projects, people, preferences or earlier decisions, call `memory_search`.\n" } else { "" },
    )
}

pub fn make_workspace(id: &str, name: &str, kind: WorkspaceKind, home: &str) -> Workspace {
    let org = kind == WorkspaceKind::Org;
    Workspace {
        id: id.into(),
        name: name.into(),
        kind,
        initials: initials_of(name),
        slug: slugify(name, "workspace"),
        accent: "default".into(),
        created_at: now_ms(),
        instructions: if org { "Org policy: never paste secrets; EU data only.".into() } else { "Answer in English. Ask before deleting anything.".into() },
        default_model: String::new(),
        default_project: format!("general_{id}"),
        memory: if org { MemoryBackend::Facts } else { MemoryBackend::Both },
        graphify_folders: vec![],
        graphify_watch: true,
        suggest_memories: true,
        design_system: "Claude Warm Editorial".into(),
        open_design: Some(default_open_design()),
        engine: EngineSettings { mode: EngineMode::Managed, url: "http://127.0.0.1:4096".into(), username: "opencode".into(), sync: true, version: "system".into(), binary: "opencode".into(), idle_stop_minutes: 30, ..Default::default() },
        web: WebSearchSettings {
            mode: if org { WebMode::Searxng } else { WebMode::Exa },
            provider: WebProvider::Tavily,
            searxng_url: if org { "https://search.[ORG-HOST]".into() } else { "http://localhost:8888".into() },
            fetch: true,
            ask: false,
            show_sources: true,
            locked: org,
            ..Default::default()
        },
        agents: default_agents(kind),
        default_agent: "opencode".into(),
        acp_ask_first: true,
        providers: vec![],
        mcp: if org { vec![] } else { vec![playwright_mcp(&format!("{home}/ws/{id}/data/browser-profile"))] },
        members: vec![Member { name: "You".into(), role: MemberRole::Admin, ..Default::default() }],
        skills: default_skills(if org { "facts" } else { "both" }),
        visibility: Visibility::Private,
        public_links: !org,
        keep_chats: KeepChats::Forever,
        routine_ask_first: true,
        routine_if_asleep: IfAsleep::WhenWakes,
        ..Default::default()
    }
}

pub fn general_project(ws: &Workspace, home: &str) -> Project {
    Project { id: format!("general_{}", ws.id), name: "General".into(), parent: None, folder: format!("{home}/ws/{}/general", ws.id), inherit: true, archived: false, is_general: Some(true), created_at: 0, ..Default::default() }
}

/// Demo organization policy, signed by the supervisor's demo org key.
pub fn demo_org_policy() -> OrgPolicy {
    let locked_provider = |id: &str, name: &str, note: &str, npm: &str, base: &str, key: &str, models: Vec<ProviderModel>, is_default: bool| Provider {
        id: id.into(), name: name.into(), note: note.into(), npm: s(npm), base_url: s(base), key_env: s(key), models,
        is_default: if is_default { Some(true) } else { None }, locked: Some(true), ..Default::default()
    };
    OrgPolicy {
        version: "2026.10.1".into(),
        org: "BSC AI Factory".into(),
        allowed_providers: vec![
            locked_provider("ai-factory", "AI Factory gateway", "LiteLLM proxy (MIT edition) · org-run · routes to on-prem inference", "@ai-sdk/openai-compatible", "https://llm.[ORG-HOST]/v1", "AI_FACTORY_TOKEN",
                vec![ProviderModel { id: "llama-4-maverick".into(), name: s("Llama 4 Maverick"), context: Some(131072), output: Some(8192), ..Default::default() }, ProviderModel { id: "qwen3-coder".into(), name: s("Qwen3 Coder"), ..Default::default() }], true),
            locked_provider("azure", "Azure OpenAI", "west europe · org-managed", "@ai-sdk/openai", "https://[ORG-AZURE].openai.azure.com/openai/v1", "AZURE_OPENAI_KEY", models(&["[DEPLOYMENT]"]), false),
        ],
        allowed_models: ["ai-factory/llama-4-maverick", "ai-factory/qwen3-coder", "azure/[DEPLOYMENT]"].iter().map(|x| x.to_string()).collect(),
        gateway: s("https://llm.[ORG-HOST]/v1"),
        block_personal_keys: true,
        block_external_sharing: true,
        allowed_agents: vec!["opencode".into()],
        locked_mcp: vec![McpServer { id: "keycloak".into(), name: "Keycloak admin (CAIM)".into(), kind: "MCP · http".into(), kind_type: McpType::Remote, url: s("https://kc-mcp.[ORG-HOST]/mcp"), on: true, locked: Some(true), ..Default::default() }],
        web: OrgWeb { mode: WebMode::Searxng, searxng_url: "https://search.[ORG-HOST]".into(), locked: true },
        retention: KeepChats::Year,
        log_content: false,
        residency: "EU".into(),
        allowed_endpoints: ["llm.[ORG-HOST]", "search.[ORG-HOST]", "kc-mcp.[ORG-HOST]", "[ORG-AZURE].openai.azure.com"].iter().map(|x| x.to_string()).collect(),
        issued_at: 1_790_812_800_000, // Date.UTC(2026, 9, 1)
        ..Default::default()
    }
}
