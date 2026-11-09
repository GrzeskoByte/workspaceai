//! Terminal session adapters (plan (c)): what to run for a session of each agent. A `match` on `agent`, no trait.
//! The app spawns `argv` in a PTY with exactly `env` (nothing inherited) in `cwd`.

use crate::engine::lifecycle::base_env;
use crate::jsonfile::write_json;
use crate::paths::expand_home;
use crate::recall::memory_brief;
use crate::runtime::Runtime;
use crate::supervisor::ApiError;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use workspaceai_shared::defaults::{memory_rule, ARTIFACT_RULE, FILE_REF_RULE};
use workspaceai_shared::opencode_config::{build_opencode_json, effective_providers, referenced_env, NATIVE_PROVIDERS};
use workspaceai_shared::types::{EngineMode, Provider, Session};

/// The adapters this build knows.
pub const AGENTS: [&str; 4] = ["opencode", "claude", "grok", "pi"];

#[derive(Debug, Clone, Serialize)]
pub struct LaunchSpec {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    /// Files the supervisor wrote for this launch (already on disk).
    pub files: Vec<String>,
}

/// A random UUIDv4, the id format claude and pi accept for a fixed session id.
pub fn uuid4() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("os randomness");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Claude Code's folder name for a cwd under `projects/`: every non-alphanumeric char becomes `-`.
pub fn claude_slug(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

impl Runtime {
    /// `<ws>/data/agents/<agent>`: the agent's own home, never the user's (`~/.claude`, `~/.grok`).
    pub fn agent_home(&self, agent: &str) -> PathBuf {
        self.paths.agents.join(agent)
    }

    pub fn project_folder(&self, pid: &str) -> String {
        let p = self.project(pid).or_else(|| self.general());
        expand_home(p.map(|p| p.folder.as_str()).unwrap_or("~"))
    }

    pub fn effective_model(&self, pid: &str) -> String {
        let chain = workspaceai_shared::util::project_path(&self.state.projects, Some(pid));
        chain.iter().rev().find(|p| !p.model.is_empty()).map(|p| p.model.clone()).unwrap_or_else(|| self.ws.default_model.clone())
    }

    /// `inheritedPrefix`: workspace instructions and parents' AGENTS.md, plus the artifact and memory rules.
    /// The project's own AGENTS.md is read by the agent from the folder (R-12).
    /// ponytail: graph/map rules dropped (non-goal); lean-ctx and vault rules arrive with B12.
    pub fn inherited_prefix(&self, pid: &str, instructions: &str) -> String {
        let mut parts: Vec<String> = vec![FILE_REF_RULE.into(), ARTIFACT_RULE.into()];
        if self.ws.use_memory != Some(false) {
            parts.push(memory_rule(self.ws.memory));
            let brief = memory_brief(&self.ws, &self.state.projects, &self.all_facts(), pid);
            if !brief.is_empty() {
                parts.push(brief);
            }
        }
        for l in self.instruction_layers(pid).into_iter().filter(|l| l.kind != "project" && !l.text.trim().is_empty()) {
            let head = if l.kind == "workspace" { "Workspace instructions".to_string() } else { format!("Inherited from {} ({})", l.name, l.file) };
            parts.push(format!("## {head}\n{}", l.text.trim()));
        }
        if !instructions.trim().is_empty() {
            parts.push(format!("## Chat instructions\n{}", instructions.trim()));
        }
        format!("WorkspaceAI instructions, broad to narrow. The project folder's own AGENTS.md is narrower and wins on conflict.\n\n{}", parts.join("\n\n"))
    }

    /// The claude transcript of a session, wherever claude filed it under its projects folder.
    pub fn claude_transcript(&self, s: &Session) -> Option<PathBuf> {
        if s.sid.is_empty() || !crate::paths::valid_id(&s.sid) {
            return None;
        }
        let projects = self.agent_home("claude").join("projects");
        let direct = projects.join(claude_slug(&self.project_folder(&s.project))).join(format!("{}.jsonl", s.sid));
        if direct.is_file() {
            return Some(direct);
        }
        std::fs::read_dir(&projects).ok()?.flatten().map(|d| d.path().join(format!("{}.jsonl", s.sid))).find(|p| p.is_file())
    }

    /// The MCP servers a claude session gets, as `--mcp-config` JSON: the workspace's servers plus memory and
    /// artifacts, each tagged with `HARNESS_CHAT` so artifacts are attributed to this session (R-13).
    /// `{env:X}` references become claude's `${X}`, with X in the process env, so no secret is written to disk.
    fn claude_mcp(&self, s: &Session, env: &mut BTreeMap<String, String>) -> Value {
        let mut servers = Map::new();
        let built = build_opencode_json(&self.ws, self.policy.as_ref(), &self.supervisor_mcp(Some(&s.id)));
        for name in referenced_env(&built.json) {
            if let Some(v) = self.keychain.get(&self.id, &name) {
                env.insert(name, v);
            }
        }
        env.extend(built.env);
        env.extend(self.supervisor_env());
        let refs = |v: &Value| -> Value {
            let s = v.to_string();
            serde_json::from_str(&regex_lite_env(&s)).unwrap_or(Value::Null)
        };
        for (name, raw) in built.json.get("mcp").and_then(Value::as_object).into_iter().flatten() {
            if raw["enabled"] == json!(false) {
                continue;
            }
            if raw["type"] == "remote" {
                servers.insert(name.clone(), json!({ "type": "http", "url": raw["url"], "headers": refs(&raw["headers"]) }));
            } else if let Some([cmd, args @ ..]) = raw["command"].as_array().map(Vec::as_slice) {
                let env_block = if raw["environment"].is_object() { refs(&raw["environment"]) } else { json!({}) };
                servers.insert(name.clone(), json!({ "command": cmd, "args": args, "env": env_block }));
            }
        }
        json!({ "mcpServers": servers })
    }

    /// The supervisor's own MCP servers, served as `workspaceai mcp <kind>` by the app binary (`crate::mcp`).
    /// The loopback URL and token are `{env:…}` references resolved from the agent's process env, so no
    /// secret lands in argv or a config file. `session` is set where the agent takes per-session config (R-13).
    pub fn supervisor_mcp(&self, session: Option<&str>) -> Map<String, Value> {
        let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "workspaceai".into());
        let mut env = json!({
            "HARNESS_MEMORY_DIR": self.paths.memory, "HARNESS_MEMORY_BACKEND": self.ws.memory.as_str(), "HARNESS_WS": self.id,
            "HARNESS_SUPERVISOR": "{env:HARNESS_SUPERVISOR}", "HARNESS_SUPERVISOR_TOKEN": "{env:HARNESS_SUPERVISOR_TOKEN}",
        });
        if let Some(s) = session {
            env["HARNESS_CHAT"] = json!(s);
        }
        let mut out = Map::new();
        let entry = |kind: &str| json!({ "type": "local", "command": [exe, "mcp", kind], "enabled": true, "environment": env });
        if self.ws.use_memory != Some(false) {
            out.insert("harness-memory".into(), entry("memory"));
        }
        out.insert("harness-artifacts".into(), entry("artifacts"));
        // Always registered; scope is read on each call, so attaching a vault never restarts running sessions.
        out.insert("harness-vault".into(), entry("vault"));
        out
    }

    /// `GET /api/ws/:id/chats/:cid/launch`. Marks the session launched and spends its first prompt.
    pub fn launch_spec(&mut self, cid: &str) -> Result<LaunchSpec, ApiError> {
        let s = self.session(cid).cloned().ok_or_else(|| ApiError::not_found("Session not found"))?;
        let cwd = self.project_folder(&s.project);
        std::fs::create_dir_all(&cwd)?;
        let first_prompt = if s.launched { None } else { s.prompt.clone().filter(|p| !p.trim().is_empty()) };
        let mut env = base_env();
        let mut files = Vec::new();
        let argv = match s.agent.as_str() {
            "opencode" => {
                let client = self.engine_client()?;
                let password = match self.ws.engine.mode {
                    EngineMode::Remote => self.keychain.get(&self.id, self.ws.engine.password_secret.as_deref().unwrap_or("OPENCODE_SERVER_PASSWORD")).unwrap_or_default(),
                    EngineMode::Managed => self.engine.as_ref().map(|e| e.password.clone()).unwrap_or_default(),
                };
                for (k, v) in [
                    ("XDG_DATA_HOME", &self.paths.xdg_data),
                    ("XDG_CONFIG_HOME", &self.paths.xdg_config),
                    ("XDG_STATE_HOME", &self.paths.xdg_state),
                    ("XDG_CACHE_HOME", &self.paths.xdg_cache),
                    ("OPENCODE_CONFIG", &self.paths.opencode_json),
                ] {
                    env.insert(k.into(), v.to_string_lossy().into_owned());
                }
                let user = if self.ws.engine.mode == EngineMode::Remote && !self.ws.engine.username.is_empty() { self.ws.engine.username.clone() } else { "opencode".into() };
                env.insert("OPENCODE_SERVER_USERNAME".into(), user);
                env.insert("OPENCODE_SERVER_PASSWORD".into(), password);
                let bin = crate::engine::detect::opencode_binary(&self.home, &self.ws.engine).unwrap_or_else(|_| "opencode".into());
                let mut a = vec![bin, "--server".into(), client.url().to_string(), "--session".into(), s.sid.clone()];
                // Same rule as claude's flag: explicit `full_access` only; the UI (B8) warns when it is turned on.
                if s.full_access {
                    a.push("--auto".into());
                }
                if let Some(p) = &first_prompt {
                    a.extend(["--prompt".into(), p.clone()]);
                }
                a.push(cwd.clone());
                a
            }
            "claude" => {
                let home = self.agent_home("claude");
                std::fs::create_dir_all(&home)?;
                env.insert("CLAUDE_CONFIG_DIR".into(), home.to_string_lossy().into_owned());
                let mcp_file = self.paths.run.join(format!("{}.mcp.json", s.sid));
                write_json(&mcp_file, &self.claude_mcp(&s, &mut env))?;
                files.push(mcp_file.to_string_lossy().into_owned());
                let mut a: Vec<String> = vec!["claude".into()];
                // The transcript on disk decides resume vs new, so a crash before the first message cannot orphan it.
                match (&self.claude_transcript(&s), &s.fork_of) {
                    (Some(_), _) => a.extend(["--resume".into(), s.sid.clone()]),
                    (None, Some(old)) => a.extend(["--resume".into(), old.clone(), "--fork-session".into(), "--session-id".into(), s.sid.clone()]),
                    (None, None) => a.extend(["--session-id".into(), s.sid.clone()]),
                }
                a.extend(["--mcp-config".into(), mcp_file.to_string_lossy().into_owned(), "--strict-mcp-config".into()]);
                // Only ever behind the explicit per-session `full_access` flag (never migrated from `yolo`).
                // The UI (B8) must show a warning when the user turns it on.
                if s.full_access {
                    a.push("--dangerously-skip-permissions".into());
                }
                a.extend(["--append-system-prompt".into(), self.inherited_prefix(&s.project, &s.instructions)]);
                a.extend(first_prompt.clone());
                a
            }
            "pi" => {
                env.extend(self.pi_agent_dir()?);
                let mut a: Vec<String> = vec!["pi".into(), "--session-id".into(), s.sid.clone()];
                a.extend(["--append-system-prompt".into(), self.inherited_prefix(&s.project, &s.instructions)]);
                a.extend(first_prompt.clone());
                a
            }
            "grok" => {
                // ponytail: grok resume, MCP config and prompt flags are unknown here (CLI not installed); opens fresh.
                let home = self.agent_home("grok");
                std::fs::create_dir_all(&home)?;
                env.insert("GROK_HOME".into(), home.to_string_lossy().into_owned());
                vec!["grok".into()]
            }
            other => return Err(ApiError::bad_request(format!("No terminal adapter for {other}"))),
        };
        // Isolation: no PATH entry may lead into the user's own agent homes (plugin bins live there).
        if let Some(path) = env.get_mut("PATH") {
            let shared = [expand_home("~/.claude"), expand_home("~/.grok")];
            *path = path.split(':').filter(|p| !shared.iter().any(|h| p == h || p.starts_with(&format!("{h}/")))).collect::<Vec<_>>().join(":");
        }
        if s.agent == "opencode" {
            self.last_opencode_launch = Some(s.id.clone());
        }
        let s = self.session_mut(cid)?;
        s.launched = true;
        s.prompt = None;
        self.save()?;
        Ok(LaunchSpec { argv, env, cwd, files })
    }

    /// Port of `piAgentDir`: a per-workspace Pi folder whose `models.json`/`settings.json` are generated and
    /// whose other entries link to the user's real Pi folder (login, sessions, packages).
    fn pi_agent_dir(&self) -> Result<BTreeMap<String, String>, ApiError> {
        let dir = self.agent_home("pi");
        let real = std::env::var("PI_CODING_AGENT_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(expand_home("~/.pi/agent")));
        std::fs::create_dir_all(&dir)?;
        for entry in std::fs::read_dir(&real).into_iter().flatten().flatten() {
            let name = entry.file_name();
            if name == "models.json" || name == "settings.json" || dir.join(&name).symlink_metadata().is_ok() {
                continue;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(entry.path(), dir.join(&name))?;
        }
        let providers = effective_providers(&self.ws, self.policy.as_ref());
        let mut env = BTreeMap::from([("PI_CODING_AGENT_DIR".to_string(), dir.to_string_lossy().into_owned())]);
        for p in &providers {
            if let Some(k) = p.key_env.as_deref().filter(|k| !k.is_empty()) {
                if let Some(v) = self.keychain.get(&self.id, k) {
                    env.insert(k.into(), v);
                }
            }
        }
        let read = |f: &str| -> Map<String, Value> { std::fs::read_to_string(real.join(f)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default() };
        let mut own = read("models.json");
        let mut merged = own.get("providers").and_then(Value::as_object).cloned().unwrap_or_default();
        merged.extend(pi_providers(&providers, |k| env.contains_key(k)));
        own.insert("providers".into(), Value::Object(merged));
        write_json(&dir.join("models.json"), &own)?;
        let mut settings = read("settings.json");
        if let Some((provider, id)) = self.ws.default_model.split_once('/') {
            settings.insert("defaultProvider".into(), json!(provider));
            settings.insert("defaultModel".into(), json!(id));
        }
        write_json(&dir.join("settings.json"), &settings)?;
        Ok(env)
    }
}

/// `{env:NAME}` → `${NAME}` inside a JSON text.
fn regex_lite_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("{env:") {
        let tail = &rest[i + 5..];
        match tail.find('}') {
            Some(j) if tail[..j].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                out.push_str(&rest[..i]);
                out.push_str(&format!("${{{}}}", &tail[..j]));
                rest = &tail[j + 1..];
            }
            _ => {
                out.push_str(&rest[..i + 5]);
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Port of `piProviders`: a workspace's providers as Pi's `models.json` providers; keys stay `$NAME` references.
pub fn pi_providers(providers: &[Provider], has_key: impl Fn(&str) -> bool) -> Map<String, Value> {
    let api = |npm: &str| match npm {
        "@ai-sdk/anthropic" => "anthropic-messages",
        "@ai-sdk/openai" => "openai-responses",
        "@ai-sdk/google" => "google-generative-ai",
        _ => "openai-completions",
    };
    let mut out = Map::new();
    for p in providers.iter().filter(|p| p.from_opencode != Some(true)) {
        let key_env = p.key_env.as_deref().unwrap_or("");
        let api_key = if !key_env.is_empty() && has_key(key_env) { format!("${key_env}") } else { "none".into() };
        if NATIVE_PROVIDERS.contains(&p.id.as_str()) {
            if let Some(base) = &p.base_url {
                let mut b = json!({ "baseUrl": base });
                if !key_env.is_empty() {
                    b["apiKey"] = json!(api_key);
                }
                out.insert(p.id.clone(), b);
            }
            continue;
        }
        let Some(base) = &p.base_url else { continue };
        if p.models.is_empty() {
            continue;
        }
        let models: Vec<Value> = p
            .models
            .iter()
            .map(|m| {
                let mut v = json!({ "id": m.id });
                if let Some(n) = m.name.as_deref().filter(|n| !n.is_empty()) {
                    v["name"] = json!(n);
                }
                if let Some(c) = m.context.filter(|c| *c > 0) {
                    v["contextWindow"] = json!(c);
                }
                if let Some(o) = m.output.filter(|o| *o > 0) {
                    v["maxTokens"] = json!(o);
                }
                v
            })
            .collect();
        out.insert(p.id.clone(), json!({ "baseUrl": base, "api": api(p.npm.as_deref().unwrap_or("")), "apiKey": api_key, "models": models }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_and_slug_shapes() {
        let u = uuid4();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert_ne!(uuid4(), u);
        assert_eq!(claude_slug("/home/a/my.proj"), "-home-a-my-proj");
    }

    #[test]
    fn env_refs_become_claude_expansions() {
        assert_eq!(regex_lite_env(r#"{"a":"Bearer {env:EXA_KEY}","b":"{env:bad-x}"}"#), r#"{"a":"Bearer ${EXA_KEY}","b":"{env:bad-x}"}"#);
    }
}
