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

/// Full access: the session's agent is relaunched with its skip-permissions flag.
pub fn full_access_body(on: bool) -> Value {
    json!({ "fullAccess": on })
}

/// Shown, and confirmed, before full access is switched on.
pub const FULL_ACCESS_WARNING: &str = "Full access lets the agent run commands and change files without asking you first. Only use it in a project you trust. The session restarts to apply it.";

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

pub fn launch_path(workspace: &str, chat: &str) -> String {
    format!("/api/ws/{}/chats/{}/launch", crate::api::seg(workspace), crate::api::seg(chat))
}

/// What the supervisor says to run for a session (`GET …/chats/:cid/launch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<String>,
}

/// `None` when the reply has no command to run.
pub fn parse_launch(v: &Value) -> Option<Launch> {
    let argv: Vec<String> = v["argv"].as_array()?.iter().filter_map(|a| a.as_str().map(String::from)).collect();
    if argv.is_empty() {
        return None;
    }
    let env = v["env"].as_object().map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default();
    Some(Launch { argv, env, cwd: v["cwd"].as_str().filter(|c| !c.is_empty()).map(String::from) })
}
