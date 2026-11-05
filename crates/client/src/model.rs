//! What the native UI shows, as plain data. `apply` folds supervisor events in; the UI only renders this and
//! reacts to the `needs` flags by refetching.

use crate::types::{Chat, ChatMessage, Project, Snapshot, ToolStep, UiEvent, WorkspaceSummary};

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
    pub active_ws: Option<String>,
    pub snapshot: Snapshot,
    pub open_chat_id: Option<String>,
    pub messages: Vec<ChatMessage>,
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
            UiEvent::ChatPermission { .. } => {
                self.toast = Some("An agent is waiting for permission. Approvals are not in the native app yet.".into());
            }
            UiEvent::Other => {}
        }
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
