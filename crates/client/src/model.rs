//! What the native UI shows, as plain data. `apply` folds supervisor events in; the UI only renders this and
//! reacts to the `needs` flags by refetching.

use crate::types::{Chat, Computer, PermissionRequest, Project, Snapshot, UiEvent, WorkspaceSummary};
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
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub workspaces: Vec<WorkspaceSummary>,
    pub computers: Vec<Computer>,
    pub active_ws: Option<String>,
    pub snapshot: Snapshot,
    pub open_chat_id: Option<String>,
    /// Permission requests headless runs are waiting on, per session.
    pub permissions: HashMap<String, Vec<PermissionRequest>>,
    pub status: Connection,
    pub needs: Needs,
    pub toast: Option<String>,
    /// An artifact an agent just made; the window opens it once.
    pub open_artifact: Option<String>,
}

impl Model {
    pub fn open_chat(&mut self, id: String) {
        self.open_chat_id = Some(id);
    }

    pub fn apply(&mut self, event: UiEvent) {
        match event {
            UiEvent::Snapshot { workspace } => {
                if self.active_ws.as_deref() == Some(&workspace) {
                    self.needs.snapshot = true;
                }
            }
            UiEvent::State { .. } => self.needs.state = true,
            UiEvent::Toast { text } => self.toast = Some(text),
            UiEvent::Approval { workspace, chat, request } => {
                if self.active_ws.as_deref() == Some(&workspace) {
                    let list = self.permissions.entry(chat).or_default();
                    list.retain(|p| p.id != request.id);
                    list.push(request);
                }
            }
            UiEvent::Artifact { workspace, artifact } => {
                if self.active_ws.as_deref() == Some(&workspace) {
                    self.needs.snapshot = true;
                    self.open_artifact = Some(artifact);
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
        out.push(SidebarRow { kind: RowKind::Project, id: p.id.clone(), title: p.name.clone(), depth });
        let mut chats: Vec<&Chat> = self.snapshot.chats.iter().filter(|c| c.project == p.id && !c.archived).collect();
        chats.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
        for c in chats {
            out.push(SidebarRow { kind: RowKind::Chat, id: c.id.clone(), title: c.title.clone(), depth: depth + 1 });
        }
        for sub in self.snapshot.projects.iter().filter(|s| !s.archived && s.parent.as_deref() == Some(&p.id)) {
            self.push_project(sub, depth + 1, out);
        }
    }
}
