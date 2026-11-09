//! Session records (R-10, R-16..R-19): create, rename, archive, fork, move, delete, import, two-way sync and
//! the read-only transcript. Port of the chat part of `runtime.ts` minus send/stop/permissions (terminals).

use crate::agents::{uuid4, AGENTS};
use crate::engine::OpencodeClient;
use crate::paths::{expand_home, valid_id};
use crate::recall::strip_recall;
use crate::runtime::Runtime;
use crate::supervisor::{ApiError, UiEvent};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use workspaceai_shared::types::{EngineMode, Project, Session};
use workspaceai_shared::util::{nest_folders, now_ms, project_path_name, title_case_segment, uid};

fn oc_err(e: crate::engine::client::OpencodeError) -> ApiError {
    ApiError::bad_request(e.message)
}

/// Node's `resolve(expandHome(p))`: absolute, `.`/`..` folded, no trailing slash.
pub fn resolve(p: &str) -> String {
    let e = PathBuf::from(expand_home(p));
    let abs = if e.is_absolute() { e } else { std::env::current_dir().unwrap_or_default().join(e) };
    let mut out = PathBuf::from("/");
    for c in abs.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(x) => out.push(x),
            _ => {}
        }
    }
    out.to_string_lossy().into_owned()
}

/// What `POST /api/ws/:id/chats` takes.
#[derive(Debug, Default, Clone)]
pub struct NewSession {
    pub project: Option<String>,
    pub title: String,
    pub agent: Option<String>,
    pub instructions: String,
    pub prompt: Option<String>,
}

impl Runtime {
    pub fn session(&self, id: &str) -> Option<&Session> {
        self.state.chats.iter().find(|c| c.id == id)
    }

    pub fn session_mut(&mut self, id: &str) -> Result<&mut Session, ApiError> {
        self.state.chats.iter_mut().find(|c| c.id == id).ok_or_else(|| ApiError::not_found("Session not found"))
    }

    pub fn session_by_sid(&self, sid: &str) -> Option<&Session> {
        self.state.chats.iter().find(|c| c.sid == sid)
    }

    fn agent_on(&self, id: &str) -> bool {
        id == "opencode" || self.ws.agents.iter().any(|a| a.id == id && a.on)
    }

    pub fn create_session(&mut self, o: NewSession) -> Result<Session, ApiError> {
        let pid = match o.project.as_deref() {
            Some(p) if self.project(p).is_some() => p.to_string(),
            _ if self.project(&self.ws.default_project).is_some() => self.ws.default_project.clone(),
            _ => self.general().map(|g| g.id.clone()).ok_or_else(|| ApiError::bad_request("Project not found"))?,
        };
        if let Some(a) = o.agent.as_deref().filter(|a| !self.agent_on(a)) {
            let name = self.ws.agents.iter().find(|x| x.id == a).map(|x| x.name.clone()).unwrap_or_else(|| a.to_string());
            return Err(ApiError::bad_request(format!("{name} is off in this workspace — connect it in Settings › Model providers")));
        }
        let agent = o.agent.clone().unwrap_or_else(|| if self.agent_on(&self.ws.default_agent) { self.ws.default_agent.clone() } else { "opencode".into() });
        if !AGENTS.contains(&agent.as_str()) {
            return Err(ApiError::bad_request(format!("No terminal adapter for {agent}")));
        }
        let title: String = if o.title.trim().is_empty() { "New session".into() } else { o.title.chars().take(80).collect() };
        let folder = self.project_folder(&pid);
        // The id is fixed before the agent runs whenever it allows it, so a crash cannot orphan a session.
        let (sid, launched) = match agent.as_str() {
            "opencode" => {
                let c = self.engine_client()?;
                let s = c.create_session(&title, &folder, &self.effective_model(&pid)).map_err(oc_err)?;
                let sid = s["id"].as_str().unwrap_or_default().to_string();
                self.push_instructions(&c, &sid, &pid, &o.instructions);
                (sid, true)
            }
            "grok" => (String::new(), false),
            _ => (uuid4(), false),
        };
        let now = now_ms();
        let s = Session {
            id: uid("chat"),
            project: pid.clone(),
            title,
            sid,
            agent,
            instructions: o.instructions,
            launched,
            prompt: o.prompt.filter(|p| !p.trim().is_empty()),
            created_at: now,
            updated_at: now,
            ..Default::default()
        };
        self.state.chats.push(s.clone());
        self.audit("chat.create", &format!("{} · {} · {}", s.title, project_path_name(&self.state.projects, Some(&pid)), s.sid), "you");
        self.changed()?;
        Ok(s)
    }

