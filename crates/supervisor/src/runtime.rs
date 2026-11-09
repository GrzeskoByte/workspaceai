//! One `Runtime` per workspace: its own folders, state, policy and audit log (R-06). Port of the
//! persistence, audit, snapshot and facts parts of `runtime.ts`; the engine arrives in batch 3.

use crate::jsonfile::{append_line, read_json, read_text, write_json, write_text};
use crate::keychain::Keychain;
use crate::paths::{expand_home, WsPaths};
use crate::supervisor::{ApiError, Emitter, UiEvent};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::time::Duration;
use workspaceai_shared::defaults::{default_agents, general_project};
use workspaceai_shared::facts::{parse_facts, serialize_facts};
use workspaceai_shared::opencode_config::{build_opencode_json, contains_literal_secret};
use workspaceai_shared::types::{AuditEvent, EngineStatus, Fact, FactStatus, OrgPolicy, PersistedState, Workspace, WorkspaceKind, WorkspaceSnapshot};
use workspaceai_shared::util::{now_ms, project_path_name, uid};
use crate::engine::{ManagedServer, OpencodeClient};

pub struct Runtime {
    pub id: String,
    /// The harness home this workspace belongs to (pinned opencode releases live under it).
    pub home: PathBuf,
    pub paths: WsPaths,
    pub ws: Workspace,
    pub state: PersistedState,
    pub policy: Option<OrgPolicy>,
    pub status: EngineStatus,
    /// The managed `opencode serve` process, when this workspace started one.
    pub engine: Option<ManagedServer>,
    /// A remote server this workspace connected to.
    pub remote: Option<OpencodeClient>,
    pub keychain: Arc<Keychain>,
    pub(crate) last_used: Arc<AtomicI64>,
    events: Emitter,
    /// Sessions the TUI creates after this moment are adopted (R-19); set when the engine connects.
    pub(crate) sync_since: i64,
    pub(crate) sync_at: i64,
    /// The loopback listener (shared with the core); its URL/token go into engine and session envs.
    pub loopback: crate::server::LoopbackSlot,
    /// `session:type` pairs that read `artifact_guide` this run (in memory, as in the TS).
    pub(crate) guided: std::collections::HashSet<String>,
    /// The opencode session whose terminal opened last: artifact attribution for opencode (plan (c)).
    pub(crate) last_opencode_launch: Option<String>,
}

impl Runtime {
    pub(crate) fn new(mut ws: Workspace, home: &Path, events: Emitter, keychain: Arc<Keychain>, loopback: crate::server::LoopbackSlot) -> Result<Self, ApiError> {
        let paths = WsPaths::new(home, &ws.id);
        paths.ensure()?;
        let mut state: PersistedState = read_json(&paths.state).unwrap_or_default();
        if !state.projects.iter().any(|p| p.is_general == Some(true)) {
            let g = general_project(&ws, &home.to_string_lossy());
            if ws.default_project.is_empty() {
                ws.default_project = g.id.clone();
            }
            state.projects.insert(0, g);
        }
        for p in &state.projects {
            std::fs::create_dir_all(expand_home(&p.folder))?;
        }
        let policy = read_json::<Value>(&paths.policy_json).and_then(|v| serde_json::from_value(v.get("policy")?.clone()).ok());
        let status = EngineStatus { state: "stopped".into(), ..Default::default() };
        let last_used = Arc::new(AtomicI64::new(now_ms()));
        let mut rt = Runtime {
            id: ws.id.clone(),
            home: home.to_path_buf(),
            paths,
            ws,
            state,
            policy,
            status,
            engine: None,
            remote: None,
            keychain,
            last_used,
            events,
            sync_since: i64::MAX,
            sync_at: 0,
            loopback,
            guided: Default::default(),
            last_opencode_launch: None,
        };
        rt.migrate_agents()?;
        rt.migrate_sessions()?;
        Ok(rt)
    }

