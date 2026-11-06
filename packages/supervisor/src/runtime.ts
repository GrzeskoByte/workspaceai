// One WorkspaceRuntime per workspace: its own folders, secrets, opencode server and state (R-06).
import { join, resolve } from 'node:path';
import { mkdirSync, existsSync, rmSync, readdirSync, readFileSync } from 'node:fs';
import type {
  Workspace, Project, Chat, Artifact, Fact, Routine, GraphMeta, GraphRun, AuditEvent, EngineStatus, UiEvent,
  WorkspaceSnapshot, OrgPolicy, ChatMessage, ToolStep, Provider,
} from '../../shared/src/types.ts';
import { uid, projectPath, projectPathName, descendants, isWithin, childProjects, titleCaseSegment, nestFolders } from '../../shared/src/util.ts';
import { buildOpencodeJson, referencedEnv, containsLiteralSecret, effectiveProviders } from '../../shared/src/opencode-config.ts';
import { parseFacts, serializeFacts } from '../../shared/src/facts.ts';
import { mapOcMessages, sourcesFromTools, toolLabel } from '../../shared/src/messages.ts';
import { chatCommands, commandAction, findHarnessCommand, parseSlash, type ChatCommand, type NativeCommand } from '../../shared/src/commands.ts';
import { generalProject, ARTIFACT_RULE, FILE_REF_RULE, GRAPH_RULE, MAP_RULE, memoryRule } from '../../shared/src/defaults.ts';
import { memoryBrief, memoryRecall, stripRecall } from './recall.ts';
import { cleanAttachments, vaultRule } from './vault.ts';
import { isAuthFailure } from './signin.ts';
import { runSummary } from './graphs.ts';
import { withHarnessSteps, withLayout } from '../../orchestrator/src/layout.ts';

/** What Claude Code answers instead of a reply when its login is gone. */
const SIGNED_OUT_REPLY = /please run \/login|oauth (session|token)[^.]*expired|invalid api key|not logged in/i;
import { leanCtxBin, LEANCTX_RULE } from './leanctx.ts';
import { ensureWsDirs, wsPaths, harnessHome, expandHome, type WsPaths } from './paths.ts';
import { readJson, writeJson, readText, writeText, appendLine } from './jsonfile.ts';
import { keychain } from './keychain.ts';
import { ManagedServer, OpencodeClient, OpencodeError, baseEnv, opencodeBinary, remoteUrlAllowed, type OcEvent, type OcPermission, type OcSession } from './opencode.ts';
import { ArtifactStore } from './artifacts.ts';
import { stopDev } from './devserver.ts';
import { playwrightCli, chromiumArgs } from './browser.ts';
import { AcpClient, acpCommandFor, claudeEffortCatalog, type AcpModel, type AcpPermissionRequest, findOnPath } from './acp.ts';
import { agentEnv, agentHome, SUBSCRIPTIONS } from './subscriptions.ts';
import { piAgentDir } from './pi-config.ts';
import { execFile } from 'node:child_process';
import { homedir, tmpdir } from 'node:os';

interface PersistedState {
  projects: Project[];
  chats: Chat[];
  artifacts: Artifact[];
  routines: Routine[];
  graphs: GraphMeta[];
  /** sessions the user chose to keep out of this workspace (import skip list) */
  skippedSessions: string[];
  /** harness project id → Open Design project id (rooted at <project>/.harness/artifacts) */
  odProjects?: Record<string, string>;
  /** ACP agent id → the models it last reported, so the chat picker knows them before a session starts */
  agentModels?: Record<string, { models: AcpModel[]; current: string }>;
  /** opencode's own model list as it last reported it, so the picker is complete while the engine is stopped */
  engineModels?: EngineModel[];
  /** Slash commands each agent advertised (ACP `available_commands_update`; opencode `GET /command`) */
  agentCommands?: Record<string, NativeCommand[]>;
}

/** What `/status` shows for a chat. */
export interface ChatStatus {
  agent: string; agentName: string; model: string; effort: string; project: string; folder: string; session: string;
  web: boolean; fullAccess: boolean; connectors: number; engine: string;
  tokens?: { input: number; output: number; reasoning: number; cacheRead: number; cacheWrite: number }; cost?: number;
}

/** What running a slash command did: sent a turn (`ran`), or left an app-side action to the composer (`ui`). */
export interface CommandResult { ran?: string; ui?: 'status' | 'model' | 'effort' | 'new' | 'help' | 'graph' | 'sheet'; args?: string; status?: ChatStatus; sheet?: CommandSheet }

/** An action sheet's content: the harness's own figures and/or the agent's own output, run without a chat turn. */
export interface CommandSheet { command: string; title: string; status?: ChatStatus; text?: string; ran?: string; note?: string }

/** A model opencode serves, with its reasoning variants. */
interface EngineModel { id: string; providerID: string; name: string; efforts?: string[] }

export interface RuntimeHooks {
  emit: (e: UiEvent) => void;
  /** Extra MCP servers the supervisor registers in every workspace (memory, artifacts). */
  extraMcp: (rt: WorkspaceRuntime) => Record<string, unknown>;
  /** Env for those MCP servers (supervisor URL + token). */
  extraEnv: (rt: WorkspaceRuntime) => Record<string, string>;
  /** Sidecars that start before / stop after this workspace's opencode (Open Design daemon). */
  beforeStart?: (rt: WorkspaceRuntime) => Promise<{ name: string; state: string; pid?: number }[]>;
  afterStop?: (rt: WorkspaceRuntime) => Promise<void>;
}

/** `quiet`: a command run for an action sheet — collected, never streamed to the chat or kept in its transcript. */
interface LiveTurn { chatId: string; messageId: string; text: string; tools: ToolStep[]; started: number; quiet?: boolean }

/** Full permissions for one opencode session: every tool action is allowed without asking. */
const ALLOW_ALL: Array<{ action: string; resource: string; effect: 'allow' }> = [{ action: '*', resource: '*', effect: 'allow' }];

export class WorkspaceRuntime {
  readonly id: string;
  readonly paths: WsPaths;
  ws: Workspace;
  state: PersistedState;
  policy: OrgPolicy | null = null;
  private hooks: RuntimeHooks;
  private server: ManagedServer | null = null;
  private remote: OpencodeClient | null = null;
  /** A managed engine another WorkspaceAI process started for this same workspace (shared, not owned). */
  private attached: { pid: number; url: string; client: OpencodeClient } | null = null;
  private starting: Promise<OpencodeClient> | null = null;
  private pump: AbortController | null = null;
  private idleTimer: NodeJS.Timeout | null = null;
  private live = new Map<string, LiveTurn>();
  status: EngineStatus = { state: 'stopped', url: '', port: 0, version: '', sidecars: [] };
  readonly artifacts: ArtifactStore;
  /** Subscription agents run as ACP children, one process per agent per project folder (R-06, R-39). */
  private acpClients = new Map<string, AcpClient>();
  private acpChatBySession = new Map<string, string>();
  private acpLoaded = new Set<string>();
  private acpPerms = new Map<string, { chatId: string; options: AcpPermissionRequest['options']; title: string; detail: string; resolve: (optionId: string | null) => void }>();
  /** permission request id → the opencode session that owns it (may be a subagent child session). */
  private permSessions = new Map<string, string>();
  /** cache of child/root session id → harness chat id, so child-session events reach the right chat. */
  private sidChats = new Map<string, string>();
  /** Recent artifact tool calls per session, used to attribute agent-made artifacts to their chat (R-13). */
  artifactCalls: { sid: string; at: number; tool: string }[] = [];

  constructor(ws: Workspace, hooks: RuntimeHooks) {
    this.id = ws.id;
    this.ws = ws;
    this.hooks = hooks;
    this.paths = ensureWsDirs(ws.id);
    this.state = readJson<PersistedState>(this.paths.state, { projects: [], chats: [], artifacts: [], routines: [], graphs: [], skippedSessions: [] });
    this.state.skippedSessions ||= [];
    if (!this.state.projects.some((p) => p.isGeneral)) {
      const g = generalProject(ws, harnessHome());
      this.state.projects.unshift(g);
      this.ws.defaultProject ||= g.id;
    }
    for (const p of this.state.projects) mkdirSync(expandHome(p.folder), { recursive: true });
    this.policy = readJson<{ policy?: OrgPolicy } | null>(this.paths.policyJson, null)?.policy ?? null;
    this.artifacts = new ArtifactStore(this);
    this.migrateAgents();
  }

  /** Workspaces created before subscriptions existed get the Grok agent and the current ACP commands. */
  private migrateAgents(): void {
    if (this.ws.kind === 'org') return;
    const defaults: Workspace['agents'] = [
      { id: 'opencode', name: 'opencode', via: 'opencode serve', on: true, builtin: true, subscription: 'opencode' },
      { id: 'claude', name: 'Claude Code', via: 'ACP adapter · Claude subscription', command: 'npx -y @zed-industries/claude-code-acp', on: false, subscription: 'claude' },
      { id: 'grok', name: 'Grok', via: 'ACP · Grok subscription', command: 'grok agent stdio', on: false, subscription: 'grok' },
      { id: 'opencode-acp', name: 'opencode (ACP)', via: 'ACP · same opencode account', command: 'opencode acp', on: false, subscription: 'opencode' },
      { id: 'pi', name: 'Pi', via: 'ACP adapter · pi’s own login · runs tools itself, not limited to the project folder', command: 'npx -y pi-acp@0.0.34', on: false },
    ];
    let changed = false;
    const before = JSON.stringify(this.ws.agents);
    this.ws.agents = this.ws.agents.filter((a) => a.id !== 'gemini');
    for (const d of defaults) {
      const have = this.ws.agents.find((a) => a.id === d.id);
      if (!have) { this.ws.agents.push({ ...d }); changed = true; continue; }
      if (d.command && have.command !== d.command) { have.command = d.command; changed = true; }
      if (!have.subscription && d.subscription) { have.subscription = d.subscription; changed = true; }
      if (d.via && have.via !== d.via) { have.via = d.via; changed = true; }
    }
    if (JSON.stringify(this.ws.agents) !== before) changed = true;
    if (changed) this.save();
  }

  // ---------- persistence ----------
  save(): void {
    writeJson(this.paths.harnessJson, this.ws);
    writeJson(this.paths.state, this.state);
  }

  audit(action: string, detail = '', actor = 'you'): void {
    const ev: AuditEvent = { at: Date.now(), actor, action, detail };
    appendLine(this.paths.audit, JSON.stringify(ev));
  }

  /** Org audit: each model call with model and token counts, no content (R-36). */
  private async auditProviderCall(chat: Chat): Promise<void> {
    try {
      const raw = await (await this.client()).messages(chat.sid) as any[];
      const last = raw.find((m) => m.type === 'assistant');
      const t = last?.tokens || {};
      const model = last?.model ? `${last.model.providerID ?? ''}/${last.model.modelID ?? last.model.id ?? ''}` : this.effectiveModel(chat.project, chat.model);
      this.audit('provider.call', `${model} · in ${t.input ?? 0} · out ${t.output ?? 0}${t.reasoning ? ' · reasoning ' + t.reasoning : ''}`, 'workspaceai');
    } catch { /* engine gone; nothing to log */ }
  }

