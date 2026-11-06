// Shared data model for the harness. The supervisor persists these; the UI renders them.
// Every chat, artifact, loop and routine belongs to exactly one project (R-10, R-13).

export type WorkspaceKind = 'personal' | 'org' | 'team';
export type ThemeId = 'claude' | 'opencode';
export type AccentId = 'default' | 'teal' | 'amber' | 'blue' | 'green';
export type MemoryBackend = 'facts' | 'graph' | 'both' | 'mcp';
export type WebMode = 'off' | 'exa' | 'mcp' | 'searxng';
export type WebProvider = 'Tavily' | 'Brave' | 'Firecrawl';
export type ArtifactType = 'Doc' | 'Deck' | 'Prototype' | 'Sheet' | 'Image' | 'Video' | 'LiveArtifact' | 'DesignSystem';

export interface AppSettings {
  theme: ThemeId;
  accent: AccentId;
  reduceMotion: boolean;
  followSystem: boolean;
  onboarded: boolean;
  activeWorkspace: string | null;
  window?: { width: number; height: number };
}

export interface ProviderModel {
  id: string;
  name?: string;
  context?: number;
  output?: number;
}

export interface Provider {
  id: string;
  name: string;
  note: string;
  /** AI SDK package written to opencode.json `npm`; empty for providers opencode knows natively. */
  npm?: string;
  baseURL?: string;
  /** Name of the env var the key is injected as; the value lives only in the OS keychain (R-34). */
  keyEnv?: string;
  models: ProviderModel[];
  isDefault?: boolean;
  /** Locked rows come from org policy and cannot be edited (R-07, R-36). */
  locked?: boolean;
  /** Provider already configured in the user's opencode, reused as is (R-18, onboarding "From opencode"). */
  fromOpencode?: boolean;
  preset?: 'anthropic' | 'openrouter' | 'ollama' | 'litellm' | 'custom' | 'opencode';
}

export interface McpServer {
  id: string;
  name: string;
  kind: string;
  /** local: command array; remote: url */
  type: 'local' | 'remote' | 'connector';
  command?: string[];
  url?: string;
  /** env var name → keychain secret name (never a value). */
  env?: Record<string, string>;
  on: boolean;
  locked?: boolean;
  builtin?: 'memory' | 'open-design' | 'playwright' | 'search';
}

export type SubscriptionId = 'opencode' | 'claude' | 'grok';

export interface AcpAgent {
  id: string;
  name: string;
  via: string;
  command?: string;
  on: boolean;
  builtin?: boolean;
  /** A subscription the user already pays for; connection state is per workspace. */
  subscription?: SubscriptionId;
}

/** One row on Settings › Agents: a subscription-backed agent and its workspace-local state. */
export interface SubscriptionStatus {
  id: SubscriptionId;
  name: string;
  installed: boolean;
  signedIn: boolean;
  version?: string;
  account?: string;
  /** Where this workspace keeps the agent's own chats, history and config (never shared). */
  dataDir: string;
  command: string;
  connected: boolean;
  agentOn: boolean;
  models: string[];
  detail: string;
}

export interface WebSearchSettings {
  mode: WebMode;
  provider: WebProvider;
  searxngUrl: string;
  fetch: boolean;
  ask: boolean;
  showSources: boolean;
  locked: boolean;
}

export interface EngineSettings {
  mode: 'managed' | 'remote';
  url: string;
  username: string;
  /** keychain secret name holding the remote password */
  passwordSecret?: string;
  sync: boolean;
  /** pinned opencode version, or 'system' */
  version: string;
  binary: string;
  idleStopMinutes: number;
}

export interface Member { name: string; email?: string; role: 'Admin' | 'Member' | 'Viewer' }