    /// Inherited layers go into the opencode session's instruction entry (R-12); failures are audited, not fatal.
    pub(crate) fn push_instructions(&self, c: &OpencodeClient, sid: &str, pid: &str, instructions: &str) {
        if let Err(e) = c.set_instructions(sid, &self.inherited_prefix(pid, instructions)) {
            self.audit("instructions.error", &e.message.chars().take(200).collect::<String>(), "workspaceai");
        }
    }

    /// After an AGENTS.md or instructions change: running opencode sessions get the new prefix now; the other
    /// agents read it on their next launch.
    pub fn refresh_instructions(&mut self) {
        if !self.engine_running() && self.remote.is_none() {
            return;
        }
        let Ok(c) = self.engine_client() else { return };
        for s in self.state.chats.iter().filter(|s| s.agent == "opencode" && !s.archived) {
            self.push_instructions(&c, &s.sid, &s.project, &s.instructions);
        }
    }

    pub fn rename_session(&mut self, cid: &str, title: &str) -> Result<(), ApiError> {
        let s = self.session(cid).cloned().ok_or_else(|| ApiError::not_found("Session not found"))?;
        let t = if title.trim().is_empty() { s.title.clone() } else { title.trim().to_string() };
        if s.agent == "opencode" {
            self.engine_client()?.patch_session(&s.sid, &json!({ "title": t })).map_err(oc_err)?;
        }
        self.session_mut(cid)?.title = t.clone();
        self.audit("chat.rename", &t, "you");
        Ok(())
    }

    /// `PATCH /api/ws/:id/chats/:cid`: title, archived, fullAccess (`yolo` accepted), instructions.
    pub fn patch_session(&mut self, cid: &str, body: &Value) -> Result<Session, ApiError> {
        self.session(cid).ok_or_else(|| ApiError::not_found("Session not found"))?;
        if let Some(t) = body["title"].as_str() {
            self.rename_session(cid, t)?;
        }
        if let Some(a) = body["archived"].as_bool() {
            let s = self.session_mut(cid)?;
            s.archived = a;
            let (agent, sid) = (s.agent.clone(), s.sid.clone());
            if agent == "opencode" && (self.engine_running() || self.remote.is_some()) {
                let _ = self.engine_client().map(|c| c.patch_session(&sid, &json!({ "archived": a })));
            }
            self.audit(if a { "chat.archive" } else { "chat.restore" }, cid, "you");
        }
        if let Some(f) = body["fullAccess"].as_bool().or(body["yolo"].as_bool()) {
            let s = self.session_mut(cid)?;
            s.full_access = f;
            s.updated_at = now_ms();
            let title = s.title.clone();
            self.audit("chat.yolo", &format!("{title} · {}", if f { "full permissions" } else { "ask first" }), "you");
        }
        if let Some(i) = body["instructions"].as_str() {
            let s = self.session_mut(cid)?;
            s.instructions = i.to_string();
            let s = s.clone();
            if s.agent == "opencode" && (self.engine_running() || self.remote.is_some()) {
                if let Ok(c) = self.engine_client() {
                    self.push_instructions(&c, &s.sid, &s.project, &s.instructions);
                }
            }
        }
        self.changed()?;
        Ok(self.session(cid).cloned().expect("checked"))
    }

