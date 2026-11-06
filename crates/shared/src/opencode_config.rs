//! Builds the opencode.json a workspace's server reads. Port of `packages/shared/src/opencode-config.ts`.
//! Keys never enter the file: every secret is an `{env:NAME}` reference and the supervisor injects the value from
//! the keychain into that workspace's process environment (R-20, R-34). Org workspaces are written from policy only.

use crate::types::*;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// (npm package, env var) of a web search MCP provider.
pub fn web_provider(p: WebProvider) -> (&'static str, &'static str) {
    match p {
        WebProvider::Tavily => ("tavily-mcp", "TAVILY_API_KEY"),
        WebProvider::Brave => ("@brave/brave-search-mcp-server", "BRAVE_API_KEY"),
        WebProvider::Firecrawl => ("firecrawl-mcp", "FIRECRAWL_API_KEY"),
    }
}

/// Providers opencode resolves itself; only custom ones need a `provider` block.
pub const NATIVE_PROVIDERS: [&str; 6] = ["anthropic", "openrouter", "openai", "opencode", "google", "mistral"];

pub fn provider_block(p: &Provider) -> Option<Value> {
    let mut options = Map::new();
    if let Some(u) = p.base_url.as_ref().filter(|u| !u.is_empty()) {
        options.insert("baseURL".into(), json!(u));
    }
    if let Some(k) = p.key_env.as_ref().filter(|k| !k.is_empty()) {
        options.insert("apiKey".into(), json!(format!("{{env:{k}}}")));
    }
    if NATIVE_PROVIDERS.contains(&p.id.as_str()) {
        // Native provider: opencode already knows its SDK and models; point it at the key, and at a proxy if one is set.
        return if options.is_empty() { None } else { Some(json!({ "options": options })) };
    }
    let mut models = Map::new();
    for m in &p.models {
        let mut entry = Map::new();
        entry.insert("name".into(), json!(m.name.as_deref().filter(|n| !n.is_empty()).unwrap_or(&m.id)));
        if m.context.unwrap_or(0) != 0 || m.output.unwrap_or(0) != 0 {
            let c = m.context.filter(|c| *c != 0).unwrap_or(128000);
            let o = m.output.filter(|o| *o != 0).unwrap_or(8192);
            entry.insert("limit".into(), json!({ "context": c, "output": o }));
        }
        models.insert(m.id.clone(), Value::Object(entry));
    }
    let npm = p.npm.as_deref().filter(|n| !n.is_empty()).unwrap_or("@ai-sdk/openai-compatible");
    Some(json!({ "npm": npm, "name": p.name, "options": options, "models": models }))
}

pub fn mcp_block(m: &McpServer) -> Option<Value> {
    if m.kind_type == McpType::Connector {
        return None;
    }
    let environment: Map<String, Value> = m.env.iter().flatten().map(|(name, _)| (name.clone(), json!(format!("{{env:{name}}}")))).collect();
    let mut out = Map::new();
    if m.kind_type == McpType::Remote {
        out.insert("type".into(), json!("remote"));
        if let Some(u) = &m.url {
            out.insert("url".into(), json!(u));
        }
        out.insert("enabled".into(), json!(m.on));
        if !environment.is_empty() {
            out.insert("headers".into(), Value::Object(environment));
        }
    } else {
        out.insert("type".into(), json!("local"));
        out.insert("command".into(), json!(m.command.clone().unwrap_or_default()));
        out.insert("enabled".into(), json!(m.on));
        if !environment.is_empty() {
            out.insert("environment".into(), Value::Object(environment));
        }
    }
    Some(Value::Object(out))
}

pub struct WebConfig {
    pub mcp: Map<String, Value>,
    pub permission: Map<String, Value>,
    pub env: BTreeMap<String, String>,
}

pub fn web_search_config(ws: &Workspace) -> WebConfig {
    let w = &ws.web;
    let level = if w.ask { "ask" } else { "allow" };
    let mut permission = Map::new();
    permission.insert("webfetch".into(), json!(if w.fetch { level } else { "deny" }));
    let mut mcp = Map::new();
    let mut env = BTreeMap::new();
    match w.mode {
        WebMode::Off => {
            permission.insert("websearch".into(), json!("deny"));
        }
        WebMode::Exa => {
            permission.insert("websearch".into(), json!(level));
            env.insert("OPENCODE_ENABLE_EXA".into(), "true".into());
        }
        WebMode::Mcp => {
            let (pkg, var) = web_provider(w.provider);
            let id = w.provider.as_str().to_lowercase();
            mcp.insert(id.clone(), json!({ "type": "local", "command": ["npx", "-y", pkg], "environment": { var: format!("{{env:{var}}}") }, "enabled": true }));
            permission.insert("websearch".into(), json!("deny"));
            permission.insert(format!("{id}_*"), json!(level));
        }
        WebMode::Searxng => {
            mcp.insert("searxng".into(), json!({ "type": "local", "command": ["npx", "-y", "mcp-searxng"], "environment": { "SEARXNG_URL": w.searxng_url }, "enabled": true }));
            permission.insert("websearch".into(), json!("deny"));
            permission.insert("searxng_*".into(), json!(level));
        }
    }
    WebConfig { mcp, permission, env }
}