export interface Workspace {
  id: string;
  name: string;
  kind: WorkspaceKind;
  initials: string;
  slug: string;
  accent: AccentId;
  createdAt: number;
  instructions: string;
  defaultModel: string;
  defaultProject: string;
  memory: MemoryBackend;
  memoryMcpCommand?: string;
  graphifyFolders: string[];
  graphifyWatch: boolean;
  /** Obsidian vaults attached to this workspace. */
  vaults?: Vault[];
  suggestMemories: boolean;
  designSystem: string;
  /** How artifacts are made and stored. Missing on older workspaces: see `odSettings`. */
  openDesign?: OpenDesignSettings;
  /** Every chat uses this workspace's memory: agents are told to, each session opens with what memory holds, and
   * the memory tools are connected. Missing means on; off leaves memory to the user. */
  useMemory?: boolean;
  /** Look every message up in memory (graph and facts) and hand the agent what matched. Missing means on. */
  memoryRecall?: boolean;
  /** lean-ctx connected to this workspace's agents (compressed reads, searches and shell output). Missing means on once installed. */
  leanCtx?: { enabled: boolean };
  engine: EngineSettings;
  web: WebSearchSettings;
  agents: AcpAgent[];
  defaultAgent: string;
  acpAskFirst: boolean;
  providers: Provider[];
  mcp: McpServer[];
  members: Member[];
  skills: { id: string; name: string; note: string; on: boolean }[];
  visibility: 'Private' | 'Workspace';
  publicLinks: boolean;
  keepChats: 'Forever' | '1 year' | '90 days';
  routineAskFirst: boolean;
  routineIfAsleep: 'Run when it wakes' | 'Wake it' | 'Skip';
  org?: OrgInfo;
}

export interface OrgInfo {
  issuer: string;
  idp: string;
  subject: string;
  policyVersion: string;
  policyVerified: boolean;
  lastPolicyFetch: number;
}

/** Signed organization policy (R-36). */
export interface OrgPolicy {
  version: string;
  org: string;
  allowedProviders: Provider[];
  allowedModels: string[];
  gateway?: string;
  blockPersonalKeys: boolean;
  blockExternalSharing: boolean;
  allowedAgents: string[];
  lockedMcp: McpServer[];
  web: { mode: WebMode; searxngUrl: string; locked: boolean };
  retention: '1 year' | '90 days' | 'Forever';
  logContent: boolean;
  residency: string;
  allowedEndpoints: string[];
  /** Where the org collects audit logs, and how often (minutes); omitted means the log stays local. */
  auditUrl?: string;
  auditEveryMinutes?: number;
  /** opencode release the org holds its workspaces on; omitted means the user's choice. */
  opencodeVersion?: string;
  issuedAt: number;
}

export interface SignedPolicy { policy: OrgPolicy; signature: string; keyId: string }

export interface Project {
  id: string;
  name: string;
  parent: string | null;
  folder: string;
  inherit: boolean;
  model: string;
  archived: boolean;
  isGeneral?: boolean;
  createdAt: number;
  /** Starts this project's dev server for the browser beside the chat; missing means detected from the folder. */
  devCommand?: string;
  /** Vault folders this project's agents can edit. The rest of the workspace's vaults stay read-only to them. */
  vaults?: VaultAttachment[];
}

/** A folder of Markdown notes, usually an Obsidian vault. The files stay where they are. */
export interface Vault { id: string; name: string; path: string }
/** `folder` is relative to the vault; '' means the whole vault. */
export interface VaultAttachment { vaultId: string; folder: string }

export interface ToolStep { name: string; detail?: string; status: 'running' | 'ok' | 'error'; input?: string; output?: string }

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  text: string;
  tools?: ToolStep[];
  artifact?: string | null;
  sources?: { title: string; url: string }[];
  createdAt: number;
  model?: string;
  error?: string;
  /** The turn failed because this subscription's sign-in expired: the chat offers to sign in again. */
  signIn?: SubscriptionId;
  /** A graph run started from this chat: the message shows it live. */
  run?: string;
}

export interface Chat {
  id: string;
  project: string;
  title: string;
  /** opencode session id (`ses_…`), one to one (R-16). ACP chats use `acp:<agent>:<id>`. */
  sid: string;
  agent: string;
  model: string;
  /** Reasoning level for the model ('' or missing = the model's default). opencode calls it a variant. */
  effort?: string;
  web: boolean | null;
  instructions: string;
  imported: boolean;
  archived: boolean;
  /** Full-permissions ("YOLO") mode: tool calls run without asking in this chat only. */
  yolo?: boolean;
  createdAt: number;
  updatedAt: number;
  /** transcript kept by the harness for ACP chats, cache for opencode chats */
  transcript?: ChatMessage[];
  /** Graph run results waiting to be handed to the agent with the next message. */
  graphNotes?: string[];
}