    pub fn fork_session(&mut self, cid: &str) -> Result<Session, ApiError> {
        let s = self.session(cid).cloned().ok_or_else(|| ApiError::not_found("Session not found"))?;
        let title = format!("{} (fork)", s.title);
        let (sid, launched, fork_of) = match s.agent.as_str() {
            "opencode" => {
                let c = self.engine_client()?;
                let sid = match c.fork_session(&s.sid) {
                    Ok(v) => v["id"].as_str().unwrap_or_default().to_string(),
                    // opencode refuses to fork an empty session; a fork of nothing is a fresh session in the project.
                    Err(e) if e.message.to_lowercase().contains("empty_session") || e.message.to_lowercase().contains("empty session") => {
                        let folder = self.project_folder(&s.project);
                        c.create_session(&title, &folder, &self.effective_model(&s.project)).map_err(oc_err)?["id"].as_str().unwrap_or_default().to_string()
                    }
                    Err(e) => return Err(oc_err(e)),
                };
                (sid, true, None)
            }
            // A session claude never wrote is forked as a fresh one.
            "claude" => (uuid4(), false, self.claude_transcript(&s).map(|_| s.sid.clone())),
            other => return Err(ApiError::bad_request(format!("Forking is not available for {other} sessions"))),
        };
        let now = now_ms();
        let fork = Session { id: uid("chat"), sid, title, imported: false, launched, fork_of, prompt: None, created_at: now, updated_at: now, ..s.clone() };
        self.state.chats.push(fork.clone());
        self.audit("chat.fork", &format!("{} → {}", s.sid, fork.sid), "you");
        self.changed()?;
        Ok(fork)
    }

    /// Removes the record. opencode also deletes the server session; claude removes its transcript only when
    /// `remove_history` (the UI's second confirm step).
    pub fn delete_session(&mut self, cid: &str, remove_history: bool) -> Result<(), ApiError> {
        let s = self.session(cid).cloned().ok_or_else(|| ApiError::not_found("Session not found"))?;
        if s.agent == "opencode" {
            if let Ok(c) = self.engine_client() {
                let _ = c.delete_session(&s.sid);
            }
        }
        if s.agent == "claude" && remove_history {
            if let Some(f) = self.claude_transcript(&s) {
                std::fs::remove_file(f)?;
            }
        }
        if valid_id(&s.sid) {
            let _ = std::fs::remove_file(self.paths.run.join(format!("{}.mcp.json", s.sid)));
        }
        self.state.chats.retain(|c| c.id != cid);
        self.audit("chat.delete", &s.title, "you");
        self.changed()
    }

    /// Moving a session moves the artifacts it made (R-13). A session can be moved but never detached (R-10).
    pub fn move_session(&mut self, cid: &str, project: &str) -> Result<usize, ApiError> {
        self.session(cid).ok_or_else(|| ApiError::not_found("Session not found"))?;
        if self.project(project).is_none() {
            return Err(ApiError::bad_request("Pick a project — a chat always belongs to one"));
        }
        let s = self.session_mut(cid)?;
        let from = std::mem::replace(&mut s.project, project.to_string());
        let s = s.clone();
        let mut moved = Vec::new();
        for a in self.state.artifacts.iter_mut().filter(|a| a.chat.as_deref() == Some(cid) && a.project == from) {
            a.project = project.to_string();
            moved.push(a.id.clone());
        }
        self.relocate_artifacts(&moved, &from)?;
        let n = moved.len();
        self.audit("chat.move", &format!("{} → {} ({n} artifacts)", s.title, project_path_name(&self.state.projects, Some(project))), "you");
        self.changed()?;
        if s.agent == "opencode" && (self.engine_running() || self.remote.is_some()) {
            if let Ok(c) = self.engine_client() {
                self.push_instructions(&c, &s.sid, project, &s.instructions);
            }
        }
        Ok(n)
    }