    /// TS `Chat` records become sessions (plan (c)). Writes only when something changed, after keeping the TS
    /// file once as `state.json.bak-tsport` (risk (g): the app reads the live `~/.harness`).
    fn migrate_sessions(&mut self) -> Result<(), ApiError> {
        let mut changed = false;
        for s in &mut self.state.chats {
            changed |= s.migrate();
        }
        if !changed {
            return Ok(());
        }
        let backup = self.paths.state.with_extension("json.bak-tsport");
        if self.paths.state.exists() && !backup.exists() {
            std::fs::copy(&self.paths.state, &backup)?;
        }
        write_json(&self.paths.state, &self.state)?;
        Ok(())
    }

    /// Workspaces created before subscriptions existed get the Grok agent and the current ACP commands.
    fn migrate_agents(&mut self) -> Result<(), ApiError> {
        if self.ws.kind == WorkspaceKind::Org {
            return Ok(());
        }
        let before = self.ws.agents.clone();
        self.ws.agents.retain(|a| a.id != "gemini");
        for d in default_agents(self.ws.kind) {
            let Some(have) = self.ws.agents.iter_mut().find(|a| a.id == d.id) else {
                self.ws.agents.push(d);
                continue;
            };
            if d.command.is_some() && have.command != d.command {
                have.command = d.command.clone();
            }
            if have.subscription.is_none() && d.subscription.is_some() {
                have.subscription = d.subscription;
            }
            if !d.via.is_empty() && have.via != d.via {
                have.via = d.via.clone();
            }
        }
        if self.ws.agents != before {
            self.save()?;
        }
        Ok(())
    }

    // ---------- persistence ----------
    pub fn save(&self) -> Result<(), ApiError> {
        write_json(&self.paths.harness_json, &self.ws)?;
        write_json(&self.paths.state, &self.state)?;
        Ok(())
    }

    pub fn events(&self) -> &Emitter {
        &self.events
    }

    pub fn changed(&self) -> Result<(), ApiError> {
        self.save()?;
        self.events.emit(UiEvent::Snapshot { workspace: self.id.clone() });
        Ok(())
    }

    pub fn audit(&self, action: &str, detail: &str, actor: &str) {
        let ev = AuditEvent { at: now_ms(), actor: actor.into(), action: action.into(), detail: detail.into() };
        if let Err(e) = append_line(&self.paths.audit, &serde_json::to_string(&ev).unwrap()) {
            eprintln!("audit log {}: {e}", self.paths.audit.display());
        }
    }

    pub fn audit_log(&self, limit: usize) -> Vec<AuditEvent> {
        let text = read_text(&self.paths.audit);
        let lines: Vec<&str> = text.trim().split('\n').filter(|l| !l.is_empty()).collect();
        let start = lines.len().saturating_sub(limit);
        lines[start..]
            .iter()
            .rev()
            .map(|l| serde_json::from_str(l).unwrap_or_else(|_| AuditEvent { at: 0, actor: "?".into(), action: l.to_string(), detail: String::new() }))
            .collect()
    }