export interface LintFinding { severity?: string; level?: string; rule?: string; id?: string; message?: string; [k: string]: unknown }
/** `od` is set when Open Design saved and linted this version (R-04, R-15 "lint runs before Ready"). */
/** What a version is written in. Docs and decks are Markdown or HTML, prototypes HTML, sheets CSV. */
export type ArtifactFormat = 'markdown' | 'html' | 'csv' | 'svg' | 'json';

export interface ArtifactVersion { v: number; at: number; chat: string | null; note: string; file: string; format?: ArtifactFormat; od?: { project: string; file: string; lint: LintFinding[] } | null }

export interface Artifact {
  id: string;
  project: string;
  /** origin chat (opencode session that made it), if any (R-13). */
  chat: string | null;
  originSession: string | null;
  type: ArtifactType;
  title: string;
  version: number;
  versions: ArtifactVersion[];
  designSystem: string;
  createdAt: number;
  updatedAt: number;
}

export interface Fact {
  id: string;
  text: string;
  /** 'ws' for workspace scope, otherwise a project id */
  scope: string;
  status: 'approved' | 'suggested';
  source?: string;
  createdAt: number;
}

export interface AuditEvent { at: number; actor: string; action: string; detail: string }

export interface RoutineRun { id: string; at: number; status: 'ok' | 'error' | 'waiting' | 'skipped' | 'running'; note: string; chat?: string }

export interface Routine {
  id: string;
  name: string;
  phrase: string;
  trigger: { kind: 'schedule'; cron: string; tz: string; once?: number } | { kind: 'event'; event: string; detail: string };
  what: { kind: 'prompt'; prompt: string } | { kind: 'graph'; graph: string } | { kind: 'artifact'; artifact: string };
  project: string;
  runsOn: 'cloud' | 'computer' | 'org-runner';
  computer?: string;
  approval: 'ask' | 'pre-approved';
  preApproved: string[];
  ifAsleep: 'Run when it wakes' | 'Wake it' | 'Skip';
  notify: ('push' | 'email' | 'chat')[];
  paused: boolean;
  runs: RoutineRun[];
  createdAt: number;
  /** When a trigger last fired (schedules catch up once after the app was closed). */
  lastFired?: number;
  /** Secret in the routine's webhook URL. */
  hookKey?: string;
}

export interface Computer {
  id: string;
  name: string;
  os: string;
  status: 'online' | 'asleep' | 'offline';
  lastSeen: number;
  capabilities: { files: boolean; commands: boolean; browser: boolean; computerUse: boolean; wake: boolean };
  allowedApps: string[];
  askFirst: { deleteFiles: boolean; sendMessages: boolean; purchases: boolean; installSoftware: boolean; systemSettings: boolean; openApps: boolean };
  /** workspaceId → access */
  access: Record<string, { allowed: boolean; folders: { path: string; mode: 'ro' | 'rw' }[] }>;
  activity: { at: number; text: string }[];
  managed?: boolean;
  /** For Wake-on-LAN. */
  mac?: string;
  /** Live view of work running on this computer (R-28). */
  live?: LiveSession | null;
}

export interface LiveSession {
  workspace: string;
  title: string;
  /** The next action, in words. */
  action: string;
  controlledBy: 'agent' | 'you';
  state: 'running' | 'paused' | 'stopped';
  ref: { kind: 'graph' | 'chat'; id: string };
  startedAt: number;
  steps: { at: number; text: string }[];
}

/** `source: 'file'` — found as a .bpmn file in the project folder rather than made in the app. */
/** `chat` — the chat whose agent made it (it is listed under that chat). */
export interface GraphMeta { id: string; name: string; project: string; file: string; updatedAt: number; pattern?: string; source?: 'file'; chat?: string }

export interface GraphRun {
  id: string;
  graph: string;
  status: 'running' | 'paused' | 'waiting' | 'completed' | 'failed' | 'stopped';
  dryRun: boolean;
  startedAt: number;
  updatedAt: number;
  log: { at: number; element: string; text: string }[];
  waiting?: { element: string; name: string; prompt: string } | null;
  completed: string[];
  /** Steps running now, the sequence flows the run took, and the step that failed — drawn on the diagram. */
  active?: string[];
  taken?: string[];
  failedAt?: string | null;
  /** Step outputs keyed by element id; agent steps hold the reply text. */
  outputs?: Record<string, unknown>;
  /** The chat holding this run's opencode session (one reviewable transcript per run). */
  chat?: string | null;
  /** Every chat the run's steps use (its opencode chat and one per ACP agent): their approval requests are the run's. */
  chats?: string[];
  /** The chat the run was started from (`/graph`, or an agent's graph_run); it shows the run and gets its results. */
  from?: string | null;
  /** Hand the results to the agent with the next message in `from` (runs the user started; an agent's own
   * graph_run already gets them as its tool result). */
  tellAgent?: boolean;
}