/// Effective providers: org workspaces only ever get what the policy allows (R-07).
pub fn effective_providers(ws: &Workspace, policy: Option<&OrgPolicy>) -> Vec<Provider> {
    let (true, Some(policy)) = (ws.kind == WorkspaceKind::Org, policy) else { return ws.providers.clone() };
    // With an org gateway (LiteLLM, MIT edition) every allowed model is reached through that one URL, with the
    // gateway's key; the gateway maps model names to backends (R-40).
    let gw = policy.gateway.as_deref().map(|g| g.trim_end_matches('/'));
    let gw_key = policy
        .allowed_providers
        .iter()
        .find(|p| gw.is_some() && p.base_url.as_deref().map(|b| b.trim_end_matches('/')) == gw)
        .and_then(|p| p.key_env.clone())
        .or_else(|| policy.allowed_providers.first().and_then(|p| p.key_env.clone()));
    policy
        .allowed_providers
        .iter()
        .map(|p| match gw {
            Some(g) => Provider { npm: Some("@ai-sdk/openai-compatible".into()), base_url: Some(g.to_string()), key_env: gw_key.clone(), locked: Some(true), ..p.clone() },
            None => Provider { locked: Some(true), ..p.clone() },
        })
        .collect()
}

pub struct Built {
    pub json: Map<String, Value>,
    pub env: BTreeMap<String, String>,
}

pub fn build_opencode_json(ws: &Workspace, policy: Option<&OrgPolicy>, extra_mcp: &Map<String, Value>) -> Built {
    let mut json = Map::new();
    json.insert("$schema".into(), json!("https://opencode.ai/config.json"));
    let mut providers = Map::new();
    for p in effective_providers(ws, policy) {
        if p.from_opencode == Some(true) {
            continue;
        }
        if let Some(b) = provider_block(&p) {
            providers.insert(p.id.clone(), b);
        }
    }
    if !providers.is_empty() {
        json.insert("provider".into(), Value::Object(providers));
    }
    if !ws.default_model.is_empty() {
        json.insert("model".into(), json!(ws.default_model));
    }
    let org_policy = policy.filter(|_| ws.kind == WorkspaceKind::Org);
    let web = match org_policy {
        Some(p) => {
            let mut w = ws.clone();
            w.web.mode = p.web.mode;
            w.web.searxng_url = p.web.searxng_url.clone();
            web_search_config(&w)
        }
        None => web_search_config(ws),
    };
    let mut mcp = web.mcp.clone();
    mcp.extend(extra_mcp.clone());
    let servers: Vec<&McpServer> = match org_policy {
        Some(p) => p.locked_mcp.iter().chain(ws.mcp.iter().filter(|m| !p.locked_mcp.iter().any(|l| l.id == m.id))).collect(),
        None => ws.mcp.iter().collect(),
    };
    for m in servers {
        if let Some(b) = mcp_block(m) {
            mcp.insert(m.id.clone(), b);
        }
    }
    if !mcp.is_empty() {
        json.insert("mcp".into(), Value::Object(mcp));
    }
    let mut permission = web.permission;
    if ws.acp_ask_first {
        for k in ["edit", "bash", "harness-vault_vault_write", "harness-vault_vault_edit"] {
            permission.insert(k.into(), json!("ask"));
        }
    }
    json.insert("permission".into(), Value::Object(permission));
    Built { json, env: web.env }
}

static ENV_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{env:([A-Z0-9_]+)\}").unwrap());
static SECRET_KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)"(?:apiKey|token|password|secret)"\s*:\s*"([^"]+)""#).unwrap());
static SK_TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"sk-[A-Za-z0-9]{12,}").unwrap());

/// Every env var name the config references; the supervisor fills these from the keychain.
pub fn referenced_env(json: &Map<String, Value>) -> Vec<String> {
    let s = Value::Object(json.clone()).to_string();
    let mut out: Vec<String> = Vec::new();
    for m in ENV_REF.captures_iter(&s) {
        if !out.contains(&m[1].to_string()) {
            out.push(m[1].to_string());
        }
    }
    out
}

/// Guard used in tests and before every write: a literal secret must never appear in opencode.json (R-34).
pub fn contains_literal_secret(json: &Map<String, Value>) -> bool {
    let s = Value::Object(json.clone()).to_string();
    SECRET_KEY.captures_iter(&s).any(|m| { let v = m[1].to_lowercase(); !(v.starts_with("{env:") || v.starts_with("{file:")) }) || SK_TOKEN.is_match(&s)
}

/// Text preview shown in onboarding and Settings › Web search.
pub fn web_config_preview(ws: &Workspace) -> String {
    let WebConfig { mcp, permission, env } = web_search_config(ws);
    let mut lines: Vec<String> = Vec::new();
    if !env.is_empty() {
        lines.push("# env of this workspace's opencode serve".into());
        lines.extend(env.iter().map(|(k, v)| format!("{k}={v}")));
        lines.push(String::new());
    }
    lines.push("// opencode.json".into());
    if !mcp.is_empty() {
        lines.push(format!("\"mcp\": {},", serde_json::to_string_pretty(&mcp).unwrap()));
    }
    lines.push(format!("\"permission\": {}", Value::Object(permission)));
    lines.join("\n")
}