    /// Uploads new audit entries to the org (policy `auditUrl`), at most every `auditEveryMinutes`. Unless the policy
    /// sets `logContent`, details of content-bearing events (prompts, chat titles) are withheld. Returns entries sent.
    // ponytail: blocking HTTP under the supervisor lock (15 s cap); move to a worker when the scheduler calls it (B11).
    pub fn upload_audit(&self, now: i64, force: bool) -> Result<usize, ApiError> {
        let policy = match &self.policy {
            Some(p) if self.ws.kind == WorkspaceKind::Org => p,
            _ => return Ok(0),
        };
        let Some(url) = policy.audit_url.as_deref() else { return Ok(0) };
        let cursor_file = self.paths.data.join("audit-upload.json");
        let cur: Value = read_json(&cursor_file).unwrap_or(json!({ "sent": 0, "at": 0 }));
        let (sent, at) = (cur["sent"].as_u64().unwrap_or(0) as usize, cur["at"].as_i64().unwrap_or(0));
        if !force && now - at < policy.audit_every_minutes.unwrap_or(15) * 60_000 {
            return Ok(0);
        }
        let text = read_text(&self.paths.audit);
        let lines: Vec<&str> = text.trim().split('\n').filter(|l| !l.is_empty()).collect();
        let fresh: Vec<AuditEvent> = lines.iter().skip(sent).filter_map(|l| serde_json::from_str(l).ok()).collect();
        if fresh.is_empty() {
            write_json(&cursor_file, &json!({ "sent": sent, "at": now }))?;
            return Ok(0);
        }
        let content = |a: &str| ["chat.", "permission.", "artifact.", "fact."].iter().any(|p| a.starts_with(p));
        let events: Vec<AuditEvent> = fresh
            .into_iter()
            .map(|e| if policy.log_content || !content(&e.action) { e } else { AuditEvent { detail: String::new(), ..e } })
            .collect();
        let mut body = json!({ "org": policy.org, "workspace": self.id, "policyVersion": policy.version, "events": events });
        if let Some(org) = &self.ws.org {
            body["subject"] = json!(org.subject);
        }
        let res = ureq::post(url).timeout(Duration::from_secs(15)).set("content-type", "application/json").send_string(&body.to_string());
        match res {
            Ok(_) => {}
            Err(ureq::Error::Status(code, _)) => return Err(ApiError::new(400, format!("Audit upload failed: {code}"))),
            Err(e) => return Err(ApiError::new(400, format!("Audit upload failed: {e}"))),
        }
        write_json(&cursor_file, &json!({ "sent": lines.len(), "at": now }))?;
        Ok(events.len())
    }

