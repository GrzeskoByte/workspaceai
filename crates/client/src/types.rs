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
    /// "claude" or "opencode".
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub reduce_motion: bool,
    #[serde(default)]
    pub follow_system: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    #[serde(default)]
    pub files: bool,
    #[serde(default)]
    pub commands: bool,
    #[serde(default)]
    pub browser: bool,
    #[serde(default)]
    pub computer_use: bool,
    #[serde(default)]
    pub wake: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Activity {
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Computer {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub os: String,
    /// "online", "asleep" or "offline".
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub last_seen: i64,
    #[serde(default)]
    pub capabilities: Capabilities,
    /// workspace id → `{allowed, folders}`, kept raw: the server replaces the whole map on every change.
    #[serde(default)]
    pub access: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub activity: Vec<Activity>,
    #[serde(default)]
    pub managed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct AppState {
    #[serde(default)]
    pub computers: Vec<Computer>,
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
    /// opencode or an ACP agent id; empty when the supervisor did not say.
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: Option<String>,
    /// Full permissions: tool calls run without asking in this chat.
    #[serde(default)]
    pub yolo: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ProviderModel {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub note: String,
    /// Name of the keychain secret that holds the API key; absent for providers that need none.
    #[serde(default)]
    pub key_env: Option<String>,
    /// The endpoint of a custom provider (JSON `baseURL`).
    #[serde(rename = "baseURL", default)]
    pub base_url: Option<String>,
    /// Org-policy rows cannot be edited.
    #[serde(default)]
    pub locked: Option<bool>,
    #[serde(default)]
    pub models: Vec<ProviderModel>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct McpServer {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Display label such as "MCP · stdio".
    #[serde(default)]
    pub kind: String,
    /// "local", "remote" or "connector" (the JSON field is `type`).
    #[serde(rename = "type", default)]
    pub kind_of: String,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub locked: Option<bool>,
    #[serde(default)]
    pub builtin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub signed_in: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub agent_on: bool,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebSettings {
    /// "off", "exa", "mcp" or "searxng".
    #[serde(default)]
    pub mode: String,
    /// "Tavily", "Brave" or "Firecrawl" (mode "mcp").
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub searxng_url: String,
    #[serde(default)]
    pub fetch: bool,
    #[serde(default)]
    pub ask: bool,
    #[serde(default)]
    pub show_sources: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Member {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// "Admin", "Member" or "Viewer".
    #[serde(default)]
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Vault {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Skill {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct OrgInfo {
    #[serde(default)]
    pub issuer: String,
    #[serde(default)]
    pub idp: String,
    #[serde(default)]
    pub subject: String,
}

/// The workspace's own opencode connection (the Engine page). Missing fields mean managed, in sync.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct EngineSettings {
    /// "managed" or "remote".
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub sync: bool,
    /// A pinned opencode version, or "system" (or empty) for the one on PATH.
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenDesignSettings {
    /// "open-design" or "builtin".
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub agent_tools: bool,
    #[serde(default)]
    pub lint: bool,
    #[serde(default)]
    pub auto_start: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AuditEvent {
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub actor: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    #[serde(default)]
    pub version: serde_json::Value,
    #[serde(default)]
    pub block_external_sharing: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub default_project: String,
    #[serde(default)]
    pub web: WebSettings,
    #[serde(default)]
    pub members: Vec<Member>,
    #[serde(default)]
    pub vaults: Vec<Vault>,
    #[serde(default)]
    pub skills: Vec<Skill>,
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub public_links: bool,
    #[serde(default)]
    pub keep_chats: String,
    #[serde(default)]
    pub design_system: String,
    #[serde(default)]
    pub open_design: Option<OpenDesignSettings>,
    #[serde(default)]
    pub engine: EngineSettings,
    #[serde(default)]
    pub org: Option<OrgInfo>,
    #[serde(default)]
    pub mcp: Vec<McpServer>,
    #[serde(default)]
    pub providers: Vec<Provider>,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub default_model: String,
    /// Missing means on.
    #[serde(default)]
    pub use_memory: Option<bool>,
    /// Missing means on.
    #[serde(default)]
    pub memory_recall: Option<bool>,
    #[serde(default)]
    pub suggest_memories: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct Sidecar {
    pub name: String,
    #[serde(default)]
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct EngineStatus {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub sidecars: Vec<Sidecar>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub id: String,
    #[serde(default)]
    pub text: String,
    /// "ws" for the workspace, otherwise a project id.
    #[serde(default)]
    pub scope: String,
    /// "approved" or "suggested".
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RoutineTrigger {
    Schedule {
        cron: String,
        #[serde(default)]
        tz: String,
        #[serde(default)]
        once: Option<i64>,
    },
    Event {
        event: String,
        #[serde(default)]
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RoutineWhat {
    Prompt { prompt: String },
    Graph { graph: String },
    Artifact { artifact: String },
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RoutineRun {
    pub id: String,
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub phrase: String,
    pub trigger: RoutineTrigger,
    pub what: RoutineWhat,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub runs: Vec<RoutineRun>,
    #[serde(default)]
    pub last_fired: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ArtifactVersion {
    #[serde(default)]
    pub v: u32,
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub note: String,
    /// "markdown", "html", "csv", "svg" or "json"; absent on old versions.
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub chat: Option<String>,
    /// Doc, Deck, Prototype, Sheet, … (the JSON field is `type`).
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub versions: Vec<ArtifactVersion>,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub routines: Vec<Routine>,
    #[serde(default)]
    pub facts: Vec<Fact>,
    #[serde(default)]
    pub workspace: WorkspaceInfo,
    #[serde(default)]
    pub engine: EngineStatus,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub chats: Vec<Chat>,
    /// Chats with a turn in progress ("working" or "waiting"); the others are idle.
    #[serde(default)]
    pub chat_activity: HashMap<String, String>,
    #[serde(default)]
    pub audit: Vec<AuditEvent>,
    #[serde(default)]
    pub policy: Option<Policy>,
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
