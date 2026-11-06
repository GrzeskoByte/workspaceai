//! Chat management: request bodies for `PATCH /api/ws/:id/chats/:cid` and the two-step delete.

use serde_json::{json, Value};

const MAX_TITLE: usize = 120;

/// `{"title": …}` with the title trimmed and capped at 120 characters; `None` for a blank one.
pub fn rename_body(title: &str) -> Option<Value> {
    let t = title.trim();
    (!t.is_empty()).then(|| json!({ "title": t.chars().take(MAX_TITLE).collect::<String>() }))
}

pub fn archive_body(archived: bool) -> Value {
    json!({ "archived": archived })
}

/// `POST` target and body for moving a chat (with its artifacts) to another project.
pub fn move_path(workspace: &str, chat: &str) -> String {
    format!("/api/ws/{}/chats/{}/move", crate::api::seg(workspace), crate::api::seg(chat))
}

pub fn move_body(project: &str) -> Value {
    json!({ "project": project })
}

/// Full permissions ("YOLO"): tool calls run without asking in this chat only.
pub fn full_access_body(on: bool) -> Value {
    json!({ "yolo": on })
}

/// A destructive button that needs a second press on the same chat.
#[derive(Debug, Default)]
pub struct Confirm {
    armed: Option<String>,
}

impl Confirm {
    /// `true` when this press confirms; otherwise it arms (or re-arms) the chat and returns `false`.
    pub fn press(&mut self, chat: &str) -> bool {
        if self.armed.as_deref() == Some(chat) {
            self.armed = None;
            true
        } else {
            self.armed = Some(chat.to_string());
            false
        }
    }

    pub fn is_armed(&self, chat: &str) -> bool {
        self.armed.as_deref() == Some(chat)
    }

    pub fn cancel(&mut self) {
        self.armed = None;
    }
}

/// Chats with a change waiting on the server, so a quick second click cannot reorder or double-send it.
#[derive(Debug, Default)]
pub struct InFlight {
    chats: Vec<String>,
}

impl InFlight {
    /// `true` when the change may start; `false` when one is already waiting for this chat.
    pub fn begin(&mut self, chat: &str) -> bool {
        if self.chats.iter().any(|c| c == chat) {
            return false;
        }
        self.chats.push(chat.to_string());
        true
    }

    pub fn is_pending(&self, chat: &str) -> bool {
        self.chats.iter().any(|c| c == chat)
    }

    pub fn finish(&mut self, chat: &str) {
        self.chats.retain(|c| c != chat);
    }

    /// Any server answer (fresh snapshot or error) means what we show is the server's again.
    pub fn finish_all(&mut self) {
        self.chats.clear();
    }
}