  /**
   * Uploads new audit entries to the org (policy `auditUrl`), at most every `auditEveryMinutes`. Unless the policy
   * sets `logContent`, details of content-bearing events (prompts, chat titles) are withheld. Returns entries sent.
   */
  async uploadAudit(now = Date.now(), force = false): Promise<number> {
    const url = this.ws.kind === 'org' ? this.policy?.auditUrl : undefined;
    if (!url) return 0;
    const cursorFile = join(this.paths.data, 'audit-upload.json');
    const cur = readJson<{ sent: number; at: number }>(cursorFile, { sent: 0, at: 0 });
    if (!force && now - cur.at < (this.policy?.auditEveryMinutes ?? 15) * 60000) return 0;
    const lines = readText(this.paths.audit).trim().split('\n').filter(Boolean);
    const fresh = lines.slice(cur.sent).map((l) => { try { return JSON.parse(l) as AuditEvent; } catch { return null; } }).filter((e): e is AuditEvent => !!e);
    if (!fresh.length) { writeJson(cursorFile, { ...cur, at: now }); return 0; }
    const content = /^(chat\.|permission\.|artifact\.|fact\.)/;
    const events = fresh.map((e) => (this.policy?.logContent || !content.test(e.action) ? e : { ...e, detail: '' }));
    const r = await fetch(url, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ org: this.policy?.org, workspace: this.id, subject: this.ws.org?.subject, policyVersion: this.policy?.version, events }), signal: AbortSignal.timeout(15000) });
    if (!r.ok) throw new Error(`Audit upload failed: ${r.status}`);
    writeJson(cursorFile, { sent: lines.length, at: now });
    return events.length;
  }

  auditLog(limit = 200): AuditEvent[] {
    const lines = readText(this.paths.audit).trim().split('\n').filter(Boolean);
    return lines.slice(-limit).reverse().map((l) => { try { return JSON.parse(l) as AuditEvent; } catch { return { at: 0, actor: '?', action: l, detail: '' }; } });
  }

  snapshot(): WorkspaceSnapshot {
    return {
      workspace: this.ws,
      projects: this.state.projects.filter((p) => !p.archived),
      chats: this.state.chats.filter((c) => !c.archived).map(({ transcript: _t, ...c }) => c),
      artifacts: this.state.artifacts,
      facts: this.allFacts(),
      routines: this.state.routines,
      graphs: this.state.graphs,
      audit: this.auditLog(60),
      engine: this.status,
      policy: this.policy,
      chatActivity: this.chatActivity(),
    };
  }

  changed(): void { this.save(); this.hooks.emit({ type: 'snapshot', workspace: this.id }); }

  /** Chats with a turn in progress, and whether each is waiting on the user. Sent to every window with the snapshot. */
  private chatActivity(): Record<string, 'working' | 'waiting'> {
    const out: Record<string, 'working' | 'waiting'> = {};
    for (const id of this.live.keys()) out[id] = 'working';
    for (const sid of this.permSessions.values()) { const id = this.chatForSid(sid)?.id; if (id && out[id]) out[id] = 'waiting'; }
    for (const p of this.acpPerms.values()) if (out[p.chatId]) out[p.chatId] = 'waiting';
    return out;
  }
  private activityChanged(): void { this.hooks.emit({ type: 'snapshot', workspace: this.id }); }
  private setLive(chatId: string, turn: LiveTurn): void { const fresh = !this.live.has(chatId); this.live.set(chatId, turn); if (fresh) this.activityChanged(); }
  private dropLive(chatId: string): void {
    if (!this.live.delete(chatId)) return;
    // Windows drop a chat's permission cards when its turn ends; an entry left behind would show as waiting on the next turn.
    for (const [id, sid] of this.permSessions) if (this.chatForSid(sid)?.id === chatId) this.permSessions.delete(id);
    this.activityChanged();
  }
  emit(e: UiEvent): void { this.hooks.emit(e); }

  // ---------- engine (opencode) ----------
  configFile(): { json: Record<string, unknown>; env: Record<string, string> } {
    // The Playwright connector runs the bundled @playwright/mcp with Chromium and this workspace's own profile (R-41).
    const ws = { ...this.ws, mcp: this.ws.mcp.map((m) => (m.builtin === 'playwright' ? { ...m, command: [process.execPath, playwrightCli(), '--headless', ...chromiumArgs(), '--user-data-dir', this.paths.browserProfile] } : m)) };
    const { json, env } = buildOpencodeJson(ws, this.policy, this.hooks.extraMcp(this));
    if (containsLiteralSecret(json)) throw new Error('Refusing to write a literal secret into opencode.json');
    return { json, env };
  }

  writeConfig(): void {
    const { json } = this.configFile();
    writeJson(this.paths.opencodeJson, json);
  }

  /** Environment for this workspace's process group: base env, XDG isolation, keychain secrets. */
  engineEnv(): Record<string, string> {
    const { json, env } = this.configFile();
    const out: Record<string, string> = {
      ...baseEnv(),
      XDG_DATA_HOME: this.paths.xdgData, XDG_CONFIG_HOME: this.paths.xdgConfig, XDG_STATE_HOME: this.paths.xdgState, XDG_CACHE_HOME: this.paths.xdgCache,
      OPENCODE_CONFIG: this.paths.opencodeJson,
      ...env,
      ...this.hooks.extraEnv(this),
    };
    for (const name of referencedEnv(json)) {
      const v = keychain.get(this.id, name);
      if (v != null) out[name] = v;
    }
    // Org workspaces: per-process proxy allow-list for the endpoints the policy names (security › residency).
    if (this.ws.kind === 'org' && this.policy) out.NO_PROXY = this.policy.allowedEndpoints.join(',') + ',127.0.0.1,localhost';
    return out;
  }

  async client(): Promise<OpencodeClient> {
    this.touch();
    if (this.ws.engine.mode === 'remote') {
      if (this.remote) return this.remote;
      return this.connectRemote();
    }
    if (this.attached) {
      if (WorkspaceRuntime.pidAlive(this.attached.pid)) return this.attached.client;
      this.attached = null;
    }
    if (this.server?.proc) return this.server.client();
    if (this.starting) return this.starting;
    this.starting = this.startManaged().finally(() => { this.starting = null; });
    return this.starting;
  }

  private setStatus(s: Partial<EngineStatus>): void {
    this.status = { ...this.status, ...s };
    this.hooks.emit({ type: 'engine', workspace: this.id, status: this.status });
  }

  private engineLockPath(): string { return join(this.paths.data, 'opencode', 'runtime.json'); }

  private static pidAlive(pid: number): boolean {
    if (!pid || pid === process.pid) return false;
    try { process.kill(pid, 0); return true; } catch { return false; }
  }

  /** Reuse the engine another WorkspaceAI window/process already runs for this workspace instead of starting a second one. */
  private async attachExisting(): Promise<OpencodeClient | null> {
    const lock = readJson<{ pid: number; url: string } | null>(this.engineLockPath(), null);
    if (!lock?.pid || !lock.url || !WorkspaceRuntime.pidAlive(lock.pid)) return null;
    const password = keychain.get(this.id, 'OPENCODE_ENGINE_PASSWORD');
    if (!password) return null;
    const client = new OpencodeClient(lock.url, 'opencode', password);
    try {
      const info = await client.info();
      this.attached = { pid: lock.pid, url: lock.url, client };
      this.setStatus({ state: 'connected', url: lock.url, port: Number(new URL(lock.url).port) || 0, version: info.version, pid: lock.pid, sidecars: [] });
      this.audit('engine.attach', `sharing opencode ${info.version} on 127.0.0.1:${new URL(lock.url).port}`, 'workspaceai');
      this.startPump(client);
      return client;
    } catch {
      return null;
    }
  }

  private async startManaged(): Promise<OpencodeClient> {
    const shared = await this.attachExisting();
    if (shared) return shared;
    const extra = await this.hooks.beforeStart?.(this).catch(() => []) ?? [];
    this.writeConfig();
    this.setStatus({ state: 'starting', error: undefined });
    const server = new ManagedServer({
      binary: opencodeBinary(this.ws.engine),
      cwd: expandHome(this.state.projects.find((p) => p.isGeneral)?.folder || this.paths.root),
      env: this.engineEnv(),
      logDir: this.paths.logs,
      onExit: (code) => {
        if (this.server !== server) return;
        this.pump?.abort();
        if (code === 0 || this.status.state === 'stopped') { this.setStatus({ state: 'stopped', pid: undefined }); return; }
        const wait = server.nextBackoff();
        this.setStatus({ state: 'restarting', error: `opencode exited (${code}); restarting in ${wait / 1000}s` });
        setTimeout(() => { if (this.server === server) { this.server = null; this.client().catch((e) => this.setStatus({ state: 'error', error: String(e.message || e) })); } }, wait);
      },
    });
    this.server = server;
    try {
      const c = await server.start();
      this.attached = null;
      keychain.set(this.id, 'OPENCODE_ENGINE_PASSWORD', server.password);
      writeJson(this.engineLockPath(), { pid: server.proc?.pid, url: server.url });
      this.setStatus({ state: 'running', url: server.url, port: server.port, version: server.version, pid: server.proc?.pid, sidecars: [{ name: 'opencode serve', state: 'running', pid: server.proc?.pid }, ...extra] });
      this.audit('engine.start', `opencode ${server.version} on 127.0.0.1:${server.port}`, 'workspaceai');
      this.startPump(c);
      return c;
    } catch (e) {
      this.server = null;
      this.setStatus({ state: 'error', error: String((e as Error).message || e) });
      throw e;
    }
  }

  async connectRemote(): Promise<OpencodeClient> {
    const e = this.ws.engine;
    const ok = remoteUrlAllowed(e.url);
    if (!ok.ok) throw new Error(ok.reason);
    const pw = keychain.get(this.id, e.passwordSecret || 'OPENCODE_SERVER_PASSWORD') || '';
    const c = new OpencodeClient(e.url, e.username || 'opencode', pw);
    const info = await c.info();
    this.remote = c;
    this.setStatus({ state: 'connected', url: e.url, port: Number(new URL(e.url).port) || 0, version: info.version, pid: undefined, sidecars: [] });
    this.startPump(c);
    return c;
  }

  async stopEngine(): Promise<void> {
    this.pump?.abort(); this.pump = null;
    const s = this.server; this.server = null; this.remote = null;
    // A shared engine belongs to another WorkspaceAI process; detach from it instead of killing it.
    const shared = this.attached; this.attached = null;
    this.status.state = 'stopped';
    await this.closeAcp();
    if (s) {
      await s.stop();
      const lock = readJson<{ pid: number } | null>(this.engineLockPath(), null);
      if (lock) rmSync(this.engineLockPath(), { force: true });
      keychain.delete(this.id, 'OPENCODE_ENGINE_PASSWORD');
    }
    if (!shared) await this.hooks.afterStop?.(this).catch(() => {});
    this.setStatus({ state: 'stopped', pid: undefined, sidecars: [] });
  }

  private async closeAcp(): Promise<void> {
    const clients = Array.from(this.acpClients.values());
    this.acpClients.clear();
    this.acpLoaded.clear();
    this.acpChatBySession.clear();
    for (const p of this.acpPerms.values()) p.resolve(null);
    this.acpPerms.clear();
    await Promise.all(clients.map((c) => c.close().catch(() => {})));
  }

  /** Stop ACP children for these agent ids (used when a subscription is disconnected or changed). */
  async stopAgents(agentIds: string[]): Promise<void> {
    const ids = new Set(agentIds);
    const closing: Promise<void>[] = [];
    for (const [key, client] of Array.from(this.acpClients)) {
      const agentId = key.split('::')[0];
      if (!ids.has(agentId)) continue;
      this.acpClients.delete(key);
      for (const k of Array.from(this.acpLoaded)) if (k.startsWith(key + '\n')) this.acpLoaded.delete(k);
      for (const k of Array.from(this.acpChatBySession.keys())) if (k.startsWith(key + '\n')) this.acpChatBySession.delete(k);
      closing.push(client.close().catch(() => {}));
    }
    await Promise.all(closing);
  }

  async restartEngine(): Promise<void> {
    this.setStatus({ state: 'restarting' });
    await this.stopEngine();
    await this.client();
  }

  /** Stop the managed server after the configured idle time (lifecycle). */
  touch(): void {
    if (this.idleTimer) clearTimeout(this.idleTimer);
    const mins = this.ws.engine.idleStopMinutes;
    if (!mins || this.ws.engine.mode !== 'managed') return;
    this.idleTimer = setTimeout(() => { if (!this.live.size) this.stopEngine().catch(() => {}); }, mins * 60000);
    this.idleTimer.unref();
  }

  private startPump(c: OpencodeClient): void {
    this.pump?.abort();
    const ac = new AbortController();
    this.pump = ac;
    const loop = async () => {
      while (!ac.signal.aborted) {
        try { await c.events((e) => this.onEvent(e), ac.signal); } catch { /* reconnect */ }
        if (!ac.signal.aborted) await new Promise((r) => setTimeout(r, 1000));
      }
    };
    loop();
  }

  private chatBySid(sid: string): Chat | undefined { return this.state.chats.find((c) => c.sid === sid); }

  /** Chat that owns a session, including child (subagent) sessions already seen. */
  private chatForSid(sid: string): Chat | undefined {
    const direct = this.chatBySid(sid);
    if (direct) return direct;
    const cached = this.sidChats.get(sid);
    return cached ? this.state.chats.find((c) => c.id === cached) : undefined;
  }

  /** Walk a session's parent chain to find the chat it belongs to (opencode subagents run in child sessions). */
  private async resolveChatForSid(sid: string): Promise<Chat | undefined> {
    const known = this.chatForSid(sid);
    if (known) return known;
    try {
      const c = await this.client();
      const seen: string[] = [];
      let cur = sid;
      for (let i = 0; i < 8 && cur; i++) {
        seen.push(cur);
        const s = await c.getSession(cur);
        const chat = this.chatBySid(s.id) || (s.parentID ? this.chatBySid(s.parentID) : undefined);
        if (chat) { for (const x of seen) this.sidChats.set(x, chat.id); return chat; }
        if (!s.parentID) break;
        cur = s.parentID;
      }
    } catch { /* session may be gone */ }
    return undefined;
  }

  private permissionCard(req: OcPermission): { id: string; sid: string; title: string; detail: string } {
    const resources = (req.resources || []).map(String);
    const detail = resources.length ? resources.join('\n') : JSON.stringify(req.metadata || req.message || '').slice(0, 300);
    return { id: req.id, sid: req.sessionID, title: req.action || 'run a tool', detail: detail.slice(0, 600) };
  }

  private emitPermission(chat: Chat, e: OcEvent): void {
    const req = (e.data.request || e.data) as OcPermission;
    if (!req?.id) return;
    const owner = String(req.sessionID || e.data.sessionID || chat.sid);
    this.permSessions.set(req.id, owner);
    if (this.live.has(chat.id)) this.activityChanged();
    this.sidChats.set(owner, chat.id);
    // YOLO chats never surface a card: approve once so the turn keeps moving. The session ruleset
    // (set when the toggle flipped) normally avoids the ask altogether.
    if (chat.yolo) { void this.replyPermission(chat.id, req.id, 'once').catch(() => {}); return; }
    this.hooks.emit({ type: 'chat.permission', workspace: this.id, chat: chat.id, request: this.permissionCard({ ...req, sessionID: owner }) });
  }

  private clearPermission(chatId: string, requestId?: string): void {
    if (requestId) this.permSessions.delete(requestId);
    if (this.live.has(chatId)) this.activityChanged();
    this.hooks.emit({ type: 'chat.permission.clear', workspace: this.id, chat: chatId, requestId });
  }

  private clearPermissionsForChat(chatId: string): void {
    for (const [id, sid] of this.permSessions) if (this.chatForSid(sid)?.id === chatId) this.permSessions.delete(id);
    if (this.live.has(chatId)) this.activityChanged();
    this.hooks.emit({ type: 'chat.permission.clear', workspace: this.id, chat: chatId });
  }

  /** Pending opencode permissions for a chat (used when a chat is opened or the app reconnects). */
  async pendingPermissions(chatId: string): Promise<{ id: string; sid: string; title: string; detail: string }[]> {
    const chat = this.chat(chatId);
    if (!chat) return [];
    const cards = await this.collectPendingPermissions(chat);
    if (chat.yolo) {
      for (const p of cards) await this.replyPermission(chat.id, p.id, 'once').catch(() => {});
      return [];
    }
    return cards;
  }

  private async collectPendingPermissions(chat: Chat): Promise<{ id: string; sid: string; title: string; detail: string }[]> {
    if (chat.agent !== 'opencode') {
      const out: { id: string; sid: string; title: string; detail: string }[] = [];
      for (const [id, p] of this.acpPerms) if (p.chatId === chat.id) out.push({ id, sid: chat.sid, title: p.title, detail: p.detail });
      return out;
    }
    const c = await this.client();
    const folder = this.projectFolder(chat.project);
    const all = await c.pendingPermissions(folder).catch(async () => {
      const roots = await c.permissions(chat.sid).catch(() => [] as OcPermission[]);
      const children = await c.listSessions({ parentID: chat.sid, limit: 200 }).catch(() => [] as OcSession[]);
      const childPerms = (await Promise.all(children.map((s) => c.permissions(s.id).catch(() => [] as OcPermission[])))).flat();
      return [...roots, ...childPerms];
    });
    const out: { id: string; sid: string; title: string; detail: string }[] = [];
    for (const p of all) {
      if (p.sessionID !== chat.sid && this.sidChats.get(p.sessionID) !== chat.id) {
        const owner = await this.resolveChatForSid(p.sessionID).catch(() => undefined);
        if (!owner || owner.id !== chat.id) continue;
      }
      this.permSessions.set(p.id, p.sessionID);
      out.push(this.permissionCard(p));
    }
    return out;
  }

  /** Reply to a pending opencode permission, routing it to the session that actually owns it. */
  async replyPermission(chatId: string, requestId: string, decision: 'once' | 'always' | 'reject'): Promise<'ok' | 'expired'> {
    const chat = this.chat(chatId);
    if (!chat) throw new Error('Chat not found');
    if (chat.agent !== 'opencode') {
      if (!this.replyAcpPermission(chat.id, requestId, decision)) return 'expired';
      this.clearPermission(chat.id, requestId);
      return 'ok';
    }
    const c = await this.client();
    const folder = this.projectFolder(chat.project);
    let sid = this.permSessions.get(requestId);
    if (!sid) {
      const pending = await c.pendingPermissions(folder).catch(() => [] as OcPermission[]);
      sid = pending.find((p) => p.id === requestId)?.sessionID;
    }
    if (!sid) { this.clearPermission(chat.id, requestId); return 'expired'; }
    try {
      await c.replyPermission(sid, requestId, decision);
    } catch (e) {
      if (!(e instanceof OpencodeError) || e.status !== 404) throw e;
      // The cached owner was stale; look the request up once more before calling it expired.
      const pending = await c.pendingPermissions(folder).catch(() => [] as OcPermission[]);
      const owner = pending.find((p) => p.id === requestId)?.sessionID;
      if (!owner) { this.clearPermission(chat.id, requestId); return 'expired'; }
      await c.replyPermission(owner, requestId, decision);
    }
    this.permSessions.delete(requestId);
    this.clearPermission(chat.id, requestId);
    return 'ok';
  }

  private onEvent(e: OcEvent): void {
    const sid = (e.data?.sessionID as string) || '';
    if (e.type === 'session.created' && sid && this.ws.engine.sync && !this.chatBySid(sid) && !this.state.skippedSessions.includes(sid)) {
      // Two-way sync: a session started in the TUI appears in the right project (R-19).
      setTimeout(() => this.adoptSession(sid).catch(() => {}), 400);
      return;
    }
    const chat = sid ? this.chatForSid(sid) : undefined;
    if (!chat) {
      // Permission requests can be owned by a subagent child session. Map it back to its chat.
      if (e.type === 'permission.asked') void this.resolveChatForSid(sid).then((c) => { if (c) this.emitPermission(c, e); }).catch(() => {});
      return;
    }
    if (e.type === 'permission.asked') { this.emitPermission(chat, e); return; }
    if (e.type === 'permission.replied') {
      const requestID = String(e.data.requestID || e.data.id || '');
      const owner = this.permSessions.get(requestID);
      const ownerChat = owner ? this.chatForSid(owner) : chat;
      this.permSessions.delete(requestID);
      if (ownerChat) this.clearPermission(ownerChat.id, requestID);
      return;
    }
    let turn = this.live.get(chat.id);
    if (!turn && (e.type.startsWith('session.text') || e.type.startsWith('session.tool') || e.type === 'session.execution.started')) {
      turn = { chatId: chat.id, messageId: String(e.data.assistantMessageID || uid('msg')), text: '', tools: [], started: Date.now() };
      this.setLive(chat.id, turn);
    }
    if (!turn) return;
    if (e.type === 'session.text.delta') {
      const d = String(e.data.delta || '');
      turn.text += d;
      this.hooks.emit({ type: 'chat.delta', workspace: this.id, chat: chat.id, messageId: turn.messageId, delta: d });
    } else if (e.type === 'session.tool.input.started' || e.type === 'session.tool.called') {
      const id = String(e.data.id || '');
      const name = String(e.data.name || (turn.tools.find((t) => t.detail === id)?.name ?? 'tool'));
      let t = turn.tools.find((x) => x.detail === id);
      if (!t) { t = { name, detail: id, status: 'running' }; turn.tools.push(t); }
      if (e.data.input) { t.name = toolLabel(name, e.data.input); t.input = JSON.stringify(e.data.input); }
      if (/artifact|open-design|open_design/i.test(name) || /artifact_(create|update)|open-design/.test(String((e.data.input as { code?: string })?.code || ''))) { this.artifactCalls.push({ sid, at: Date.now(), tool: name }); this.artifactCalls = this.artifactCalls.filter((x) => Date.now() - x.at < 600000).slice(-50); }
      this.hooks.emit({ type: 'chat.tool', workspace: this.id, chat: chat.id, messageId: turn.messageId, tool: t });
    } else if (e.type === 'session.tool.input.ended') {
      const t = turn.tools.find((x) => x.detail === String(e.data.id));
      if (t && /artifact_(create|update)|open-design/.test(String(e.data.text || ''))) { this.artifactCalls.push({ sid, at: Date.now(), tool: 'execute' }); }
      if (t) { try { t.name = toolLabel(t.name.split(' ')[0], JSON.parse(String(e.data.text || '{}'))); t.input = String(e.data.text || '{}'); } catch { /* keep */ } this.hooks.emit({ type: 'chat.tool', workspace: this.id, chat: chat.id, messageId: turn.messageId, tool: t }); }
    } else if (e.type === 'session.tool.success' || e.type === 'session.tool.failed' || e.type === 'session.tool.error') {
      const t = turn.tools.find((x) => x.detail === String(e.data.id));
      if (t) { t.status = e.type === 'session.tool.success' ? 'ok' : 'error'; this.hooks.emit({ type: 'chat.tool', workspace: this.id, chat: chat.id, messageId: turn.messageId, tool: t }); }
    } else if (e.type === 'session.execution.succeeded' || e.type === 'session.execution.failed' || e.type === 'session.execution.interrupted') {
      this.dropLive(chat.id);
      if (this.ws.kind === 'org') void this.auditProviderCall(chat);
      // The engine drops pending permissions when a turn ends; do the same in the UI.
      this.clearPermissionsForChat(chat.id);
      chat.updatedAt = Date.now();
      this.save();
      this.hooks.emit({ type: 'chat.done', workspace: this.id, chat: chat.id, outcome: e.type.split('.').pop() || 'done' });
      this.touch();
    }
  }

  isStreaming(chatId: string): boolean { return this.live.has(chatId); }

  // ---------- subscription agents over ACP (R-06, R-23, R-39) ----------
  private projectFolder(pid: string): string {
    return expandHome(this.project(pid)?.folder || this.general().folder);
  }

  private vaultRules = new Map<string, string>();

  private acpKey(agentId: string, folder: string): string { return agentId + '::' + folder; }
  private piSignature = '';

  /**
   * The same MCP servers an opencode chat gets (memory, artifacts, Open Design, the workspace's connectors), in ACP's
   * `mcpServers` shape, with `{env:X}` references resolved from this workspace's environment and keychain only.
   */
  acpMcpServers(folder: string): unknown[] {
    const { json } = this.configFile();
    const env = this.engineEnv();
    const resolveRef = (v: unknown) => String(v ?? '').replace(/\{env:([A-Z0-9_]+)\}/gi, (_m, k) => env[k] ?? '');
    const out: unknown[] = [];
    for (const [name, raw] of Object.entries((json.mcp || {}) as Record<string, any>)) {
      if (!raw || raw.enabled === false) continue;
      const pairs = (o: Record<string, unknown> | undefined) => Object.entries(o || {}).map(([k, v]) => ({ name: k, value: resolveRef(v) }));
      if (raw.type === 'remote') out.push({ type: 'http', name, url: raw.url, headers: pairs(raw.headers) });
      else if (Array.isArray(raw.command) && raw.command.length) out.push({ name, command: raw.command[0], args: raw.command.slice(1), env: pairs(name === 'harness-vault' ? { ...raw.environment, HARNESS_PROJECT_FOLDER: folder } : raw.environment) });
    }
    return out;
  }

  private async acpFor(agentId: string, pid: string): Promise<AcpClient> {
    const folder = this.projectFolder(pid);
    const key = this.acpKey(agentId, folder);
    const pi = agentId === 'pi' ? piAgentDir(this) : null;
    // A changed provider or key only reaches Pi on a restart; never cut off a reply for it, the next message does it.
    if (pi && pi.signature !== this.piSignature && ![...this.live.values()].some((t) => this.chat(t.chatId)?.agent === 'pi')) {
      await this.stopAgents(['pi']);
      this.piSignature = pi.signature;
    }
    const existing = this.acpClients.get(key);
    if (existing?.running) return existing;
    const rec = this.ws.agents.find((a) => a.id === agentId);
    const command = acpCommandFor(rec || { id: agentId });
    const env = pi ? pi.env : rec?.subscription ? agentEnv(this, rec.subscription) : {};
    const client = new AcpClient({
      id: agentId,
      command,
      cwd: folder,
      env,
      onUpdate: (sid, update) => {
        if (update?.sessionUpdate === 'available_commands_update') { this.rememberAgentCommands(agentId, update.availableCommands); return; }
        this.onAcpUpdate(key, sid, update);
      },
      onPermission: (sid, req) => this.onAcpPermission(key, sid, req),
      onExit: (code) => {
        if (this.acpClients.get(key) !== client) return;
        this.acpClients.delete(key);
        for (const k of Array.from(this.acpLoaded)) if (k.startsWith(key + '\n')) this.acpLoaded.delete(k);
        const live = Array.from(this.live.values()).find((t) => {
          const c = this.chat(t.chatId);
          return c && c.agent === agentId && this.projectFolder(c.project) === folder;
        });
        if (live) this.hooks.emit({ type: 'toast', text: `${rec?.name || agentId} stopped${code === 0 ? '' : ' (exit ' + code + ')'}` });
      },
      log: (line) => appendLine(join(this.paths.logs, `acp-${agentId}.log`), `${new Date().toISOString()} ${line}`),
    });
    this.acpClients.set(key, client);
    await client.start();
    return client;
  }

  /** Claude Code's effort levels per model, read once per run from the CLI (the ACP adapter does not pass them on). */
  private claudeEfforts: Map<string, string[]> | null = null;
  private claudeEffortsLoading = false;

  private rememberAgentModels(agentId: string, client: AcpClient): void {
    if (!client.models.length) return;
    const map = (this.state.agentModels ||= {});
    let models = client.models;
    if (agentId === 'claude') {
      if (this.claudeEfforts) models = models.map((m) => { const e = this.claudeEfforts!.get(m.id); return e?.length ? { ...m, efforts: e } : m; });
      else if (!this.claudeEffortsLoading) {
        this.claudeEffortsLoading = true;
        const rec = this.ws.agents.find((a) => a.id === 'claude');
        claudeEffortCatalog(rec?.subscription ? agentEnv(this, rec.subscription) : {})
          .then((cat) => { this.claudeEfforts = cat; this.rememberAgentModels(agentId, client); })
          .finally(() => { this.claudeEffortsLoading = false; });
      }
    }
    const next = { models, current: client.defaultModel };
    if (JSON.stringify(map[agentId]) === JSON.stringify(next)) return;
    map[agentId] = next;
    this.save();
    this.hooks.emit({ type: 'models', workspace: this.id });
  }

  private rememberAgentCommands(agentId: string, list: unknown): void {
    if (!Array.isArray(list)) return;
    const next: NativeCommand[] = list.map((c: any) => ({ name: String(c?.name || ''), ...(c?.description ? { description: String(c.description) } : {}), ...(c?.input?.hint ? { hint: String(c.input.hint) } : {}) })).filter((c) => c.name);
    const map = (this.state.agentCommands ||= {});
    if (JSON.stringify(map[agentId]) === JSON.stringify(next)) return;
    map[agentId] = next;
    this.save();
  }

  /** The commands a chat's composer offers: the harness set mapped for its agent, then the agent's own. */
  async chatCommandList(chatId: string): Promise<ChatCommand[]> {
    const chat = this.chat(chatId); if (!chat) throw new Error('Chat not found');
    if (chat.agent === 'opencode' && (this.status.state === 'running' || this.status.state === 'connected')) {
      const list = await (await this.client()).commands().catch(() => null);
      if (list) { (this.state.agentCommands ||= {}).opencode = list.map((c) => ({ name: c.name, ...(c.description ? { description: c.description } : {}) })); }
    }
    const name = chat.agent === 'opencode' ? 'opencode' : this.ws.agents.find((a) => a.id === chat.agent)?.name || chat.agent;
    return chatCommands(chat.agent, name, this.state.agentCommands?.[chat.agent] || []);
  }

  async chatStatus(chatId: string): Promise<ChatStatus> {
    const chat = this.chat(chatId); if (!chat) throw new Error('Chat not found');
    const agentName = chat.agent === 'opencode' ? 'opencode' : this.ws.agents.find((a) => a.id === chat.agent)?.name || chat.agent;
    const model = chat.agent === 'opencode' ? chat.model || this.effectiveModel(chat.project) : chat.model || this.agentModels(chat.agent).current;
    const st: ChatStatus = {
      agent: chat.agent, agentName, model, effort: chat.effort || '', project: projectPathName(this.state.projects, chat.project), folder: this.projectFolder(chat.project),
      session: chat.sid, web: this.webEnabledFor(chat), fullAccess: !!chat.yolo, connectors: this.ws.mcp.filter((m) => m.on).length, engine: this.status.state,
    };
    if (chat.agent === 'opencode' && this.status.state !== 'stopped') {
      const s = await (await this.client()).getSession(chat.sid).catch(() => null) as any;
      if (s?.tokens) st.tokens = { input: s.tokens.input || 0, output: s.tokens.output || 0, reasoning: s.tokens.reasoning || 0, cacheRead: s.tokens.cache?.read || 0, cacheWrite: s.tokens.cache?.write || 0 };
      if (typeof s?.cost === 'number') st.cost = s.cost;
    }
    return st;
  }

  /**
   * Run a slash command typed in a chat. WorkspaceAI commands run as the agent's own equivalent (see `commands.ts`);
   * anything else goes to the agent as typed: Claude Code and Grok run their own commands, opencode its server commands.
   */
  async runCommand(chatId: string, text: string): Promise<CommandResult> {
    const chat = this.chat(chatId); if (!chat) throw new Error('Chat not found');
    const cmd = parseSlash(text); if (!cmd) throw new Error('Not a slash command');
    const withArgs = (c: string) => c + (cmd.args ? ' ' + cmd.args : '');
    const busy = this.isStreaming(chatId);
    const h = findHarnessCommand(cmd.name);
    const action = h ? commandAction(h, chat.agent) : null;
    if (h && !action) throw new Error(`/${h.name} has no equivalent in ${chat.agent === 'opencode' ? 'opencode' : this.ws.agents.find((a) => a.id === chat.agent)?.name || chat.agent}`);
    // Info commands answer in an action sheet: the harness's figures plus the agent's own output, run quietly.
    if (h?.sheet && action && (action.kind === 'native' || (action.kind === 'ui' && action.action === 'status'))) {
      const sheet: CommandSheet = { command: h.name, title: h.sheet };
      if (action.kind === 'ui' || chat.agent === 'opencode') sheet.status = await this.chatStatus(chatId);
      const native = action.kind === 'native' ? withArgs(action.command) : action.then;
      if (native && chat.agent !== 'opencode') {
        if (busy) sheet.note = `${this.agentName(chat)} is answering — its own ${native} shows once the answer finishes.`;
        else {
          sheet.ran = native;
          const [plan, own] = await Promise.all([
            // Claude Code's plan limits (session, week) come from its CLI; in a session it only reports that session's cost.
            chat.agent === 'claude' && h.name === 'usage' ? this.claudePlanUsage().catch(() => '') : Promise.resolve(''),
            this.quietCommand(chat, native).catch((e) => { sheet.note = (e as Error).message; return ''; }),
          ]);
          sheet.text = plan ? plan + (own ? '\n\n### This chat\n\n```\n' + own + '\n```' : '') : own;
        }
      }
      if (!sheet.status && !sheet.text) sheet.status = await this.chatStatus(chatId);
      return { ui: 'sheet', sheet };
    }
    if (action?.kind === 'ui') {
      if (action.action !== 'status') return { ui: action.action, args: cmd.args };
      return { ui: 'status', status: await this.chatStatus(chatId) };
    }
    if (busy) throw new Error('Wait for the current answer to finish, or stop it first');
    if (action?.kind === 'native') { await this.send(chatId, withArgs(action.command)); return { ran: withArgs(action.command) }; }
    if (action?.kind === 'oc-compact') { await this.send(chatId, '/compact', { oc: (c, sid) => c.compact(sid) }); return { ran: '/compact' }; }
    const ocName = action?.kind === 'oc-command' ? action.name : cmd.name;
    if (chat.agent === 'opencode') {
      const known = action?.kind === 'oc-command' || (this.state.agentCommands?.opencode || []).some((c) => c.name === ocName);
      if (!known) throw new Error(`opencode has no /${cmd.name} command — type /help for this chat’s commands`);
      await this.send(chatId, '/' + withArgs(ocName), { oc: (c, sid) => c.runCommand(sid, ocName, cmd.args) });
      return { ran: '/' + withArgs(ocName) };
    }
    await this.send(chatId, text.trim());
    return { ran: text.trim() };
  }

  /** Models an ACP agent offers, as it last reported them; empty until it has run one session here. */
  agentModels(agentId: string): { models: AcpModel[]; current: string } {
    return this.state.agentModels?.[agentId] || { models: [], current: '' };
  }

  private probedThisRun = new Set<string>();
  /** Ask each agent for its models once per run, so a subscription's new models show up without a chat first. */
  refreshAgentModels(agentId: string): void {
    if (this.probedThisRun.has(agentId) && this.agentModels(agentId).models.length) return;
    this.probedThisRun.add(agentId);
    this.probeAgentModels(agentId);
  }

  private engineModelsLoading = false;
  /**
   * Every model opencode serves for the connected providers, with reasoning variants. Read live while the engine
   * runs and cached; while it is stopped the cache answers, and an empty cache starts the engine once to fill it.
   */
  async engineModels(): Promise<EngineModel[]> {
    const read = async (c: OpencodeClient) => {
      const live = (await c.models()).filter((m) => m.enabled !== false && m.status !== 'deprecated').map((m): EngineModel => {
        const efforts = (m.variants || []).map((v) => v.id).filter(Boolean);
        return { id: m.id, providerID: m.providerID, name: m.name || m.id, ...(efforts.length ? { efforts } : {}) };
      });
      if (JSON.stringify(live) !== JSON.stringify(this.state.engineModels || [])) { this.state.engineModels = live; this.save(); }
      return live;
    };
    if (this.status.state === 'running' || this.status.state === 'connected') return read(await this.client()).catch(() => this.state.engineModels || []);
    if (!this.state.engineModels?.length && !this.engineModelsLoading) {
      this.engineModelsLoading = true;
      void this.client().then(read).then(() => this.hooks.emit({ type: 'models', workspace: this.id })).catch(() => {}).finally(() => { this.engineModelsLoading = false; });
    }
    return this.state.engineModels || [];
  }

  private probing = new Set<string>();
  private probeFailed = new Map<string, number>();
  /**
   * Learn an agent's models before its first chat: open one session in the General project and close it again.
   * No prompt is sent, so it costs nothing on the subscription. Runs in the background; emits `models` when done.
   */
  probeAgentModels(agentId: string): void {
    if (this.probing.has(agentId) || Date.now() - (this.probeFailed.get(agentId) || 0) < 300000) return;
    this.probing.add(agentId);
    void (async () => {
      try {
        const pid = this.general().id;
        const client = await this.acpFor(agentId, pid);
        const sid = await client.newSession({ cwd: this.projectFolder(pid), mcpServers: [] });
        this.rememberAgentModels(agentId, client);
        // Agents announce their slash commands just after the session opens.
        for (let i = 0; i < 20 && !this.state.agentCommands?.[agentId]; i++) await new Promise((r) => setTimeout(r, 100));
        await client.closeSession(sid);
      } catch (e) {
        this.probeFailed.set(agentId, Date.now());
        appendLine(join(this.paths.logs, `acp-${agentId}.log`), `${new Date().toISOString()} model probe failed: ${String((e as Error).message || e).slice(0, 400)}`);
      } finally { this.probing.delete(agentId); }
    })();
  }

  /**
   * Providers this workspace can actually reach: the key is in the keychain, the provider needs none
   * (OpenCode Zen, a local endpoint such as Ollama), or it is reused from the user's own opencode login.
   */
  connectedProviders(): Provider[] {
    const local = (u?: string) => !!u && /^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])(:|\/|$)/.test(u);
    return effectiveProviders(this.ws, this.policy).filter((p) => p.fromOpencode || !p.keyEnv || local(p.baseURL) || keychain.has(this.id, p.keyEnv));
  }

  private acpSessionId(chat: Chat): string | null {
    const prefix = `acp:${chat.agent}:`;
    return chat.sid.startsWith(prefix) ? chat.sid.slice(prefix.length) : null;
  }

  private async ensureAcpSession(chat: Chat): Promise<{ client: AcpClient; sessionId: string; key: string }> {
    const folder = this.projectFolder(chat.project);
    const client = await this.acpFor(chat.agent, chat.project);
    const key = this.acpKey(chat.agent, folder);
    const existing = this.acpSessionId(chat);
    const effort = chat.effort || '';
    if (existing && this.acpLoaded.has(key + '\n' + existing)) {
      // Claude Code takes effort only when a session opens: reopen it (the replayed history is not this turn's).
      if (chat.agent !== 'claude' || (client.sessionEffort.get(existing) ?? '') === effort) return { client, sessionId: existing, key };
      this.acpChatBySession.delete(key + '\n' + existing);
      try {
        await client.loadSession({ cwd: folder, sessionId: existing, mcpServers: this.acpMcpServers(folder), ...this.acpSessionMeta(chat, effort) });
      } finally { this.acpChatBySession.set(key + '\n' + existing, chat.id); }
      return { client, sessionId: existing, key };
    }
    if (existing) {
      try {
        const sid = await client.loadSession({ cwd: folder, sessionId: existing, mcpServers: this.acpMcpServers(folder), ...this.acpSessionMeta(chat, effort) });
        this.rememberAgentModels(chat.agent, client);
        this.acpLoaded.add(key + '\n' + sid);
        this.acpChatBySession.set(key + '\n' + sid, chat.id);
        return { client, sessionId: sid, key };
      } catch { /* agent cannot reload it; start a fresh session below */ }
    }
    const sid = await client.newSession({ cwd: folder, mcpServers: this.acpMcpServers(folder), ...this.acpSessionMeta(chat, effort) });
    this.rememberAgentModels(chat.agent, client);
    chat.sid = `acp:${chat.agent}:${sid}`;
    this.acpLoaded.add(key + '\n' + sid);
    this.acpChatBySession.set(key + '\n' + sid, chat.id);
    return { client, sessionId: sid, key };
  }

  /**
   * What an ACP session is opened with. Claude Code takes the harness instructions as a system prompt addition and
   * effort as a CLI flag, both through its adapter's `_meta`; other agents get the instructions with their first
   * message (see sendAcp) and effort as a config option.
   */
  private acpSessionMeta(chat: { agent: string; project: string; instructions?: string }, effort: string): { meta?: Record<string, unknown>; effort?: string } {
    if (chat.agent !== 'claude') return {};
    const append = this.inheritedPrefix(chat.project, chat.instructions || '');
    return { effort, meta: { systemPrompt: { append }, claudeCode: { options: effort ? { extraArgs: { effort } } : {} } } };
  }

  private static acpText(content: unknown): string {
    if (!content) return '';
    if (typeof content === 'string') return content;
    if (Array.isArray(content)) return content.map((c) => WorkspaceRuntime.acpText(c)).join('');
    const c = content as Record<string, unknown>;
    if (typeof c.text === 'string') return c.text;
    if (c.content) return WorkspaceRuntime.acpText(c.content);
    return '';
  }

  private onAcpUpdate(key: string, sessionId: string, update: Record<string, any>): void {
    const chatId = this.acpChatBySession.get(key + '\n' + sessionId);
    const chat = chatId ? this.chat(chatId) : undefined;
    if (!chat) return;
    const kind = String(update.sessionUpdate || update.type || '');
    const turn = this.live.get(chat.id);
    if (!turn) return;
    if (kind === 'agent_message_chunk') {
      const d = WorkspaceRuntime.acpText(update.content);
      if (!d) return;
      turn.text += d;
      if (!turn.quiet) this.hooks.emit({ type: 'chat.delta', workspace: this.id, chat: chat.id, messageId: turn.messageId, delta: d });
      return;
    }
    if (kind === 'tool_call' || kind === 'tool_call_update') {
      const id = String(update.toolCallId || update.id || '');
      if (!id) return;
      let t = turn.tools.find((x) => x.detail === id);
      if (!t) { t = { name: String(update.title || update.kind || 'tool'), detail: id, status: 'running' }; turn.tools.push(t); }
      if (update.title) t.name = String(update.title);
      const st = String(update.status || '');
      if (st === 'completed') t.status = 'ok';
      else if (st === 'failed' || st === 'cancelled') t.status = 'error';
      else if (st) t.status = 'running';
      if (update.rawInput && typeof update.rawInput === 'object') {
        try { t.input = JSON.stringify(update.rawInput).slice(0, 2000); } catch { /* ignore */ }
      }
      if (update.rawOutput && typeof update.rawOutput === 'object') {
        try { t.output = JSON.stringify(update.rawOutput).slice(0, 2000); } catch { /* ignore */ }
      }
      if (/artifact|open-design|open_design/i.test(t.name)) {
        this.artifactCalls.push({ sid: chat.sid, at: Date.now(), tool: t.name });
        this.artifactCalls = this.artifactCalls.filter((x) => Date.now() - x.at < 600000).slice(-50);
      }
      if (!turn.quiet) this.hooks.emit({ type: 'chat.tool', workspace: this.id, chat: chat.id, messageId: turn.messageId, tool: { ...t } });
    }
  }

  private onAcpPermission(key: string, sessionId: string, req: AcpPermissionRequest): Promise<string | null> {
    const chatId = this.acpChatBySession.get(key + '\n' + sessionId);
    if (!chatId) return Promise.resolve(null);
    const chat = this.chat(chatId);
    // YOLO chats, and workspaces that turned off "Ask before file writes and commands", auto-approve once; no card.
    // The harness's own memory and artifact tools never need asking, nor does reading a vault. Vault writes do.
    if (chat?.yolo || this.ws.acpAskFirst === false || /harness-(memory|artifacts)/.test(req.title) || /harness-vault[^a-z]*vault_(list|read|search)\b/i.test(req.title)) return Promise.resolve(WorkspaceRuntime.acpChoice(req.options, 'once')?.optionId || null);
    return new Promise((resolve) => {
      this.acpPerms.set(req.id, { chatId, options: req.options, title: req.title, detail: req.detail, resolve });
      this.activityChanged();
      this.hooks.emit({ type: 'chat.permission', workspace: this.id, chat: chatId, request: { id: req.id, sid: chat?.sid || '', title: req.title, detail: req.detail } });
    });
  }

  private static acpChoice(options: AcpPermissionRequest['options'], decision: string) {
    const wantsReject = decision === 'reject';
    const wantsAlways = decision === 'always';
    const byKind = (k: string) => options.find((o) => (o.kind || '').toLowerCase() === k);
    return wantsReject
      ? byKind('reject_always') || byKind('reject_once') || options.find((o) => /reject|deny|no/i.test(o.name))
      : wantsAlways
        ? byKind('allow_always') || options.find((o) => /always/i.test(o.name)) || byKind('allow_once') || options[0]
        : byKind('allow_once') || options.find((o) => /allow|approve|yes/i.test(o.name)) || options[0];
  }

  /** Settings/chat UI answers an ACP permission request. */
  replyAcpPermission(chatId: string, requestId: string, decision: string): boolean {
    const pending = this.acpPerms.get(requestId);
    if (!pending || pending.chatId !== chatId) return false;
    this.acpPerms.delete(requestId);
    this.activityChanged();
    const chosen = WorkspaceRuntime.acpChoice(pending.options, decision);
    pending.resolve(chosen?.optionId || null);
    return true;
  }

  private appendTranscript(chat: Chat, msg: ChatMessage): void {
    const list = chat.transcript ? [...chat.transcript] : [];
    list.push(msg);
    chat.transcript = list.slice(-400);
  }

  private async sendAcp(chat: Chat, text: string): Promise<void> {
    let opened: Awaited<ReturnType<WorkspaceRuntime['ensureAcpSession']>>;
    try { opened = await this.ensureAcpSession(chat); } catch (e) {
      // Signed out before the turn could start: the same sign-in prompt in the chat.
      const sub = this.ws.agents.find((a) => a.id === chat.agent)?.subscription;
      if (!sub || !isAuthFailure((e as Error).message)) throw e;
      void this.stopAgents([chat.agent]).catch(() => {});
      this.appendTranscript(chat, { id: uid('msg'), role: 'user', text, createdAt: Date.now() });
      this.appendTranscript(chat, { id: uid('msg'), role: 'assistant', text: '', createdAt: Date.now(), error: `${SUBSCRIPTIONS[sub].name} is signed out in this workspace.`, signIn: sub });
      chat.updatedAt = Date.now();
      this.changed();
      this.hooks.emit({ type: 'chat.done', workspace: this.id, chat: chat.id, outcome: 'signin' });
      return;
    }
    const { client, sessionId } = opened;
    if (chat.model) await client.setModel(sessionId, chat.model);
    if (chat.effort && chat.agent !== 'claude') await client.setEffort(sessionId, chat.effort);
    let body = text;
    // Agents other than Claude Code (which has them in its system prompt) get the harness instructions with the
    // first message of the chat; the transcript keeps only what the user typed.
    if (chat.agent !== 'claude' && !parseSlash(text) && !(chat.transcript || []).some((m) => m.role === 'user')) body = this.inheritedPrefix(chat.project, chat.instructions) + '\n\n---\n\n' + body;
    if (!parseSlash(text)) body += memoryRecall(this, chat.project, text) + this.takeGraphNotes(chat) + this.vaultNote(chat);
    // A slash command must reach the agent exactly as typed.
    if (!this.webEnabledFor(chat) && !parseSlash(text)) body += '\n\n[harness: web search is off for this chat — answer without searching the web, and say so if it matters.]';
    const turn: LiveTurn = { chatId: chat.id, messageId: uid('msg'), text: '', tools: [], started: Date.now() };
    this.setLive(chat.id, turn);
    this.appendTranscript(chat, { id: uid('msg'), role: 'user', text, createdAt: Date.now() });
    const slash = !!parseSlash(text);
    chat.updatedAt = Date.now();
    this.changed();
    void (async () => {
      try {
        const stop = await client.prompt(sessionId, body);
        if (slash && chat.agent === 'claude' && !turn.text.trim()) turn.text = this.claudeLocalCommandOutput(sessionId, turn.started);
        this.finishAcpTurn(chat, turn, stop === 'cancelled' ? 'interrupted' : 'done');
      } catch (e) {
        this.finishAcpTurn(chat, turn, 'failed', e as Error);
      }
    })();
  }

  /**
   * Output of a Claude Code local command (`/context`, `/usage`…) run in this session. The ACP adapter drops it
   * (Claude Code now reports it as a synthetic message), but Claude Code records it in the session transcript.
   */
  agentName(chat: Chat): string { return chat.agent === 'opencode' ? 'opencode' : this.ws.agents.find((a) => a.id === chat.agent)?.name || chat.agent; }

  /**
   * `claude -p /usage` with this workspace's Claude Code account: plan usage, no model call. Run from a workspace's
   * own config folder it can answer with the session cost only; then this computer's default Claude Code login is
   * asked instead — only when it is the same account, so a workspace never shows another account's usage.
   */
  async claudePlanUsage(): Promise<string> {
    const cli = findOnPath('claude');
    if (!cli) return '';
    const rec = this.ws.agents.find((a) => a.id === 'claude');
    const own = rec?.subscription ? agentEnv(this, rec.subscription) : {};
    const ask = (env: Record<string, string>) => new Promise<string>((res, rej) => execFile(cli, ['-p', '/usage'], { env: { ...baseEnv(), ...env }, cwd: tmpdir(), timeout: 60000, maxBuffer: 4 << 20 }, (err, out) => (err ? rej(err) : res(String(out).trim()))));
    const account = (file: string) => { try { return (JSON.parse(readFileSync(file, 'utf8')) as { oauthAccount?: { accountUuid?: string } }).oauthAccount?.accountUuid || ''; } catch { return ''; } };
    const sameAccount = () => { const mine = own.CLAUDE_CONFIG_DIR ? account(join(own.CLAUDE_CONFIG_DIR, '.claude.json')) : ''; return !!mine && mine === account(join(homedir(), '.claude.json')); };
    const { CLAUDE_CONFIG_DIR: _drop, ...viaDefault } = own;
    // Once the default login is known to answer for this account, ask it straight away (one run, not two).
    if (this.planUsageViaDefault && sameAccount()) { const r = await ask(viaDefault).catch(() => ''); if (/%\s*used/i.test(r)) return r; }
    const first = await ask(own);
    if (/%\s*used/i.test(first) || !own.CLAUDE_CONFIG_DIR || !sameAccount()) return first;
    const second = await ask(viaDefault).catch(() => '');
    if (/%\s*used/i.test(second)) { this.planUsageViaDefault = true; return second; }
    return first;
  }
  private planUsageViaDefault = false;

  /** Run an agent's own slash command for an action sheet: its output comes back here, not into the chat. */
  async quietCommand(chat: Chat, command: string): Promise<string> {
    const { client, sessionId } = await this.ensureAcpSession(chat);
    const turn: LiveTurn = { chatId: chat.id, messageId: uid('msg'), text: '', tools: [], started: Date.now(), quiet: true };
    this.setLive(chat.id, turn);
    try {
      await client.prompt(sessionId, command);
      if (!turn.text.trim() && chat.agent === 'claude') turn.text = this.claudeLocalCommandOutput(sessionId, turn.started);
      return turn.text.replace(/\x1b\[[0-9;]*m/g, '').trim();
    } finally { if (this.live.get(chat.id) === turn) this.dropLive(chat.id); }
  }

  private claudeLocalCommandOutput(sessionId: string, since: number): string {
    const rec = this.ws.agents.find((a) => a.id === 'claude');
    const projects = join(rec?.subscription ? agentHome(this, rec.subscription) : '', 'projects');
    try {
      for (const dir of readdirSync(projects)) {
        const file = join(projects, dir, sessionId + '.jsonl');
        if (!existsSync(file)) continue;
        let out = '';
        for (const line of readText(file).split('\n')) {
          if (!line.includes('"local_command"')) continue;
          const e = JSON.parse(line);
          if (e.subtype !== 'local_command' || Date.parse(e.timestamp || '') < since - 2000) continue;
          out = String(e.content || '').replace(/<\/?local-command-std(out|err)>/g, '').trim();
        }
        return out;
      }
    } catch { /* no transcript: nothing to show */ }
    return '';
  }

  private finishAcpTurn(chat: Chat, turn: LiveTurn, outcome: string, error?: Error): void {
    const current = this.live.get(chat.id);
    if (current !== turn) return;
    this.dropLive(chat.id);
    const live = this.chat(chat.id);
    if (!live) return;
    // A sign-in that expired: the chat offers to sign in again, and the agent restarts with the new login.
    const sub = this.ws.agents.find((a) => a.id === chat.agent)?.subscription;
    const signedOut = !!sub && (isAuthFailure(error?.message) || (turn.text.length < 400 && SIGNED_OUT_REPLY.test(turn.text)));
    if (signedOut) void this.stopAgents([chat.agent]).catch(() => {});
    if (error || signedOut) {
      this.appendTranscript(live, { id: uid('msg'), role: 'assistant', text: turn.text, tools: turn.tools, createdAt: Date.now(), error: signedOut ? `${SUBSCRIPTIONS[sub!].name} is signed out in this workspace.` : (error as Error).message || String(error), ...(signedOut ? { signIn: sub } : {}) });
    } else if (turn.text.trim() || turn.tools.length) {
      this.appendTranscript(live, { id: uid('msg'), role: 'assistant', text: turn.text, tools: turn.tools, createdAt: Date.now() });
    }
    live.updatedAt = Date.now();
    if (live.title === 'New chat') {
      const first = live.transcript?.find((m) => m.role === 'user')?.text || '';
      if (first) live.title = first.slice(0, 60);
    }
    this.audit('chat.prompt', `${live.title} · ${live.agent}`);
    this.save();
    this.hooks.emit({ type: 'chat.done', workspace: this.id, chat: chat.id, outcome: signedOut ? 'signin' : outcome });
    this.touch();
  }

  // ---------- projects (R-09, R-10, R-11) ----------
  project(id: string): Project | undefined { return this.state.projects.find((p) => p.id === id); }
  general(): Project { return this.state.projects.find((p) => p.isGeneral) as Project; }

  createProject(name: string, parent: string | null, folder?: string): Project {
    const clean = name.trim();
    if (!clean) throw new Error('Name the project');
    const par = parent ? this.project(parent) : undefined;
    if (parent && !par) throw new Error('Parent project not found');
    const seg = clean.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'project';
    const dir = folder ? resolve(expandHome(folder)) : par ? join(expandHome(par.folder), seg) : join(harnessHome(), 'projects', this.ws.slug, seg);
    mkdirSync(dir, { recursive: true });
    const p: Project = { id: uid('proj'), name: clean, parent: parent || null, folder: dir, inherit: true, model: '', archived: false, createdAt: Date.now() };
    this.state.projects.push(p);
    this.audit('project.create', projectPathName(this.state.projects, p.id));
    this.changed();
    return p;
  }

  updateProject(id: string, patch: Partial<Pick<Project, 'name' | 'inherit' | 'model' | 'parent' | 'vaults'>>): Project {
    const p = this.project(id);
    if (!p) throw new Error('Project not found');
    if (patch.parent !== undefined) {
      if (p.isGeneral) throw new Error('General stays at the top level');
      if (patch.parent === id || (patch.parent && descendants(this.state.projects, id).includes(patch.parent))) throw new Error('A project cannot move inside itself');
      p.parent = patch.parent;
    }
    if (patch.name !== undefined) { if (!patch.name.trim()) throw new Error('Name the project'); p.name = patch.name.trim(); }
    if (patch.inherit !== undefined) p.inherit = patch.inherit;
    if (patch.model !== undefined) p.model = patch.model;
    if (patch.vaults !== undefined) p.vaults = cleanAttachments(this.ws.vaults || [], patch.vaults);
    this.audit('project.update', p.name);
    this.changed();
    return p;
  }

  /** Archive hides the project, its sub-projects and their chats; nothing is deleted. */
  archiveProject(id: string): number {
    const p = this.project(id);
    if (!p) throw new Error('Project not found');
    if (p.isGeneral) throw new Error('General can’t be archived — quick chats live here.');
    const gone = [id, ...descendants(this.state.projects, id)];
    for (const x of this.state.projects) if (gone.includes(x.id)) x.archived = true;
    for (const c of this.state.chats) if (gone.includes(c.project)) c.archived = true;
    this.audit('project.archive', p.name);
    this.changed();
    return gone.length - 1;
  }

  restoreProject(id: string): void {
    const gone = [id, ...this.state.projects.filter((x) => projectPath(this.state.projects, x.id).some((y) => y.id === id)).map((x) => x.id)];
    for (const x of this.state.projects) if (gone.includes(x.id)) x.archived = false;
    for (const c of this.state.chats) if (gone.includes(c.project)) c.archived = false;
    this.changed();
  }

  /**
   * Deletes the project and its whole sub-tree (archived parts too) with their chats, artifacts, graphs, routines,
   * facts and maps, files included. The folders and anything else in them, AGENTS.md too, stay on disk.
   */
  async deleteProject(id: string): Promise<void> {
    const p = this.project(id);
    if (!p) throw new Error('Project not found');
    if (p.isGeneral) throw new Error('General can’t be deleted');
    // Not descendants(): it skips archived projects, which would orphan an archived sub-tree.
    const doomed = this.state.projects.filter((x) => isWithin(this.state.projects, x.id, id));
    const ids = new Set(doomed.map((x) => x.id));
    const inTree = <T extends { project: string }>(xs: T[]): T[] => xs.filter((x) => ids.has(x.project));
    // Best effort: an unreachable engine must not block the delete.
    for (const c of inTree(this.state.chats)) await this.deleteChat(c.id).catch(() => {});
    this.state.chats = this.state.chats.filter((c) => !ids.has(c.project));
    for (const r of inTree(this.state.routines)) this.deleteRoutine(r.id);
    for (const g of inTree(this.state.graphs)) this.deleteGraph(g.id);
    for (const a of inTree(this.state.artifacts)) this.artifacts.remove(a.id);
    // Named files, never all of .harness: a project at ~ would have ~/.harness, the whole WorkspaceAI home, there.
    const survivors = this.state.projects.filter((x) => !ids.has(x.id));
    for (const x of doomed) {
      stopDev(`${this.id}:${x.id}`);
      const folder = expandHome(x.folder);
      if (survivors.some((s) => expandHome(s.folder) === folder)) continue; // another project shares these
      rmSync(join(folder, '.harness', 'memory', 'facts.md'), { force: true });
      rmSync(join(folder, '.harness', 'map.json'), { force: true });
    }
    this.state.projects = survivors;
    if (ids.has(this.ws.defaultProject)) this.ws.defaultProject = this.general().id;
    this.audit('project.delete', `${p.name} (+${ids.size - 1} sub-projects)`);
    this.changed();
  }

  agentsFile(pid: string): string { const p = this.project(pid); if (!p) throw new Error('Project not found'); return join(expandHome(p.folder), 'AGENTS.md'); }
  readAgents(pid: string): string { return readText(this.agentsFile(pid)); }
  writeAgents(pid: string, text: string): void {
    writeText(this.agentsFile(pid), text);
    this.audit('instructions.save', this.agentsFile(pid));
    this.changed();
    // Chats in sub-projects carry this file as an inherited layer; refresh theirs.
    if (this.status.state === 'running' || this.status.state === 'connected') {
      for (const c of this.state.chats) if (c.agent === 'opencode' && !c.archived && c.project !== pid && projectPath(this.state.projects, c.project).some((p) => p.id === pid)) this.pushInstructions(c.sid, c.project, c.instructions).catch(() => {});
    }
  }

  /** Broad → narrow: workspace, every parent's AGENTS.md (when inheriting), then the project's own (R-12). */
  instructionLayers(pid: string): { name: string; file: string; text: string; kind: 'workspace' | 'parent' | 'project' }[] {
    const chain = projectPath(this.state.projects, pid);
    const own = chain[chain.length - 1];
    const layers: { name: string; file: string; text: string; kind: 'workspace' | 'parent' | 'project' }[] = [{ name: 'Workspace · ' + this.ws.name, file: 'workspace settings', text: this.ws.instructions, kind: 'workspace' }];
    if (own?.inherit !== false) for (const p of chain.slice(0, -1)) layers.push({ name: p.name, file: join(expandHome(p.folder), 'AGENTS.md'), text: readText(join(expandHome(p.folder), 'AGENTS.md')), kind: 'parent' });
    if (own) layers.push({ name: own.name, file: join(expandHome(own.folder), 'AGENTS.md'), text: readText(join(expandHome(own.folder), 'AGENTS.md')), kind: 'project' });
    return layers;
  }

  /** Inherited layers sent to the session; the project's own AGENTS.md is read by opencode itself from the folder. */
  inheritedPrefix(pid: string, chatInstructions = ''): string {
    const layers = this.instructionLayers(pid).filter((l) => l.kind !== 'project' && l.text.trim());
    const parts = layers.map((l) => `## ${l.kind === 'workspace' ? 'Workspace instructions' : 'Inherited from ' + l.name + ' (' + l.file + ')'}\n${l.text.trim()}`);
    if (chatInstructions.trim()) parts.push('## Chat instructions\n' + chatInstructions.trim());
    // Every agent is told how artifacts are made (all through Open Design), to use memory, and to use lean-ctx when on.
    const memory = this.ws.useMemory === false ? [] : [memoryRule(this.ws.memory), memoryBrief(this, pid)].filter(Boolean);
    parts.unshift(FILE_REF_RULE, ARTIFACT_RULE, GRAPH_RULE, MAP_RULE, ...memory, ...(this.ws.leanCtx?.enabled !== false && leanCtxBin() ? [LEANCTX_RULE] : []), ...[vaultRule(this, pid)].filter(Boolean));
    return 'WorkspaceAI instructions, broad to narrow. The project folder\'s own AGENTS.md is narrower and wins on conflict.\n\n' + parts.join('\n\n');
  }

  effectiveModel(pid: string, chatModel = ''): string {
    if (chatModel) return chatModel;
    const chain = projectPath(this.state.projects, pid).reverse();
    for (const p of chain) if (p.model) return p.model;
    return this.ws.defaultModel;
  }

  // ---------- chats (R-16) ----------
  chat(id: string): Chat | undefined { return this.state.chats.find((c) => c.id === id); }

  /** Sessions the harness itself is creating; two-way sync must not adopt them as TUI sessions (R-19). */
  private ownSids = new Set<string>();
  private inflight = 0;
  async owning<T>(fn: () => Promise<T>): Promise<T> { this.inflight++; try { return await fn(); } finally { this.inflight--; } }
  markOwn(sid: string): void { this.ownSids.add(sid); }

  async createChat(o: { project?: string; title: string; agent?: string; model?: string; effort?: string; web?: boolean | null; instructions?: string }): Promise<Chat> {
    return this.owning(() => this.createChatInner(o));
  }

  private async createChatInner(o: { project?: string; title: string; agent?: string; model?: string; effort?: string; web?: boolean | null; instructions?: string }): Promise<Chat> {
    const pid = o.project && this.project(o.project) ? o.project : (this.ws.defaultProject && this.project(this.ws.defaultProject) ? this.ws.defaultProject : this.general().id);
    const proj = this.project(pid) as Project;
    const on = (id?: string) => !!id && (id === 'opencode' || this.ws.agents.some((a) => a.id === id && a.on));
    if (o.agent && !on(o.agent)) throw new Error(`${this.ws.agents.find((a) => a.id === o.agent)?.name || o.agent} is off in this workspace — connect it in Settings › Model providers`);
    const agent = o.agent || (on(this.ws.defaultAgent) ? this.ws.defaultAgent : 'opencode');
    const model = agent === 'opencode' ? this.effectiveModel(pid, o.model) : '';
    const title = (o.title || 'New chat').slice(0, 80);
    const chatId = uid('chat');
    let sid = '';
    let chatModel = o.model || '';
    if (agent === 'opencode') {
      const c = await this.client();
      const s = await c.createSession({ title, directory: expandHome(proj.folder), model, variant: o.effort || undefined });
      sid = s.id;
      this.markOwn(sid);
      await this.pushInstructions(sid, pid, o.instructions || '');
    } else {
      const client = await this.acpFor(agent, pid);
      const folder = this.projectFolder(pid);
      const acpSid = await client.newSession({ cwd: folder, mcpServers: this.acpMcpServers(folder), ...this.acpSessionMeta({ agent, project: pid, instructions: o.instructions }, o.effort || '') });
      this.rememberAgentModels(agent, client);
      // With no pick, Pi runs the project's or workspace's model when it offers it (its own providers may not).
      const inherited = agent === 'pi' && !o.model ? this.effectiveModel(pid) : '';
      if (client.models.some((m) => m.id === inherited)) chatModel = inherited;
      if (chatModel) await client.setModel(acpSid, chatModel);
      if (o.effort && agent !== 'claude') await client.setEffort(acpSid, o.effort);
      sid = `acp:${agent}:${acpSid}`;
      const key = this.acpKey(agent, folder);
      this.acpLoaded.add(key + '\n' + acpSid);
      this.acpChatBySession.set(key + '\n' + acpSid, chatId);
    }
    const chat: Chat = { id: chatId, project: pid, title, sid, agent, model: chatModel, ...(o.effort ? { effort: o.effort } : {}), web: o.web ?? null, instructions: o.instructions || '', imported: false, archived: false, createdAt: Date.now(), updatedAt: Date.now(), transcript: [] };
    this.state.chats.push(chat);
    this.audit('chat.create', `${title} · ${projectPathName(this.state.projects, pid)} · ${sid}`);
    this.changed();
    return chat;
  }

  /**
   * Inherited layers (workspace instructions, every parent's AGENTS.md, chat instructions) go into the session's
   * instruction entries, so they shape every turn without appearing as a message. The project's own AGENTS.md is
   * read by opencode from the folder (R-12).
   */
  private instructionsPushed = new Set<string>();
  async pushInstructions(sid: string, pid: string, chatInstructions: string): Promise<void> {
    const c = await this.client();
    const prefix = this.inheritedPrefix(pid, chatInstructions);
    await c.req('PUT', `/api/experimental/session/${sid}/instructions/entries/harness`, { value: prefix || '' }).catch((e) => this.audit('instructions.error', String((e as Error).message).slice(0, 200), 'workspaceai'));
  }

  webEnabledFor(chat: Chat): boolean { return this.effectiveWebMode() !== 'off' && chat.web !== false; }
  effectiveWebMode() { return this.ws.kind === 'org' && this.policy ? this.policy.web.mode : this.ws.web.mode; }

  /** `o.oc` replaces the prompt for opencode chats (a server command or a compaction instead of a message). */
  async send(chatId: string, text: string, o: { oc?: (c: OpencodeClient, sid: string) => Promise<unknown> } = {}): Promise<void> {
    const chat = this.chat(chatId);
    if (!chat) throw new Error('Chat not found');
    if (!text.trim()) throw new Error('Type a message first');
    if (chat.agent !== 'opencode') return this.sendAcp(chat, text);
    const c = await this.client();
    // Chats made before the current instructions (e.g. the artifact rule) get them before their next turn.
    if (!this.instructionsPushed.has(chat.id)) { this.instructionsPushed.add(chat.id); await this.pushInstructions(chat.sid, chat.project, chat.instructions); }
    let body = text;
    if (!this.webEnabledFor(chat) && !parseSlash(text)) body += '\n\n[harness: web search is off for this chat — answer without searching the web, and say so if it matters.]';
    if (!parseSlash(text) && !o.oc) body += memoryRecall(this, chat.project, text) + this.takeGraphNotes(chat) + this.vaultNote(chat);
    this.setLive(chat.id, { chatId: chat.id, messageId: uid('msg'), text: '', tools: [], started: Date.now() });
    if (o.oc) await o.oc(c, chat.sid); else await c.prompt(chat.sid, body);
    chat.updatedAt = Date.now();
    if (chat.title === 'New chat' && !parseSlash(text)) chat.title = text.slice(0, 60);
    this.audit('chat.prompt', `${chat.title} · ${chat.model || this.effectiveModel(chat.project)}${this.ws.kind === 'org' && this.policy?.logContent ? ' · ' + text.slice(0, 200) : ''}`);
    this.changed();
  }

  /** Send a prompt and wait until its turn is over; returns the reply text (graph steps, the artifact pipeline). */
  async askAndWait(chatId: string, text: string, timeoutMs = 600000): Promise<string> {
    const chat = this.chat(chatId);
    if (!chat) throw new Error('Chat not found');
    const sentAt = Date.now();
    await this.send(chatId, text);
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, 700));
      if (this.isStreaming(chatId)) continue;
      // opencode chats read the session; ACP chats read the transcript the harness keeps for them.
      const all = chat.agent === 'opencode' ? mapOcMessages(await (await this.client()).messages(chat.sid)) : (chat.transcript || []);
      const reply = all.filter((m) => m.role === 'assistant' && m.createdAt >= sentAt - 1000).pop();
      if (reply?.error) throw new Error(reply.error);
      if (reply?.text?.trim()) return reply.text;
    }
    throw new Error('The agent did not answer in time');
  }

  async stop(chatId: string): Promise<void> {
    const chat = this.chat(chatId);
    if (!chat) return;
    if (chat.agent !== 'opencode') {
      const sid = this.acpSessionId(chat);
      const client = this.acpClients.get(this.acpKey(chat.agent, this.projectFolder(chat.project)));
      if (sid && client?.running) await client.cancel(sid).catch(() => {});
      const turn = this.live.get(chatId);
      if (turn) this.finishAcpTurn(chat, turn, 'interrupted');
      else this.hooks.emit({ type: 'chat.done', workspace: this.id, chat: chatId, outcome: 'interrupted' });
      return;
    }
    const c = await this.client();
    await c.interrupt(chat.sid).catch(() => {});
    this.dropLive(chatId);
    this.hooks.emit({ type: 'chat.done', workspace: this.id, chat: chatId, outcome: 'interrupted' });
  }

  async messages(chatId: string): Promise<ChatMessage[]> {
    const chat = this.chat(chatId);
    if (!chat) throw new Error('Chat not found');
    if (chat.agent !== 'opencode') return chat.transcript || [];
    const c = await this.client();
    const raw = await c.messages(chat.sid);
    const msgs = mapOcMessages(raw).filter((m) => !(m.role === 'user' && /^(WorkspaceAI|Harness) instructions, broad to narrow/.test(m.text)));
    for (const m of msgs) {
      if (m.role === 'user') m.text = stripRecall(m.text).replace(/\n\n\[harness: web search is off[^\]]*\]$/, '');
      if (m.role === 'assistant') {
        m.sources = this.ws.web.showSources ? sourcesFromTools(m.tools) : [];
        const a = this.state.artifacts.find((x) => x.chat === chat.id && x.createdAt >= m.createdAt - 1000 && x.createdAt <= m.createdAt + 600000);
        if (a) m.artifact = a.id;
      }
    }
    // Graph runs started from this chat are the harness's own messages; they sit between the session's turns.
    const runs = (chat.transcript || []).filter((m) => m.run);
    return runs.length ? [...msgs, ...runs].sort((a, b) => a.createdAt - b.createdAt) : msgs;
  }

  async renameChat(chatId: string, title: string): Promise<void> {
    const chat = this.chat(chatId); if (!chat) throw new Error('Chat not found');
    const t = title.trim() || chat.title;
    if (chat.agent === 'opencode') await (await this.client()).renameSession(chat.sid, t);
    chat.title = t;
    this.audit('chat.rename', t);
    this.changed();
  }

  async forkChat(chatId: string): Promise<Chat> { return this.owning(() => this.forkChatInner(chatId)); }

  private async forkChatInner(chatId: string): Promise<Chat> {
    const chat = this.chat(chatId); if (!chat) throw new Error('Chat not found');
    let sid = `acp:${chat.agent}:${uid('s')}`;
    if (chat.agent === 'opencode') {
      const c = await this.client();
      try { sid = (await c.forkSession(chat.sid)).id; this.markOwn(sid); }
      catch (e) {
        // opencode refuses to fork an empty session; a fork of nothing is a fresh session in the same project.
        if (!/empty_session|empty session/i.test(String((e as Error).message))) throw e;
        const proj = this.project(chat.project) as Project;
        sid = (await c.createSession({ title: chat.title + ' (fork)', directory: expandHome(proj.folder), model: this.effectiveModel(chat.project, chat.model) })).id;
        this.markOwn(sid);
      }
    }
    const fork: Chat = { ...chat, id: uid('chat'), sid, title: chat.title + ' (fork)', imported: false, createdAt: Date.now(), updatedAt: Date.now(), transcript: chat.transcript ? [...chat.transcript] : [] };
    this.state.chats.push(fork);
    this.audit('chat.fork', `${chat.sid} → ${sid}`);
    this.changed();
    return fork;
  }

  async deleteChat(chatId: string): Promise<void> {
    const chat = this.chat(chatId); if (!chat) throw new Error('Chat not found');
    if (chat.agent === 'opencode') await (await this.client()).deleteSession(chat.sid).catch(() => {});
    else {
      const sid = this.acpSessionId(chat);
      const key = this.acpKey(chat.agent, this.projectFolder(chat.project));
      const client = this.acpClients.get(key);
      if (sid && client?.running) await client.closeSession(sid).catch(() => {});
      if (sid) { this.acpLoaded.delete(key + '\n' + sid); this.acpChatBySession.delete(key + '\n' + sid); }
      this.dropLive(chatId);
    }
    this.state.chats = this.state.chats.filter((c) => c.id !== chatId);
    this.audit('chat.delete', chat.title);
    this.changed();
  }

  /** Moving a chat moves the artifacts it made (R-13). A chat can be moved but never detached (R-10). */
  moveChat(chatId: string, project: string): number {
    const chat = this.chat(chatId); if (!chat) throw new Error('Chat not found');
    if (!this.project(project)) throw new Error('Pick a project — a chat always belongs to one');
    const from = chat.project;
    chat.project = project;
    const moved: string[] = [];
    for (const a of this.state.artifacts) if (a.chat === chatId && a.project === from) { a.project = project; moved.push(a.id); }
    this.artifacts.relocate(moved, from);
    const n = moved.length;
    this.audit('chat.move', `${chat.title} → ${projectPathName(this.state.projects, project)} (${n} artifacts)`);
    this.changed();
    if (chat.agent === 'opencode' && this.status.state !== 'stopped') this.pushInstructions(chat.sid, project, chat.instructions).catch(() => {});
    return n;
  }

  updateChat(chatId: string, patch: Partial<Pick<Chat, 'web' | 'model' | 'effort' | 'agent' | 'instructions'>>): Chat {
    const chat = this.chat(chatId); if (!chat) throw new Error('Chat not found');
    Object.assign(chat, patch);
    // ACP chats pick up model and effort on their next turn (see sendAcp); opencode sessions switch now.
    if ((patch.model || patch.effort !== undefined) && chat.agent === 'opencode') {
      const model = chat.model || this.effectiveModel(chat.project);
      if (model) this.client().then((c) => c.switchModel(chat.sid, model, chat.effort || undefined)).catch(() => {});
    }
    if (patch.instructions !== undefined && chat.agent === 'opencode') this.pushInstructions(chat.sid, chat.project, chat.instructions).catch(() => {});
    this.changed();
    return chat;
  }

  /**
   * Full-permissions ("YOLO") mode for one chat. opencode gets a session ruleset that allows every
   * tool action; ACP agents get their permission requests auto-approved. Either way the flag is
   * enforced in the supervisor too, so a missed ruleset still cannot leave a card waiting.
   */
  async setChatYolo(chatId: string, yolo: boolean): Promise<Chat> {
    const chat = this.chat(chatId); if (!chat) throw new Error('Chat not found');
    chat.yolo = yolo;
    chat.updatedAt = Date.now();
    if (chat.agent === 'opencode') {
      try {
        const c = await this.client();
        await c.setSessionPermissions(chat.sid, yolo ? ALLOW_ALL : []);
      } catch (e) {
        this.audit('chat.yolo.error', String((e as Error).message || e).slice(0, 200), 'workspaceai');
      }
      // Settle anything already waiting before the ruleset applied.
      if (yolo) await this.pendingPermissions(chat.id).catch(() => []);
    } else if (yolo) {
      for (const [id, p] of Array.from(this.acpPerms)) if (p.chatId === chat.id) this.replyAcpPermission(chat.id, id, 'once');
    }
    if (yolo) this.clearPermissionsForChat(chat.id);
    this.audit('chat.yolo', `${chat.title} · ${yolo ? 'full permissions' : 'ask first'}`);
    this.changed();
    return chat;
  }

  /** Session started outside harness (TUI): put it in the project whose folder contains it, creating projects as needed. */
  async adoptSession(sid: string): Promise<Chat | null> {
    // Let any harness-side create/fork/import finish first, then re-check whose session this is.
    for (let i = 0; this.inflight > 0 && i < 100; i++) await new Promise((r) => setTimeout(r, 100));
    if (this.chatBySid(sid) || this.ownSids.has(sid)) return null;
    const c = await this.client();
    const s = await c.getSession(sid);
    if (s.parentID || this.chatBySid(sid) || this.ownSids.has(sid)) return null;
    const chat = this.chatFromSession(s, false);
    this.changed();
    this.hooks.emit({ type: 'toast', text: `New session from the TUI: ${chat.title}` });
    return chat;
  }

  projectForFolder(dir: string): Project {
    const abs = resolve(expandHome(dir));
    const hits = this.state.projects.filter((p) => !p.archived && (abs === resolve(expandHome(p.folder)) || abs.startsWith(resolve(expandHome(p.folder)) + '/')));
    hits.sort((a, b) => b.folder.length - a.folder.length);
    if (hits[0] && resolve(expandHome(hits[0].folder)) === abs) return hits[0];
    // Create a project for this folder, nested under the deepest containing project (or top level).
    const parent = hits[0] && !hits[0].isGeneral ? hits[0] : null;
    const p: Project = { id: uid('proj'), name: titleCaseSegment(abs), parent: parent ? parent.id : null, folder: abs, inherit: true, model: '', archived: false, createdAt: Date.now() };
    this.state.projects.push(p);
    return p;
  }

  chatFromSession(s: OcSession, imported: boolean): Chat {
    const dir = s.location?.directory || this.general().folder;
    const proj = this.projectForFolder(dir);
    const chat: Chat = { id: uid('chat'), project: proj.id, title: s.title || 'Session ' + s.id.slice(-6), sid: s.id, agent: 'opencode', model: s.model ? `${s.model.providerID}/${s.model.id}` : '', web: false, instructions: '', imported, archived: false, createdAt: s.time?.created || Date.now(), updatedAt: s.time?.updated || Date.now() };
    this.state.chats.push(chat);
    return chat;
  }

  /** Import sessions (from this workspace's server, or copied from another server) — folders become nested projects (R-18). */
  importSessions(sessions: OcSession[]): { chats: number; projects: number } {
    const before = this.state.projects.length;
    for (const { dir } of nestFolders(sessions.map((s) => s.location?.directory || '').filter(Boolean))) this.projectForFolder(dir);
    let n = 0;
    for (const s of sessions) if (!this.chatBySid(s.id)) { this.chatFromSession(s, true); n++; }
    this.audit('sessions.import', `${n} chats`);
    this.changed();
    return { chats: n, projects: this.state.projects.length - before };
  }

  async scanSessions(): Promise<OcSession[]> {
    const c = await this.client();
    const all = await c.listSessions();
    return all.filter((s) => !s.parentID && !this.chatBySid(s.id) && !s.time.archived);
  }

  attachCommand(chatId?: string): string {
    const chat = chatId ? this.chat(chatId) : undefined;
    const url = this.ws.engine.mode === 'remote' ? this.ws.engine.url : this.status.url || 'http://127.0.0.1:<port>';
    const pw = this.ws.engine.mode === 'managed' ? `OPENCODE_SERVER_PASSWORD='${this.server?.password ?? ''}' ` : '';
    const proj = chat ? this.project(chat.project) : undefined;
    return `${pw}opencode --server ${url}${chat ? ' --session ' + chat.sid : ''}${proj ? ' ' + expandHome(proj.folder) : ''}`;
  }

  // ---------- memory: facts (R-22) ----------
  factsFile(scope: string): string {
    if (scope === 'ws') return join(this.paths.memory, 'facts.md');
    const p = this.project(scope); if (!p) throw new Error('Project not found');
    return join(expandHome(p.folder), '.harness', 'memory', 'facts.md');
  }

  scopeFacts(scope: string): Fact[] { try { return parseFacts(readText(this.factsFile(scope)), scope); } catch { return []; } }

  allFacts(): Fact[] {
    const out = this.scopeFacts('ws');
    for (const p of this.state.projects) if (!p.archived) out.push(...this.scopeFacts(p.id));
    return out;
  }

  private writeScope(scope: string, facts: Fact[]): void {
    const title = scope === 'ws' ? `${this.ws.name} — workspace memory` : `${this.project(scope)?.name} — project memory`;
    writeText(this.factsFile(scope), serializeFacts(facts, title));
    this.writeScopesManifest();
  }

  /** The memory MCP server reads this to find project scopes without talking to the supervisor. */
  writeScopesManifest(): void {
    writeJson(join(this.paths.memory, 'scopes.json'), { workspace: this.ws.name, scopes: [{ id: 'ws', name: 'Workspace', file: join(this.paths.memory, 'facts.md') }, ...this.state.projects.filter((p) => !p.archived).map((p) => ({ id: p.id, name: projectPathName(this.state.projects, p.id), folder: expandHome(p.folder), parent: p.parent, file: join(expandHome(p.folder), '.harness', 'memory', 'facts.md') }))] });
  }

  addFact(text: string, scope: string, status: 'approved' | 'suggested' = 'approved', source?: string): Fact {
    const t = text.trim();
    if (!t) throw new Error('Write the fact first');
    if (scope !== 'ws' && !this.project(scope)) throw new Error('Project not found');
    const facts = this.scopeFacts(scope);
    const f: Fact = { id: uid('f'), text: t.slice(0, 400), scope, status, source, createdAt: Date.now() };
    facts.push(f);
    this.writeScope(scope, facts);
    this.audit(status === 'suggested' ? 'memory.suggest' : 'memory.add', t.slice(0, 80));
    this.changed();
    return f;
  }

  updateFact(id: string, patch: { status?: 'approved'; text?: string }): void {
    for (const scope of ['ws', ...this.state.projects.map((p) => p.id)]) {
      const facts = this.scopeFacts(scope);
      const f = facts.find((x) => x.id === id);
      if (!f) continue;
      if (patch.status) f.status = patch.status;
      if (patch.text) f.text = patch.text;
      this.writeScope(scope, facts);
      this.audit('memory.approve', f.text.slice(0, 80));
      this.changed();
      return;
    }
    throw new Error('Fact not found');
  }

  removeFact(id: string): void {
    for (const scope of ['ws', ...this.state.projects.map((p) => p.id)]) {
      const facts = this.scopeFacts(scope);
      if (!facts.some((x) => x.id === id)) continue;
      this.writeScope(scope, facts.filter((x) => x.id !== id));
      this.audit('memory.forget', id);
      this.changed();
      return;
    }
    throw new Error('Fact not found');
  }

  clearFacts(): void {
    for (const scope of ['ws', ...this.state.projects.map((p) => p.id)]) if (this.scopeFacts(scope).length) this.writeScope(scope, []);
    this.audit('memory.clear', 'all');
    this.changed();
  }

  // ---------- routines (R-27) ----------
  createRoutine(r: Omit<Routine, 'id' | 'runs' | 'createdAt' | 'paused'> & { paused?: boolean }): Routine {
    if (!this.project(r.project)) throw new Error('Pick a project for this routine');
    const routine: Routine = { ...r, id: uid('rtn'), paused: !!r.paused, runs: [], createdAt: Date.now() };
    this.state.routines.push(routine);
    this.audit('routine.create', routine.name);
    this.changed();
    return routine;
  }

  updateRoutine(id: string, patch: Partial<Routine>): Routine {
    const r = this.state.routines.find((x) => x.id === id);
    if (!r) throw Object.assign(new Error('Routine not found'), { status: 404 });
    if (patch.project && !this.project(patch.project)) throw new Error('Project not found');
    const next = { ...r, ...patch, id: r.id, runs: patch.runs || r.runs, createdAt: r.createdAt };
    Object.assign(r, next);
    this.audit('routine.update', r.name);
    this.changed();
    return r;
  }

  deleteRoutine(id: string): void {
    const r = this.state.routines.find((x) => x.id === id);
    if (!r) throw Object.assign(new Error('Routine not found'), { status: 404 });
    this.state.routines = this.state.routines.filter((x) => x.id !== id);
    this.audit('routine.delete', r.name);
    this.changed();
  }

  // ---------- graphs (R-24, R-37, R-38) ----------
  /**
   * Graphs are files in their project: new ones go to <project>/graphs/<name>.bpmn, and any .bpmn file found in a
   * project folder is a graph of that project too (see scanGraphFiles), so a graph can be added as a file and run.
   */
  graphFile(id: string, project?: string, name?: string): string {
    const meta = this.state.graphs.find((g) => g.id === id);
    const pid = project || meta?.project || this.general().id;
    const p = this.project(pid) || this.general();
    const slug = (name || meta?.name || 'graph').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'graph';
    const dir = join(expandHome(p.folder), 'graphs');
    mkdirSync(dir, { recursive: true });
    const taken = (f: string) => existsSync(f) || this.state.graphs.some((g) => g.id !== id && g.file === f);
    let file = join(dir, `${slug}.bpmn`);
    for (let i = 2; taken(file); i++) file = join(dir, `${slug}-${i}.bpmn`);
    return file;
  }

  private inProjectFolder(file: string, pid: string): boolean {
    const p = this.project(pid);
    return !!p && resolve(file).startsWith(resolve(expandHome(p.folder)) + '/');
  }

  /** Move the file only when the graph moves to another project (or its file is gone); a rename keeps the file. */
  private relocateGraph(meta: GraphMeta): void {
    if (meta.file && existsSync(meta.file) && this.inProjectFolder(meta.file, meta.project)) return;
    const want = this.graphFile(meta.id, meta.project, meta.name);
    if (meta.file && existsSync(meta.file)) { writeText(want, readText(meta.file)); rmSync(meta.file, { force: true }); }
    meta.file = want;
  }
  private runPath(id: string): string { return join(this.paths.runs, id + '.json'); }

  /**
   * Find .bpmn files in project folders: new ones become graphs of the deepest project that holds them, graphs whose
   * file was deleted are dropped. Returns whether anything changed.
   */
  scanGraphFiles(): boolean {
    const SKIP = new Set(['node_modules', '.git', 'dist', 'build', 'out', 'target', '.venv', 'venv', '__pycache__', '.next', '.cache', 'vendor']);
    const known = new Map(this.state.graphs.map((g) => [resolve(g.file), g] as [string, GraphMeta]));
    let changed = false;
    const live = this.state.graphs.filter((g) => !g.file || existsSync(g.file));
    if (live.length !== this.state.graphs.length) { this.state.graphs = live; changed = true; }
    const projects = this.state.projects.filter((p) => !p.archived && p.folder)
      .map((p) => ({ p, dir: resolve(expandHome(p.folder)) }))
      .sort((a, b) => b.dir.length - a.dir.length);
    const seen = new Set<string>();
    for (const { dir } of projects) {
      let budget = 4000;
      const walk = (d: string, depth: number) => {
        if (budget-- <= 0 || depth > 4 || seen.has(d)) return;
        seen.add(d);
        let entries: import('node:fs').Dirent[] = [];
        try { entries = readdirSync(d, { withFileTypes: true }); } catch { return; }
        for (const e of entries) {
          const f = join(d, e.name);
          if (e.isDirectory()) { if (!SKIP.has(e.name) && (!e.name.startsWith('.') || e.name === '.harness')) walk(f, depth + 1); continue; }
          if (!e.isFile() || !e.name.endsWith('.bpmn') || known.has(f)) continue;
          const owner = projects.find((x) => f.startsWith(x.dir + '/'));
          if (!owner) continue;
          let xml = '';
          try { xml = readText(f); } catch { continue; }
          if (!/<(bpmn:)?definitions[\s>]/.test(xml)) continue;
          const name = /<(?:bpmn:)?process\b[^>]*\bname="([^"]+)"/.exec(xml)?.[1] || e.name.replace(/\.bpmn$/, '').replace(/[-_]+/g, ' ');
          const g: GraphMeta = { id: uid('gph'), name: name.slice(0, 80), project: owner.p.id, file: f, updatedAt: Date.now(), source: 'file' };
          this.state.graphs.push(g); known.set(f, g); changed = true;
        }
      };
      walk(dir, 0);
    }
    if (changed) { this.save(); this.changed(); }
    return changed;
  }

  /** A graph by id, name or file path (as agents and the /graph command name them). */
  findGraph(ref: string): GraphMeta | undefined {
    const r = ref.trim();
    if (!r) return undefined;
    const low = r.toLowerCase();
    return this.state.graphs.find((g) => g.id === r)
      || this.state.graphs.find((g) => g.file === r || g.file.endsWith('/' + r.replace(/^\.?\//, '')))
      || this.state.graphs.find((g) => g.name.toLowerCase() === low)
      || this.state.graphs.find((g) => g.name.toLowerCase().includes(low));
  }

  createGraph(o: { name: string; project: string; xml: string; pattern?: string; chat?: string | null }): GraphMeta {
    if (!this.project(o.project)) throw new Error('Pick a project for this graph');
    const name = (o.name || 'New graph').slice(0, 80);
    const g: GraphMeta = { id: uid('gph'), name, project: o.project, file: '', updatedAt: Date.now(), pattern: o.pattern, ...(o.chat && this.chat(o.chat) ? { chat: o.chat } : {}) };
    g.file = this.graphFile(g.id, o.project, name);
    writeText(g.file, o.xml);
    this.state.graphs.push(g);
    this.audit('graph.create', name);
    this.changed();
    return g;
  }

  readGraph(id: string): { meta: GraphMeta; xml: string } {
    const meta = this.state.graphs.find((g) => g.id === id);
    if (!meta) throw Object.assign(new Error('Graph not found'), { status: 404 });
    if (!meta.file || !existsSync(meta.file)) { this.relocateGraph(meta); this.save(); }
    // A file without a diagram (hand-written, or from an agent) gets one, so it opens and passes the model check.
    return { meta, xml: existsSync(meta.file) ? withHarnessSteps(withLayout(readText(meta.file))) : '' };
  }

  saveGraph(id: string, o: { name?: string; project?: string; xml?: string }): GraphMeta {
    const meta = this.state.graphs.find((g) => g.id === id);
    if (!meta) throw Object.assign(new Error('Graph not found'), { status: 404 });
    if (o.project && !this.project(o.project)) throw new Error('Project not found');
    if (o.name) meta.name = o.name.slice(0, 80);
    if (o.project) meta.project = o.project;
    this.relocateGraph(meta);
    if (o.xml !== undefined) writeText(meta.file, o.xml);
    meta.updatedAt = Date.now();
    this.audit('graph.save', meta.name);
    this.changed();
    return meta;
  }

  deleteGraph(id: string): void {
    const meta = this.state.graphs.find((g) => g.id === id);
    if (!meta) throw Object.assign(new Error('Graph not found'), { status: 404 });
    if (meta.file && existsSync(meta.file)) rmSync(meta.file, { force: true });
    this.state.graphs = this.state.graphs.filter((g) => g.id !== id);
    this.audit('graph.delete', meta.name);
    this.changed();
  }

  /** A run started from a chat shows there as a live card (a message that points at the run). */
  graphRunStarted(run: GraphRun, graphName: string): void {
    const chat = run.from ? this.chat(run.from) : undefined;
    if (!chat) return;
    this.appendTranscript(chat, { id: uid('msg'), role: 'system', text: `Running graph “${graphName}”${run.dryRun ? ' (dry run)' : ''}`, createdAt: Date.now(), run: run.id });
    chat.updatedAt = Date.now();
    this.save();
    this.hooks.emit({ type: 'chat.done', workspace: this.id, chat: chat.id, outcome: 'graph' });
  }

  /** The run finished (or waits for someone): its card says so, and the agent hears the results with the next message. */
  graphRunSettled(run: GraphRun): void {
    const chat = run.from ? this.chat(run.from) : undefined;
    if (!chat) return;
    const name = this.state.graphs.find((g) => g.id === run.graph)?.name || 'graph';
    const summary = runSummary(run, name);
    const msg = (chat.transcript || []).find((m) => m.run === run.id);
    if (msg) msg.text = summary;
    if (run.tellAgent && run.status !== 'waiting') chat.graphNotes = [...(chat.graphNotes || []), summary].slice(-5);
    chat.updatedAt = Date.now();
    this.save();
    this.hooks.emit({ type: 'chat.done', workspace: this.id, chat: chat.id, outcome: 'graph' });
  }

  /** Graph results the agent hasn't seen yet, as a block for the next message (taken once). */
  private takeGraphNotes(chat: Chat): string {
    if (!chat.graphNotes?.length) return '';
    const notes = chat.graphNotes; chat.graphNotes = [];
    return `\n\n<harness-graph>\nGraph runs the user started in this chat since your last turn:\n${notes.join('\n\n')}\n</harness-graph>`;
  }

  /** Sessions learn their vault access when they start; this tells a running one when it changed. First call is the baseline. */
  private vaultNote(chat: Chat): string {
    const rule = vaultRule(this, chat.project);
    const last = this.vaultRules.get(chat.id);
    this.vaultRules.set(chat.id, rule);
    if (last === undefined || last === rule) return '';
    return `\n\n<harness-vault>\nVault access changed for this chat.\n${rule || 'This workspace has no vaults now.'}\n</harness-vault>`;
  }

  saveStoredRun(run: GraphRun, engineState: unknown): void {
    writeJson(this.runPath(run.id), { run, engineState });
  }

  listStoredRuns(): { run: GraphRun; engineState: unknown }[] {
    if (!existsSync(this.paths.runs)) return [];
    const out: { run: GraphRun; engineState: unknown }[] = [];
    for (const name of readdirSync(this.paths.runs)) {
      if (!name.endsWith('.json')) continue;
      const row = readJson<{ run?: GraphRun; engineState?: unknown } | null>(join(this.paths.runs, name), null);
      if (row?.run) out.push({ run: row.run, engineState: row.engineState ?? null });
    }
    return out.sort((a, b) => b.run.updatedAt - a.run.updatedAt);
  }

  // ---------- teardown ----------
  async destroy(): Promise<void> {
    await this.stopEngine();
    keychain.purge(this.id);
    if (existsSync(this.paths.root)) rmSync(this.paths.root, { recursive: true, force: true });
  }

  childCount(pid: string): number { return childProjects(this.state.projects, pid).length; }
}

export function loadWorkspace(id: string): Workspace | null {
  return readJson<Workspace | null>(wsPaths(id).harnessJson, null);
}