    /// `attachCommand`: the shell line that opens the session in the opencode TUI.
    pub fn attach_command(&self, cid: Option<&str>) -> Result<String, ApiError> {
        let s = match cid {
            Some(c) => Some(self.session(c).ok_or_else(|| ApiError::not_found("Session not found"))?),
            None => None,
        };
        let url = if self.ws.engine.mode == EngineMode::Remote {
            self.ws.engine.url.clone()
        } else {
            Some(self.engine_status().url).filter(|u| !u.is_empty()).unwrap_or_else(|| "http://127.0.0.1:<port>".into())
        };
        let pw = match (&self.ws.engine.mode, &self.engine) {
            (EngineMode::Managed, Some(e)) => format!("OPENCODE_SERVER_PASSWORD={} ", shell_quote(&e.password)),
            (EngineMode::Managed, None) => "OPENCODE_SERVER_PASSWORD='' ".into(),
            _ => String::new(),
        };
        let mut out = format!("{pw}opencode --server {}", shell_quote(&url));
        if let Some(s) = s {
            out.push_str(&format!(" --session {} {}", shell_quote(&s.sid), shell_quote(&self.project_folder(&s.project))));
        }
        Ok(out)
    }

    // ---------- import and sync (R-18, R-19) ----------

    /// The project for a folder, created (nested under the deepest containing project) when none matches.
    pub fn project_for_folder(&mut self, dir: &str) -> String {
        let abs = resolve(dir);
        let mut hits: Vec<&Project> = self
            .state
            .projects
            .iter()
            .filter(|p| !p.archived && { let f = resolve(&p.folder); abs == f || abs.starts_with(&format!("{f}/")) })
            .collect();
        hits.sort_by_key(|p| std::cmp::Reverse(p.folder.len()));
        if let Some(h) = hits.first().filter(|h| resolve(&h.folder) == abs) {
            return h.id.clone();
        }
        let parent = hits.first().filter(|h| h.is_general != Some(true)).map(|h| h.id.clone());
        let p = Project { id: uid("proj"), name: title_case_segment(&abs), parent, folder: abs, inherit: true, created_at: now_ms(), ..Default::default() };
        let id = p.id.clone();
        self.state.projects.push(p);
        id
    }

    /// `chatFromSession`: a record for an opencode session.
    fn adopt_oc(&mut self, s: &Value, imported: bool) -> Session {
        let dir = s["location"]["directory"].as_str().map(str::to_string).unwrap_or_else(|| self.project_folder(""));
        let project = self.project_for_folder(&dir);
        let id = s["id"].as_str().unwrap_or_default().to_string();
        let title = s["title"].as_str().filter(|t| !t.is_empty()).map(str::to_string).unwrap_or_else(|| format!("Session {}", &id[id.len().saturating_sub(6)..]));
        let now = now_ms();
        let session = Session {
            id: uid("chat"),
            project,
            title,
            sid: id,
            agent: "opencode".into(),
            imported,
            launched: true,
            created_at: s["time"]["created"].as_i64().unwrap_or(now),
            updated_at: s["time"]["updated"].as_i64().unwrap_or(now),
            ..Default::default()
        };
        self.state.chats.push(session.clone());
        session
    }

    pub fn scan_sessions(&mut self) -> Result<Vec<Value>, ApiError> {
        let all = self.engine_client()?.list_sessions(None, 10_000).map_err(oc_err)?;
        Ok(all.into_iter().filter(|s| s["parentID"].is_null() && s["time"]["archived"].is_null() && self.session_by_sid(s["id"].as_str().unwrap_or("")).is_none()).collect())
    }

    /// Import sessions (from this workspace's server, or copied from another): folders become nested projects.
    pub fn import_sessions(&mut self, sessions: &[Value]) -> Result<Value, ApiError> {
        let before = self.state.projects.len();
        let dirs: Vec<&str> = sessions.iter().filter_map(|s| s["location"]["directory"].as_str()).filter(|d| !d.is_empty()).collect();
        for (dir, _) in nest_folders(&dirs) {
            self.project_for_folder(&dir);
        }
        let mut n = 0;
        for s in sessions {
            if self.session_by_sid(s["id"].as_str().unwrap_or("")).is_none() {
                self.adopt_oc(s, true);
                n += 1;
            }
        }
        self.audit("sessions.import", &format!("{n} chats"), "you");
        self.changed()?;
        Ok(json!({ "chats": n, "projects": self.state.projects.len() - before }))
    }

