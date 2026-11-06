//! Shared data model. Port of `packages/shared/src/types.ts`; JSON field names are the TS ones (camelCase).
//! Every struct is tolerant on read (missing field = default) and keeps fields it does not know in `extra`,
//! so `state.json` / `harness.json` written by the TS supervisor round-trip without losing anything.
//! Not ported (spec non-goals): graph/map/Graphify types and `UiEvent` (redefined with the in-process backend).

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Fields a struct does not name.
pub type Extra = Map<String, Value>;

/// `field?: T | null`: absent, null and a value stay three different things.
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

macro_rules! string_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
        pub enum $name { $($(#[$vm])* #[serde(rename = $s)] $v),+ }
        impl $name {
            pub fn as_str(&self) -> &'static str { match self { $(Self::$v => $s),+ } }
        }
    };
}

string_enum!(WorkspaceKind { #[default] Personal = "personal", Org = "org", Team = "team" });
string_enum!(MemoryBackend { Facts = "facts", Graph = "graph", #[default] Both = "both", Mcp = "mcp" });
string_enum!(WebMode { Off = "off", #[default] Exa = "exa", Mcp = "mcp", Searxng = "searxng" });
string_enum!(WebProvider { #[default] Tavily = "Tavily", Brave = "Brave", Firecrawl = "Firecrawl" });
string_enum!(McpType { #[default] Local = "local", Remote = "remote", Connector = "connector" });
string_enum!(EngineMode { #[default] Managed = "managed", Remote = "remote" });
string_enum!(FactStatus { #[default] Approved = "approved", Suggested = "suggested" });
string_enum!(SubscriptionId { #[default] Opencode = "opencode", Claude = "claude", Grok = "grok" });
string_enum!(Visibility { #[default] Private = "Private", Workspace = "Workspace" });
string_enum!(KeepChats { #[default] Forever = "Forever", Year = "1 year", Days90 = "90 days" });
string_enum!(IfAsleep { #[default] WhenWakes = "Run when it wakes", Wake = "Wake it", Skip = "Skip" });
string_enum!(MemberRole { #[default] Admin = "Admin", Member = "Member", Viewer = "Viewer" });
string_enum!(ArtifactFormat { #[default] Markdown = "markdown", Html = "html", Csv = "csv", Svg = "svg", Json = "json" });
string_enum!(ArtifactType {
    #[default] Doc = "Doc", Deck = "Deck", Prototype = "Prototype", Sheet = "Sheet", Image = "Image",
    Video = "Video", LiveArtifact = "LiveArtifact", DesignSystem = "DesignSystem",
});
string_enum!(Role { #[default] User = "user", Assistant = "assistant", System = "system" });
string_enum!(ToolStatus { #[default] Running = "running", Ok = "ok", Error = "error" });
string_enum!(OdEngine { #[default] OpenDesign = "open-design", Builtin = "builtin" });

/// Accent ids are a closed set in the UI, but an unknown one in a file must survive: kept as text.
pub type AccentId = String;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    pub theme: String,
    pub accent: AccentId,
    pub reduce_motion: bool,
    pub follow_system: bool,
    pub onboarded: bool,
    pub active_workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowSize>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSize {
    /// `Number` keeps 1280 as 1280 (an `f64` would write 1280.0).
    pub width: serde_json::Number,
    pub height: serde_json::Number,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderModel {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<i64>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub note: String,
    /// AI SDK package written to opencode.json `npm`; empty for providers opencode knows natively.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,
    #[serde(rename = "baseURL", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Name of the env var the key is injected as; the value lives only in the OS keychain (R-34).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    pub models: Vec<ProviderModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
    /// Locked rows come from org policy and cannot be edited (R-07, R-36).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    /// Provider already configured in the user's opencode, reused as is (R-18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_opencode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(rename = "type")]
    pub kind_type: McpType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// env var name → keychain secret name (never a value).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    pub on: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AcpAgent {
    pub id: String,
    pub name: String,
    pub via: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub on: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builtin: Option<bool>,
    /// A subscription the user already pays for; connection state is per workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription: Option<SubscriptionId>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One row on Settings › Agents.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubscriptionStatus {
    pub id: SubscriptionId,
    pub name: String,
    pub installed: bool,
    pub signed_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    pub data_dir: String,
    pub command: String,
    pub connected: bool,
    pub agent_on: bool,
    pub models: Vec<String>,
    pub detail: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WebSearchSettings {
    pub mode: WebMode,
    pub provider: WebProvider,
    pub searxng_url: String,
    pub fetch: bool,
    pub ask: bool,
    pub show_sources: bool,
    pub locked: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EngineSettings {
    pub mode: EngineMode,
    pub url: String,
    pub username: String,
    /// keychain secret name holding the remote password
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_secret: Option<String>,
    pub sync: bool,
    /// pinned opencode version, or 'system'
    pub version: String,
    pub binary: String,
    pub idle_stop_minutes: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Member {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub role: MemberRole,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub note: String,
    pub on: bool,
}

/// Open Design as the artifact engine (R-04). The default is `DEFAULT_OPEN_DESIGN` in `defaults.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OpenDesignSettings {
    /// 'open-design': every artifact is saved, versioned and linted by Open Design; 'builtin': the harness store only.
    pub engine: OdEngine,
    /// Also give agents Open Design's own MCP tools. Off by default.
    pub agent_tools: bool,
    /// Lint every version before saving it.
    pub lint: bool,
    /// Start the daemon with the workspace instead of on the first artifact.
    pub auto_start: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for OpenDesignSettings {
    fn default() -> Self {
        Self { engine: OdEngine::OpenDesign, agent_tools: false, lint: true, auto_start: true, extra: Extra::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LeanCtxSettings {
    pub enabled: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A folder of Markdown notes, usually an Obsidian vault. The files stay where they are.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Vault {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `folder` is relative to the vault; '' means the whole vault.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VaultAttachment {
    pub vault_id: String,
    pub folder: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub kind: WorkspaceKind,
    pub initials: String,
    pub slug: String,
    pub accent: AccentId,
    pub created_at: i64,
    pub instructions: String,
    pub default_model: String,
    pub default_project: String,
    pub memory: MemoryBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mcp_command: Option<String>,
    pub graphify_folders: Vec<String>,
    pub graphify_watch: bool,
    /// Obsidian vaults attached to this workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vaults: Option<Vec<Vault>>,
    pub suggest_memories: bool,
    pub design_system: String,
    /// How artifacts are made and stored. Missing on older workspaces: see `od_settings`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_design: Option<OpenDesignSettings>,
    /// Missing means on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_memory: Option<bool>,
    /// Missing means on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_recall: Option<bool>,
    /// Missing means on once installed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lean_ctx: Option<LeanCtxSettings>,
    pub engine: EngineSettings,
    pub web: WebSearchSettings,
    pub agents: Vec<AcpAgent>,
    pub default_agent: String,
    pub acp_ask_first: bool,
    pub providers: Vec<Provider>,
    pub mcp: Vec<McpServer>,
    pub members: Vec<Member>,
    pub skills: Vec<Skill>,
    pub visibility: Visibility,
    pub public_links: bool,
    pub keep_chats: KeepChats,
    pub routine_ask_first: bool,
    pub routine_if_asleep: IfAsleep,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<OrgInfo>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OrgInfo {
    pub issuer: String,
    pub idp: String,
    pub subject: String,
    pub policy_version: String,
    pub policy_verified: bool,
    pub last_policy_fetch: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OrgWeb {
    pub mode: WebMode,
    #[serde(rename = "searxngUrl")]
    pub searxng_url: String,
    pub locked: bool,
}

/// Signed organization policy (R-36).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OrgPolicy {
    pub version: String,
    pub org: String,
    pub allowed_providers: Vec<Provider>,
    pub allowed_models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    pub block_personal_keys: bool,
    pub block_external_sharing: bool,
    pub allowed_agents: Vec<String>,
    pub locked_mcp: Vec<McpServer>,
    pub web: OrgWeb,
    pub retention: KeepChats,
    pub log_content: bool,
    pub residency: String,
    pub allowed_endpoints: Vec<String>,
    /// Where the org collects audit logs, and how often (minutes); omitted means the log stays local.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit_every_minutes: Option<i64>,
    /// opencode release the org holds its workspaces on; omitted means the user's choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opencode_version: Option<String>,
    pub issued_at: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SignedPolicy {
    pub policy: OrgPolicy,
    pub signature: String,
    pub key_id: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub folder: String,
    pub inherit: bool,
    pub model: String,
    pub archived: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_general: Option<bool>,
    pub created_at: i64,
    /// Starts this project's dev server; missing means detected from the folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_command: Option<String>,
    /// Vault folders this project's agents can edit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vaults: Option<Vec<VaultAttachment>>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolStep {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub status: ToolStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Source {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: Role,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolStep>>,
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<Source>>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The turn failed because this subscription's sign-in expired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sign_in: Option<SubscriptionId>,
    /// A graph run started from this chat (graph runs are not ported; kept as written).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Becomes `Session` in batch 5; kept here so existing `state.json` files load.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub project: String,
    pub title: String,
    /// opencode session id (`ses_…`), one to one (R-16). ACP chats use `acp:<agent>:<id>`.
    pub sid: String,
    pub agent: String,
    pub model: String,
    /// Reasoning level ('' or missing = the model's default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub web: Option<bool>,
    pub instructions: String,
    pub imported: bool,
    pub archived: bool,
    /// Full-permissions ("YOLO") mode for this chat only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yolo: Option<bool>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Vec<ChatMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_notes: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LintFinding {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OdRef {
    pub project: String,
    pub file: String,
    pub lint: Vec<LintFinding>,
}

/// `od` is set when Open Design saved and linted this version (R-04, R-15).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtifactVersion {
    pub v: i64,
    pub at: i64,
    pub chat: Option<String>,
    pub note: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ArtifactFormat>,
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    pub od: Option<Option<OdRef>>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub project: String,
    /// origin chat (opencode session that made it), if any (R-13).
    pub chat: Option<String>,
    pub origin_session: Option<String>,
    #[serde(rename = "type")]
    pub kind: ArtifactType,
    pub title: String,
    pub version: i64,
    pub versions: Vec<ArtifactVersion>,
    pub design_system: String,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Fact {
    pub id: String,
    pub text: String,
    /// 'ws' for workspace scope, otherwise a project id
    pub scope: String,
    pub status: FactStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub created_at: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AuditEvent {
    pub at: i64,
    pub actor: String,
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RoutineRun {
    pub id: String,
    pub at: i64,
    /// 'ok' | 'error' | 'waiting' | 'skipped' | 'running'
    pub status: String,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Trigger {
    Schedule {
        cron: String,
        tz: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        once: Option<i64>,
    },
    Event { event: String, detail: String },
}

impl Default for Trigger {
    fn default() -> Self {
        Trigger::Schedule { cron: String::new(), tz: String::new(), once: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum What {
    Prompt { prompt: String },
    Graph { graph: String },
    Artifact { artifact: String },
}

impl Default for What {
    fn default() -> Self {
        What::Prompt { prompt: String::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Routine {
    pub id: String,
    pub name: String,
    pub phrase: String,
    pub trigger: Trigger,
    pub what: What,
    pub project: String,
    /// 'cloud' | 'computer' | 'org-runner'
    pub runs_on: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub computer: Option<String>,
    /// 'ask' | 'pre-approved'
    pub approval: String,
    pub pre_approved: Vec<String>,
    pub if_asleep: IfAsleep,
    /// 'push' | 'email' | 'chat'
    pub notify: Vec<String>,
    pub paused: bool,
    pub runs: Vec<RoutineRun>,
    pub created_at: i64,
    /// When a trigger last fired (schedules catch up once after the app was closed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fired: Option<i64>,
    /// Secret in the routine's webhook URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_key: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Capabilities {
    pub files: bool,
    pub commands: bool,
    pub browser: bool,
    pub computer_use: bool,
    pub wake: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AskFirst {
    pub delete_files: bool,
    pub send_messages: bool,
    pub purchases: bool,
    pub install_software: bool,
    pub system_settings: bool,
    pub open_apps: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderGrant {
    pub path: String,
    /// 'ro' | 'rw'
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ComputerAccess {
    pub allowed: bool,
    pub folders: Vec<FolderGrant>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ActivityLine {
    pub at: i64,
    pub text: String,
}

/// Live view of work running on a computer (R-28).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LiveSession {
    pub workspace: String,
    pub title: String,
    /// The next action, in words.
    pub action: String,
    /// 'agent' | 'you'
    pub controlled_by: String,
    /// 'running' | 'paused' | 'stopped'
    pub state: String,
    /// `{ kind: 'graph' | 'chat', id }`
    #[serde(rename = "ref")]
    pub reference: Value,
    pub started_at: i64,
    pub steps: Vec<ActivityLine>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Computer {
    pub id: String,
    pub name: String,
    pub os: String,
    /// 'online' | 'asleep' | 'offline'
    pub status: String,
    pub last_seen: i64,
    pub capabilities: Capabilities,
    pub allowed_apps: Vec<String>,
    pub ask_first: AskFirst,
    /// workspaceId → access
    pub access: BTreeMap<String, ComputerAccess>,
    pub activity: Vec<ActivityLine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed: Option<bool>,
    /// For Wake-on-LAN.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    pub live: Option<Option<LiveSession>>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Sidecar {
    pub name: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineStatus {
    /// 'stopped' | 'starting' | 'running' | 'restarting' | 'error' | 'connected'
    pub state: String,
    pub url: String,
    pub port: i64,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub sidecars: Vec<Sidecar>,
}

/// Everything the UI needs for the active workspace. Nothing from other workspaces is included (R-06).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub workspace: Workspace,
    pub projects: Vec<Project>,
    pub chats: Vec<Chat>,
    pub artifacts: Vec<Artifact>,
    pub facts: Vec<Fact>,
    pub routines: Vec<Routine>,
    /// Graphs are not ported; kept as written.
    pub graphs: Vec<Value>,
    pub audit: Vec<AuditEvent>,
    pub engine: EngineStatus,
    pub policy: Option<OrgPolicy>,
    /// Chats with a turn in progress ('working' | 'waiting').
    pub chat_activity: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceSummary {
    pub id: String,
    pub name: String,
    pub kind: WorkspaceKind,
    pub initials: String,
    pub accent: AccentId,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    pub settings: AppSettings,
    pub workspaces: Vec<WorkspaceSummary>,
    pub computers: Vec<Computer>,
}

/// `data/state.json` of one workspace (`PersistedState` in runtime.ts). Engine/agent model caches and
/// command lists stay in `extra` as written.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PersistedState {
    pub projects: Vec<Project>,
    pub chats: Vec<Chat>,
    pub artifacts: Vec<Artifact>,
    pub routines: Vec<Routine>,
    pub graphs: Vec<Value>,
    /// sessions the user chose to keep out of this workspace (import skip list)
    pub skipped_sessions: Vec<String>,
    /// harness project id → Open Design project id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub od_projects: Option<BTreeMap<String, String>>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One model in the model picker.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub efforts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
}

/// A group in the model picker: an opencode provider the workspace has connected.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelGroup {
    pub provider: String,
    pub name: String,
    pub note: String,
    pub models: Vec<ModelOption>,
}