/** Graphify memory as the Settings › Memory screen draws it (R-23). */
export interface MemoryGraphNode {
  id: string;
  label: string;
  kind: 'folder' | 'file' | 'symbol' | 'concept' | 'community';
  community: string;
  size?: number;
  x?: number;
  y?: number;
  /** The file this node comes from, when Graphify recorded one. */
  path?: string;
}

export interface MemoryGraphEdge {
  from: string;
  to: string;
  kind: string;
  weight?: number;
  ambiguous?: boolean;
}

export interface MemoryGraph {
  source: 'graphify' | 'harness-index';
  builtAt: number;
  nodes: MemoryGraphNode[];
  edges: MemoryGraphEdge[];
  communities: { id: string; name: string; size: number; color: string }[];
  ambiguous: { id: string; from: string; to: string; reason: string; status: 'pending' | 'kept' | 'dropped' }[];
  folders: string[];
  fileCount: number;
  truncated: boolean;
}

export interface GraphifyStatus {
  installed: boolean;
  version?: string;
  graphFile?: string;
  folders: string[];
  watching: boolean;
  builtAt?: number;
  nodes: number;
  edges: number;
  communities: number;
  pendingLinks: number;
  error?: string;
}

/** Everything the UI needs for the active workspace. Nothing from other workspaces is included (R-06). */
export interface WorkspaceSnapshot {
  workspace: Workspace;
  projects: Project[];
  chats: Chat[];
  artifacts: Artifact[];
  facts: Fact[];
  routines: Routine[];
  graphs: GraphMeta[];
  audit: AuditEvent[];
  engine: EngineStatus;
  policy: OrgPolicy | null;
  /** Chats with a turn in progress; the others are idle. */
  chatActivity: Record<string, 'working' | 'waiting'>;
}

export interface EngineStatus {
  state: 'stopped' | 'starting' | 'running' | 'restarting' | 'error' | 'connected';
  url: string;
  port: number;
  version: string;
  pid?: number;
  error?: string;
  sidecars: { name: string; state: string; pid?: number }[];
}

export interface WorkspaceSummary { id: string; name: string; kind: WorkspaceKind; initials: string; accent: AccentId; note: string }

export interface AppState {
  settings: AppSettings;
  workspaces: WorkspaceSummary[];
  computers: Computer[];
}

/** Events pushed from supervisor to UI over SSE. */
export type UiEvent =
  | { type: 'state'; workspace: string | null }
  | { type: 'snapshot'; workspace: string }
  | { type: 'chat.delta'; workspace: string; chat: string; messageId: string; delta: string }
  | { type: 'chat.tool'; workspace: string; chat: string; messageId: string; tool: ToolStep }
  | { type: 'chat.done'; workspace: string; chat: string; outcome: string }
  | { type: 'chat.permission'; workspace: string; chat: string; request: { id: string; sid: string; title: string; detail: string } }
  | { type: 'chat.permission.clear'; workspace: string; chat: string; requestId?: string }
  | { type: 'toast'; text: string }
  | { type: 'map'; workspace: string; scope: string }
  /** Harness changed a note: the vault screen refreshes its tree and the open note. */
  | { type: 'vault'; workspace: string; vault: string; path: string }
  | { type: 'engine'; workspace: string; status: EngineStatus }
  | { type: 'models'; workspace: string }
  | { type: 'graph.run'; workspace: string; run: GraphRun }
  /** An agent saved an artifact from this chat; the chat opens it beside the conversation. */
  | { type: 'artifact'; workspace: string; chat: string; artifact: string; version: number }
  | { type: 'routine.run'; workspace: string; routine: string; run: RoutineRun };