    /// `copySessions`: export from another server, import into this workspace's own, with history.
    pub fn copy_sessions(&mut self, source: &OpencodeClient, ids: &[String]) -> Result<Value, ApiError> {
        let target = self.engine_client()?;
        let (mut imported, mut failed) = (Vec::new(), Vec::new());
        for id in ids {
            let r = source.export_session(id).and_then(|exp| {
                let payload = if exp.get("data").is_some_and(|d| !d.is_null()) { &exp["data"] } else { &exp };
                target.import_session(&payload["info"], &payload["messages"], payload["info"]["location"]["directory"].as_str())
            });
            match r {
                Ok(s) => imported.push(s),
                Err(_) => failed.push(id.clone()),
            }
        }
        let mut res = self.import_sessions(&imported)?;
        res["failed"] = json!(failed);
        Ok(res)
    }

    /// Two-way sync (R-19): a session started in the TUI after the engine connected gets a record in the
    /// project of its folder. ponytail: polled from the snapshot route at most every 2 s instead of the
    /// opencode event stream (the core is not shared with a thread yet); an SSE thread can replace it in B8.
    pub fn sync_sessions(&mut self) {
        let now = now_ms();
        if !self.ws.engine.sync || now - self.sync_at < 2000 || !(self.engine_running() || self.remote.is_some()) {
            return;
        }
        self.sync_at = now;
        let Ok(c) = self.engine_client() else { return };
        let Ok(all) = c.list_sessions(None, 10_000) else { return };
        let fresh: Vec<Value> = all
            .into_iter()
            .filter(|s| {
                let sid = s["id"].as_str().unwrap_or("");
                s["parentID"].is_null()
                    && s["time"]["created"].as_i64().unwrap_or(0) >= self.sync_since
                    && self.session_by_sid(sid).is_none()
                    && !self.state.skipped_sessions.iter().any(|x| x == sid)
            })
            .collect();
        if fresh.is_empty() {
            return;
        }
        for s in &fresh {
            let session = self.adopt_oc(s, false);
            self.events().emit(UiEvent::Toast { text: format!("New session from the TUI: {}", session.title) });
        }
        let _ = self.changed();
    }

