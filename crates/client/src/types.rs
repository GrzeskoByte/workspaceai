//! The slice of the supervisor's JSON the native UI reads. Mirrors `packages/shared/src/types.ts`; fields the UI
//! does not use are ignored, and missing optional ones default.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct WorkspaceSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub initials: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub active_workspace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct AppState {
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceSummary>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub is_general: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub project: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub chats: Vec<Chat>,
    /// Chats with a turn in progress ("working" or "waiting"); the others are idle.
    #[serde(default)]
    pub chat_activity: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ToolStep {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub tools: Vec<ToolStep>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PermissionRequest {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub detail: String,
}

/// Events pushed by the supervisor over SSE. Types this slice does not use become `Other`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum UiEvent {
    #[serde(rename = "state")]
    State { workspace: Option<String> },
    #[serde(rename = "snapshot")]
    Snapshot { workspace: String },
    #[serde(rename = "chat.delta")]
    ChatDelta {
        workspace: String,
        chat: String,
        #[serde(rename = "messageId")]
        message_id: String,
        delta: String,
    },
    #[serde(rename = "chat.tool")]
    ChatTool {
        workspace: String,
        chat: String,
        #[serde(rename = "messageId")]
        message_id: String,
        tool: ToolStep,
    },
    #[serde(rename = "chat.done")]
    ChatDone { workspace: String, chat: String, outcome: String },
    #[serde(rename = "chat.permission")]
    ChatPermission { workspace: String, chat: String, request: PermissionRequest },
    #[serde(rename = "chat.permission.clear")]
    ChatPermissionClear {
        workspace: String,
        chat: String,
        #[serde(rename = "requestId", default)]
        request_id: Option<String>,
    },
    #[serde(rename = "toast")]
    Toast { text: String },
    #[serde(other)]
    Other,
}
