//! What the native UI shows, as plain data. `apply` folds supervisor events in; the UI only renders this and
//! reacts to the `needs` flags by refetching.

use crate::types::{Chat, ChatMessage, Computer, PermissionRequest, Project, Snapshot, ToolStep, UiEvent, WorkspaceSummary};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Connection {
    #[default]
    Starting,
    Connected,
    Disconnected(String),
}

/// What the UI must refetch because an event made the model stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Needs {
    pub state: bool,
    pub snapshot: bool,
    pub messages: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Project,
    Chat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarRow {
    pub kind: RowKind,
    pub id: String,
    pub title: String,
    pub depth: usize,
    /// A chat with a turn in progress.
    pub working: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub workspaces: Vec<WorkspaceSummary>,
    pub computers: Vec<Computer>,
    pub active_ws: Option<String>,
    pub snapshot: Snapshot,
    pub open_chat_id: Option<String>,
    pub messages: Vec<ChatMessage>,
    /// Permission requests an agent is waiting on, per chat.
    pub permissions: HashMap<String, Vec<PermissionRequest>>,
    pub status: Connection,
    pub needs: Needs,
    pub toast: Option<String>,
}

impl Model {
    pub fn open_chat(&mut self, id: String, messages: Vec<ChatMessage>) {
        self.open_chat_id = Some(id);
        self.messages = messages;
        self.needs.messages = false;
    }

    fn is_open(&self, workspace: &str, chat: &str) -> bool {
        self.active_ws.as_deref() == Some(workspace) && self.open_chat_id.as_deref() == Some(chat)
    }

    fn assistant_message(&mut self, id: &str) -> &mut ChatMessage {
        if let Some(i) = self.messages.iter().position(|m| m.id == id) {
            return &mut self.messages[i];
        }
        self.messages.push(ChatMessage { id: id.into(), role: "assistant".into(), text: String::new(), tools: vec![], error: None, created_at: 0 });
        self.messages.last_mut().unwrap()
    }

    pub fn apply(&mut self, event: UiEvent) {
        match event {
            UiEvent::ChatDelta { workspace, chat, message_id, delta } => {
                if self.active_ws.as_deref() == Some(&workspace) {
                    self.snapshot.chat_activity.insert(chat.clone(), "working".into());
                }
                if self.is_open(&workspace, &chat) {
                    self.assistant_message(&message_id).text.push_str(&delta);
                }
            }
            UiEvent::ChatTool { workspace, chat, message_id, tool } => {
                if self.is_open(&workspace, &chat) {
                    let tools: &mut Vec<ToolStep> = &mut self.assistant_message(&message_id).tools;
                    match tools.iter_mut().find(|t| t.name == tool.name) {
                        Some(existing) => *existing = tool,
                        None => tools.push(tool),
                    }
                }
            }
            UiEvent::ChatDone { workspace, chat, .. } => {
                if self.active_ws.as_deref() == Some(&workspace) {
                    self.snapshot.chat_activity.remove(&chat);
                    self.permissions.remove(&chat);
                    self.needs.snapshot = true;
                    if self.is_open(&workspace, &chat) {
                        self.needs.messages = true;
                    }
                }
            }
            UiEvent::Snapshot { workspace } => {
                if self.active_ws.as_deref() == Some(&workspace) {
                    self.needs.snapshot = true;
                }
            }
            UiEvent::State { .. } => self.needs.state = true,
            UiEvent::Toast { text } => self.toast = Some(text),
            UiEvent::ChatPermission { workspace, chat, request } => {
                if self.active_ws.as_deref() == Some(&workspace) {
                    let list = self.permissions.entry(chat).or_default();
                    list.retain(|p| p.id != request.id);
                    list.push(request);
                }
            }
            UiEvent::ChatPermissionClear { workspace, chat, request_id } => {
                if self.active_ws.as_deref() == Some(&workspace) {
                    match request_id {
                        Some(id) => self.drop_permission(&chat, &id),
                        None => {
                            self.permissions.remove(&chat);
                        }
                    }
                }
            }
            UiEvent::Other => {}
        }
    }

    /// Requests the open chat's agent is waiting on.
    pub fn pending_permissions(&self) -> &[PermissionRequest] {
        self.open_chat_id.as_ref().and_then(|c| self.permissions.get(c)).map_or(&[], |v| v.as_slice())
    }

    pub fn drop_permission(&mut self, chat: &str, id: &str) {
        if let Some(list) = self.permissions.get_mut(chat) {
            list.retain(|p| p.id != id);
        }
    }

    /// Replaces what the engine reports as pending (it survives restarts and missed events).
    pub fn set_permissions(&mut self, chat: &str, list: Vec<PermissionRequest>) {
        self.permissions.insert(chat.to_string(), list);
    }

    pub fn open_chat_working(&self) -> bool {
        self.open_chat_id.as_ref().is_some_and(|c| self.snapshot.chat_activity.contains_key(c))
    }

    /// Projects as a tree (General first), each followed by its chats newest first, then its subprojects.
    /// Archived projects and chats are hidden.
    pub fn sidebar(&self) -> Vec<SidebarRow> {
        let mut out = Vec::new();
        let mut roots: Vec<&Project> = self.snapshot.projects.iter().filter(|p| !p.archived && p.parent.is_none()).collect();
        roots.sort_by_key(|p| p.is_general != Some(true)); // stable: General first, the rest keep their order
        for p in roots {
            self.push_project(p, 0, &mut out);
        }
        out
    }

    /// Archived chats of non-archived projects, newest first: (id, title).
    pub fn archived_chats(&self) -> Vec<(String, String)> {
        let live = |id: &str| self.snapshot.projects.iter().any(|p| p.id == id && !p.archived);
        let mut v: Vec<&Chat> = self.snapshot.chats.iter().filter(|c| c.archived && live(&c.project)).collect();
        v.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
        v.into_iter().map(|c| (c.id.clone(), c.title.clone())).collect()
    }

    /// Ctrl/Cmd+1..9 → the Nth workspace's id (`None` for other keys or no such workspace).
    pub fn workspace_for_shortcut(&self, key: &str) -> Option<&str> {
        let n = key.parse::<usize>().ok().filter(|n| (1..=9).contains(n))?;
        self.workspaces.get(n - 1).map(|w| w.id.as_str())
    }

    fn push_project(&self, p: &Project, depth: usize, out: &mut Vec<SidebarRow>) {
        out.push(SidebarRow { kind: RowKind::Project, id: p.id.clone(), title: p.name.clone(), depth, working: false });
        let mut chats: Vec<&Chat> = self.snapshot.chats.iter().filter(|c| c.project == p.id && !c.archived).collect();
        chats.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
        for c in chats {
            let working = self.snapshot.chat_activity.contains_key(&c.id);
            out.push(SidebarRow { kind: RowKind::Chat, id: c.id.clone(), title: c.title.clone(), depth: depth + 1, working });
        }
        for sub in self.snapshot.projects.iter().filter(|s| !s.archived && s.parent.as_deref() == Some(&p.id)) {
            self.push_project(sub, depth + 1, out);
        }
    }
}