    /// Claude sessions under `<source>/projects/*/*.jsonl` (the user's own `~/.claude`), not yet in this workspace.
    pub fn scan_claude_sessions(&self, source: &Path) -> Vec<Value> {
        let mut out = Vec::new();
        for dir in std::fs::read_dir(source.join("projects")).into_iter().flatten().flatten() {
            for f in std::fs::read_dir(dir.path()).into_iter().flatten().flatten() {
                let p = f.path();
                let Some(id) = p.file_stem().and_then(|s| s.to_str()).filter(|_| p.extension().is_some_and(|e| e == "jsonl")) else { continue };
                if !valid_id(id) || self.session_by_sid(id).is_some() || !f.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                let (title, cwd) = claude_head(&p);
                let updated = f.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0);
                out.push(json!({ "id": id, "title": title, "directory": cwd, "updatedAt": updated }));
            }
        }
        out
    }

    /// Copies the chosen claude transcripts into this workspace's `CLAUDE_CONFIG_DIR` (never points at the shared
    /// home) and records them as resumable sessions.
    pub fn import_claude_sessions(&mut self, source: &Path, ids: &[String]) -> Result<Value, ApiError> {
        let before = self.state.projects.len();
        let found = self.scan_claude_sessions(source);
        let projects = self.agent_home("claude").join("projects");
        let mut n = 0;
        for s in found.iter().filter(|s| ids.iter().any(|i| s["id"] == i.as_str())) {
            let id = s["id"].as_str().unwrap_or_default().to_string();
            let cwd = s["directory"].as_str().unwrap_or_default().to_string();
            let src_dir = std::fs::read_dir(source.join("projects"))?.flatten().map(|d| d.path()).find(|d| d.join(format!("{id}.jsonl")).is_file());
            let Some(src_dir) = src_dir else { continue };
            let slug = if cwd.is_empty() { src_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { crate::agents::claude_slug(&cwd) };
            let dest = projects.join(&slug).join(format!("{id}.jsonl"));
            std::fs::create_dir_all(dest.parent().expect("has parent"))?;
            if !dest.exists() {
                std::fs::copy(src_dir.join(format!("{id}.jsonl")), &dest)?;
            }
            let project = if cwd.is_empty() { self.general().map(|g| g.id.clone()).unwrap_or_default() } else { self.project_for_folder(&cwd) };
            let now = now_ms();
            self.state.chats.push(Session {
                id: uid("chat"),
                project,
                title: s["title"].as_str().unwrap_or("Claude session").to_string(),
                sid: id,
                agent: "claude".into(),
                imported: true,
                launched: true,
                created_at: now,
                updated_at: s["updatedAt"].as_i64().unwrap_or(now),
                ..Default::default()
            });
            n += 1;
        }
        self.audit("sessions.import", &format!("{n} claude sessions"), "you");
        self.changed()?;
        Ok(json!({ "chats": n, "projects": self.state.projects.len() - before }))
    }

    // ---------- transcript (read-only; Library search and "Summarize into memory") ----------
    pub fn session_messages(&mut self, cid: &str) -> Result<Vec<Value>, ApiError> {
        let s = self.session(cid).cloned().ok_or_else(|| ApiError::not_found("Session not found"))?;
        match s.agent.as_str() {
            "opencode" => {
                let raw = self.engine_client()?.messages(&s.sid).map_err(oc_err)?;
                Ok(map_oc_messages(&raw)
                    .into_iter()
                    .filter(|m| !(m["role"] == "user" && m["text"].as_str().is_some_and(|t| t.starts_with("WorkspaceAI instructions, broad to narrow") || t.starts_with("Harness instructions, broad to narrow"))))
                    .map(|mut m| {
                        if m["role"] == "user" {
                            let t = strip_recall(m["text"].as_str().unwrap_or(""));
                            m["text"] = json!(t);
                        }
                        m
                    })
                    .collect())
            }
            "claude" => Ok(self.claude_transcript(&s).map(|f| claude_messages(&std::fs::read_to_string(f).unwrap_or_default())).unwrap_or_default()),
            _ => Ok(Vec::new()),
        }
    }
}