    pub fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            workspace: self.ws.clone(),
            projects: self.state.projects.iter().filter(|p| !p.archived).cloned().collect(),
            chats: self.state.chats.iter().filter(|c| !c.archived).cloned().collect(),
            artifacts: self.state.artifacts.clone(),
            facts: self.all_facts(),
            routines: self.state.routines.clone(),
            graphs: self.state.graphs.clone(),
            audit: self.audit_log(60),
            engine: self.engine_status(),
            policy: self.policy.clone(),
            chat_activity: Default::default(),
        }
    }

    // ---------- engine config ----------
    /// `HARNESS_SUPERVISOR` + `HARNESS_SUPERVISOR_TOKEN` while the loopback listener runs; empty otherwise.
    pub fn supervisor_env(&self) -> std::collections::BTreeMap<String, String> {
        crate::server::loopback_info(&self.loopback)
            .map(|l| [("HARNESS_SUPERVISOR".to_string(), l.url), ("HARNESS_SUPERVISOR_TOKEN".to_string(), l.token)].into())
            .unwrap_or_default()
    }

    /// opencode.json for this workspace, with the supervisor's MCP servers (no session: opencode shares one config).
    /// ponytail: no Playwright profile rewrite yet.
    pub fn write_config(&self) -> Result<(), ApiError> {
        let built = build_opencode_json(&self.ws, self.policy.as_ref(), &self.supervisor_mcp(None));
        if contains_literal_secret(&built.json) {
            return Err(ApiError::new(500, "Refusing to write a literal secret into opencode.json"));
        }
        write_json(&self.paths.opencode_json, &built.json)?;
        Ok(())
    }

    // ---------- memory: facts (R-22) ----------
    pub fn facts_file(&self, scope: &str) -> Result<PathBuf, ApiError> {
        if scope == "ws" {
            return Ok(self.paths.memory.join("facts.md"));
        }
        let p = self.project(scope).ok_or_else(|| ApiError::new(400, "Project not found"))?;
        Ok(PathBuf::from(expand_home(&p.folder)).join(".harness/memory/facts.md"))
    }

    pub fn scope_facts(&self, scope: &str) -> Vec<Fact> {
        let Ok(path) = self.facts_file(scope) else { return Vec::new() };
        let text = read_text(&path);
        parse_facts(&text, scope)
    }

    pub fn all_facts(&self) -> Vec<Fact> {
        let mut out = self.scope_facts("ws");
        for p in self.state.projects.iter().filter(|p| !p.archived) {
            out.extend(self.scope_facts(&p.id));
        }
        out
    }

    fn write_scope(&self, scope: &str, facts: &[Fact]) -> Result<(), ApiError> {
        let title = if scope == "ws" {
            format!("{} — workspace memory", self.ws.name)
        } else {
            format!("{} — project memory", self.project(scope).map(|p| p.name.as_str()).unwrap_or("undefined"))
        };
        write_text(&self.facts_file(scope)?, &serialize_facts(facts, &title))?;
        self.write_scopes_manifest()
    }

    /// The memory MCP server reads this to find project scopes without talking to the supervisor.
    pub fn write_scopes_manifest(&self) -> Result<(), ApiError> {
        let mut scopes = vec![json!({ "id": "ws", "name": "Workspace", "file": self.paths.memory.join("facts.md") })];
        for p in self.state.projects.iter().filter(|p| !p.archived) {
            let folder = expand_home(&p.folder);
            scopes.push(json!({
                "id": p.id, "name": project_path_name(&self.state.projects, Some(&p.id)), "folder": folder,
                "parent": p.parent, "file": format!("{folder}/.harness/memory/facts.md"),
            }));
        }
        write_json(&self.paths.memory.join("scopes.json"), &json!({ "workspace": self.ws.name, "scopes": scopes }))?;
        Ok(())
    }

    pub fn add_fact(&self, text: &str, scope: &str, status: FactStatus, source: Option<String>) -> Result<Fact, ApiError> {
        let t = text.trim();
        if t.is_empty() {
            return Err(ApiError::new(400, "Write the fact first"));
        }
        if scope != "ws" && self.project(scope).is_none() {
            return Err(ApiError::new(400, "Project not found"));
        }
        let mut facts = self.scope_facts(scope);
        let f = Fact { id: uid("f"), text: t.chars().take(400).collect(), scope: scope.into(), status, source, created_at: now_ms(), ..Default::default() };
        facts.push(f.clone());
        self.write_scope(scope, &facts)?;
        let short: String = t.chars().take(80).collect();
        self.audit(if status == FactStatus::Suggested { "memory.suggest" } else { "memory.add" }, &short, "you");
        self.changed()?;
        Ok(f)
    }

    /// Every scope in order: workspace first, then projects, so a fact id lookup is stable.
    fn fact_scopes(&self) -> Vec<String> {
        std::iter::once("ws".to_string()).chain(self.state.projects.iter().map(|p| p.id.clone())).collect()
    }

    pub fn update_fact(&self, id: &str, patch: &Value) -> Result<(), ApiError> {
        for scope in self.fact_scopes() {
            let mut facts = self.scope_facts(&scope);
            let Some(f) = facts.iter_mut().find(|x| x.id == id) else { continue };
            if let Some(status) = patch.get("status").and_then(|v| v.as_str()) {
                f.status = match status {
                    "suggested" => FactStatus::Suggested,
                    _ => FactStatus::Approved,
                };
            }
            if let Some(text) = patch.get("text").and_then(|v| v.as_str()) {
                f.text = text.to_string();
            }
            let detail: String = f.text.chars().take(80).collect();
            self.write_scope(&scope, &facts)?;
            self.audit("memory.approve", &detail, "you");
            self.changed()?;
            return Ok(());
        }
        Err(ApiError::bad_request("Fact not found"))
    }

    pub fn remove_fact(&self, id: &str) -> Result<(), ApiError> {
        for scope in self.fact_scopes() {
            let facts = self.scope_facts(&scope);
            if !facts.iter().any(|x| x.id == id) {
                continue;
            }
            let kept: Vec<Fact> = facts.into_iter().filter(|x| x.id != id).collect();
            self.write_scope(&scope, &kept)?;
            self.audit("memory.forget", id, "you");
            self.changed()?;
            return Ok(());
        }
        Err(ApiError::bad_request("Fact not found"))
    }

    pub fn clear_facts(&self) -> Result<(), ApiError> {
        let scopes = std::iter::once("ws".to_string()).chain(self.state.projects.iter().map(|p| p.id.clone()));
        for scope in scopes {
            if !self.scope_facts(&scope).is_empty() {
                self.write_scope(&scope, &[])?;
            }
        }
        self.audit("memory.clear", "all", "you");
        self.changed()
    }
}