/** One model in the model picker: what it is called and which effort levels it takes (none = no setting). */
export interface ModelOption { id: string; name: string; description?: string; efforts?: string[]; defaultEffort?: string }
/** A group in the model picker: an opencode provider the workspace has connected. */
export interface ModelGroup { provider: string; name: string; note: string; models: ModelOption[] }

/** Open Design as the artifact engine (R-04). */
export interface OpenDesignSettings {
  /** 'open-design': every artifact is saved, versioned and linted by Open Design, and saving fails if it can't be.
   *  'builtin': the harness's own store only (no lint, no PDF/PPTX export). */
  engine: 'open-design' | 'builtin';
  /** Also give agents Open Design's own MCP tools (projects, files, skills, runs). Off by default, so artifacts
   *  always go through the harness tools and show up beside the chat. */
  agentTools: boolean;
  /** Lint every version before saving it. */
  lint: boolean;
  /** Start the daemon with the workspace instead of on the first artifact. */
  autoStart: boolean;
}

// ---------- project mind maps ----------
/** What a map node stands for. Items come from the workspace's data; idea, task and note nodes are made on the map. */
export type MapNodeKind = 'workspace' | 'project' | 'chat' | 'artifact' | 'graph' | 'run' | 'fact' | 'memory' | 'routine' | 'group' | 'idea' | 'task' | 'note' | 'repo' | 'milestone' | 'ticket';
export type MapTaskStatus = 'todo' | 'doing' | 'done';
export type MapLayout = 'right' | 'balance';
export type MapSide = 'left' | 'right';

/** A node made on the map, by you or an agent. */
export interface MapOwnNode { id: string; kind: 'idea' | 'task' | 'note'; text: string; status?: MapTaskStatus; parent: string; order: number; side?: MapSide; createdBy: 'user' | 'agent'; createdAt: number }
/** Where an item from the workspace was moved to on the map. */
export interface MapPlacement { parent: string; order: number; side?: MapSide }
export interface MapLink { id: string; from: string; to: string; label?: string }

/** What is saved for a map: only what can't be derived from the workspace. Positions are never saved. */
export interface ProjectMap { nodes: MapOwnNode[]; placements: Record<string, MapPlacement>; links: MapLink[]; hidden: string[]; layout: MapLayout }

export interface ResolvedMapNode {
  /** `chat:<id>`, `artifact:<id>`… for items; the own node's id otherwise. */
  id: string;
  kind: MapNodeKind;
  /** The id of the item this node stands for. */
  ref?: string;
  title: string;
  meta?: string;
  parent: string | null;
  order: number;
  side?: MapSide;
  status?: MapTaskStatus;
  own?: boolean;
  /** Starts collapsed unless the viewer opened it (memory, sub-projects' contents). */
  folded?: boolean;
  /** For an artifact node: its type, which picks the icon. */
  subtype?: string;
  /** The page a repository, milestone or ticket opens. */
  url?: string;
  /** A repository GitLab refused without a token; `ref` is its host. */
  needsToken?: boolean;
}
export interface ResolvedMap { scope: string; root: string; layout: MapLayout; nodes: ResolvedMapNode[]; links: MapLink[]; hiddenItems?: { id: string; title: string }[] }

export type MapOp =
  | { op: 'add'; parent: string; kind?: 'idea' | 'task' | 'note'; text: string; status?: MapTaskStatus; side?: MapSide; before?: string; by?: 'user' | 'agent' }
  | { op: 'import'; parent: string; markdown: string; by?: 'user' | 'agent' }
  | { op: 'update'; id: string; text?: string; status?: MapTaskStatus | null; kind?: 'idea' | 'task' | 'note' }
  | { op: 'move'; id: string; parent?: string; before?: string; side?: MapSide }
  | { op: 'link'; from: string; to: string; label?: string }
  | { op: 'unlink'; id: string }
  | { op: 'hide'; id: string }
  | { op: 'unhide'; id: string }
  | { op: 'remove'; id: string }
  | { op: 'setLayout'; layout: MapLayout };

/** One Graphify node as the map shows it: what it connects to and, for a file, a preview. */
export interface MemoryNodeDetail {
  node: MemoryGraphNode;
  community?: string;
  links: { id: string; label: string; kind: MemoryGraphNode['kind']; relation: string }[];
  file?: { path: string; size: number; text?: string; image?: string; truncated?: boolean; note?: string };
}