/// Plain-text `mapOcMessages`: user and assistant turns, consecutive assistant steps merged.
pub fn map_oc_messages(raw: &[Value]) -> Vec<Value> {
    let mut chrono: Vec<&Value> = raw.iter().collect();
    chrono.sort_by_key(|m| m["time"]["created"].as_i64().unwrap_or(0));
    let mut out: Vec<Value> = Vec::new();
    for m in chrono {
        let created = m["time"]["created"].as_i64().unwrap_or_else(now_ms);
        let id = m["id"].as_str().unwrap_or_default();
        match m["type"].as_str() {
            Some("user") => {
                let text = m["text"].as_str().or(m["payload"]["text"].as_str()).unwrap_or_default();
                out.push(json!({ "id": id, "role": "user", "text": text, "createdAt": created }));
            }
            Some("assistant") => {
                let text: Vec<&str> = m["content"].as_array().into_iter().flatten().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect();
                let text = text.join("\n\n");
                match out.last_mut() {
                    Some(prev) if prev["role"] == "assistant" && prev.get("error").is_none() => {
                        let joined = [prev["text"].as_str().unwrap_or(""), &text].iter().filter(|t| !t.is_empty()).copied().collect::<Vec<_>>().join("\n\n");
                        prev["text"] = json!(joined);
                    }
                    _ => {
                        let mut v = json!({ "id": id, "role": "assistant", "text": text, "createdAt": created });
                        if !m["error"].is_null() {
                            v["error"] = json!(m["error"]["message"].as_str().map(str::to_string).unwrap_or_else(|| m["error"].to_string()));
                        }
                        out.push(v);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn claude_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n\n"),
        _ => String::new(),
    }
}

/// ISO-8601 UTC (`2026-11-09T10:00:00.123Z`) → epoch ms; 0 when unreadable.
fn iso_ms(s: &str) -> i64 {
    chrono_like(s).unwrap_or(0)
}

fn chrono_like(s: &str) -> Option<i64> {
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, sec) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    let ms = if s.as_bytes().get(19) == Some(&b'.') { n(20, 23).unwrap_or(0) } else { 0 };
    // days_from_civil (Howard Hinnant)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + h) * 60 + mi) * 60_000 + sec * 1000 + ms)
}

/// A claude `.jsonl` transcript as plain-text turns. Tool results (user turns without text) are skipped.
pub fn claude_messages(jsonl: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for line in jsonl.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let role = match v["type"].as_str() {
            Some(r @ ("user" | "assistant")) => r,
            _ => continue,
        };
        if v["isMeta"] == json!(true) {
            continue;
        }
        let text = claude_text(&v["message"]["content"]);
        if text.trim().is_empty() {
            continue;
        }
        out.push(json!({ "id": v["uuid"].as_str().unwrap_or_default(), "role": role, "text": text, "createdAt": iso_ms(v["timestamp"].as_str().unwrap_or("")) }));
    }
    out
}

/// Title (first user text, 60 chars) and cwd of a claude transcript.
fn claude_head(path: &Path) -> (String, String) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut cwd = String::new();
    let mut title = String::new();
    for line in text.lines().take(200) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if cwd.is_empty() {
            cwd = v["cwd"].as_str().unwrap_or_default().to_string();
        }
        if title.is_empty() && v["type"] == "user" && v["isMeta"] != json!(true) {
            let t = claude_text(&v["message"]["content"]);
            if !t.trim().is_empty() && !t.starts_with('<') {
                title = t.trim().chars().take(60).collect();
            }
        }
        if !cwd.is_empty() && !title.is_empty() {
            break;
        }
    }
    (if title.is_empty() { "Claude session".into() } else { title }, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_timestamps() {
        assert_eq!(iso_ms("1970-01-01T00:00:00.000Z"), 0);
        assert_eq!(iso_ms("2026-11-09T10:00:00.123Z"), 1_794_218_400_123);
        assert_eq!(iso_ms("nope"), 0);
    }

    #[test]
    fn resolve_folds_dots() {
        assert_eq!(resolve("/a/b/../c/./d/"), "/a/c/d");
    }

    #[test]
    fn claude_jsonl_turns() {
        let j = r#"{"type":"user","cwd":"/x","uuid":"u1","timestamp":"1970-01-01T00:00:01.000Z","message":{"role":"user","content":"hello"}}
{"type":"assistant","uuid":"a1","message":{"content":[{"type":"text","text":"hi"},{"type":"tool_use","name":"Bash"}]}}
{"type":"user","message":{"content":[{"type":"tool_result","content":"x"}]}}
{"type":"summary","summary":"s"}"#;
        let m = claude_messages(j);
        assert_eq!(m.len(), 2);
        assert_eq!((m[0]["text"].as_str(), m[0]["createdAt"].as_i64()), (Some("hello"), Some(1000)));
        assert_eq!(m[1]["text"], "hi");
    }
}

/// POSIX single-quote escaping for text that ends up in a line the user pastes into a shell. Plain words stay unquoted.
fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod shell_quote_tests {
    use super::shell_quote;
    #[test]
    fn quotes_what_a_shell_would_interpret() {
        assert_eq!(shell_quote("ses_abc123"), "ses_abc123");
        assert_eq!(shell_quote("/home/me/web"), "/home/me/web");
        assert_eq!(shell_quote("ses_x; rm -rf ~"), "'ses_x; rm -rf ~'");
        assert_eq!(shell_quote("it's $(id)"), "'it'\\''s $(id)'");
        assert_eq!(shell_quote(""), "''");
    }
}
