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
