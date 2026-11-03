// The workspace supervisor: owns every WorkspaceRuntime, app-level settings, computers and UI event fan-out.
import { readdirSync, existsSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import type { AppSettings, AppState, Workspace, WorkspaceKind, UiEvent, OrgPolicy, Provider, SignedPolicy, Computer } from '../../shared/src/types.ts';
import { uid, initialsOf, slugify } from '../../shared/src/util.ts';
import { makeWorkspace, demoOrgPolicy, AGENTS_TEMPLATES, PROVIDER_PRESETS } from '../../shared/src/defaults.ts';
import { WorkspaceRuntime, loadWorkspace, type RuntimeHooks } from './runtime.ts';
import { harnessHome, appJsonPath, computersJsonPath, wsPaths, expandHome } from './paths.ts';
import { readJson, writeJson, writeText } from './jsonfile.ts';
import { keychain } from './keychain.ts';
import { demoIssuer, signPolicy, acceptPolicy } from './policy.ts';
import { OpencodeClient, remoteUrlAllowed, type OcSession } from './opencode.ts';

const DEFAULT_SETTINGS: AppSettings = { theme: 'claude', accent: 'default', reduceMotion: false, followSystem: false, onboarded: false, activeWorkspace: null };

export class Supervisor {
  settings: AppSettings;
  runtimes = new Map<string, WorkspaceRuntime>();
  computers: Computer[];
  private listeners = new Set<(e: UiEvent) => void>();
  hooksExtra: Pick<RuntimeHooks, 'extraMcp' | 'extraEnv' | 'beforeStart' | 'afterStop'> = { extraMcp: () => ({}), extraEnv: () => ({}) };

  constructor() {
    mkdirSync(harnessHome(), { recursive: true });
    this.settings = { ...DEFAULT_SETTINGS, ...readJson<Partial<AppSettings>>(appJsonPath(), {}) };
    this.computers = readJson<Computer[]>(computersJsonPath(), []);
    const wsDir = join(harnessHome(), 'ws');
    if (existsSync(wsDir)) for (const id of readdirSync(wsDir)) {
      const ws = loadWorkspace(id);
      if (ws) this.runtimes.set(id, new WorkspaceRuntime(ws, this.hooks()));
    }
  }

  private hooks(): RuntimeHooks {
    return { emit: (e) => this.emit(e), extraMcp: (rt) => this.hooksExtra.extraMcp(rt), extraEnv: (rt) => this.hooksExtra.extraEnv(rt), beforeStart: async (rt) => (this.hooksExtra.beforeStart ? this.hooksExtra.beforeStart(rt) : []), afterStop: async (rt) => { await this.hooksExtra.afterStop?.(rt); } };
  }

  on(fn: (e: UiEvent) => void): () => void { this.listeners.add(fn); return () => this.listeners.delete(fn); }
  emit(e: UiEvent): void { for (const l of this.listeners) l(e); }

  saveSettings(): void { writeJson(appJsonPath(), this.settings); this.emit({ type: 'state', workspace: this.settings.activeWorkspace }); }
  saveComputers(): void { writeJson(computersJsonPath(), this.computers); this.emit({ type: 'state', workspace: this.settings.activeWorkspace }); }

  ordered(): WorkspaceRuntime[] {
    const kindRank: Record<WorkspaceKind, number> = { personal: 0, org: 1, team: 2 };
    return Array.from(this.runtimes.values()).sort((a, b) => kindRank[a.ws.kind] - kindRank[b.ws.kind] || a.ws.createdAt - b.ws.createdAt);
  }

  appState(): AppState {
    return {
      settings: this.settings,
      workspaces: this.ordered().map((r) => ({ id: r.id, name: r.ws.name, kind: r.ws.kind, initials: r.ws.initials, accent: r.ws.accent, note: this.wsNote(r) })),
      computers: this.computers,
    };
  }

  private wsNote(r: WorkspaceRuntime): string {
    const provs = (r.ws.kind === 'org' && r.policy ? r.policy.allowedProviders : r.ws.providers).map((p) => p.name).join(', ') || 'no providers yet';
    const mem = { facts: 'facts', graph: 'Graphify', both: 'Graphify + facts', mcp: 'custom memory' }[r.ws.memory];
    return `${provs} · ${r.ws.mcp.filter((m) => m.on).length} connectors · ${mem}${r.ws.kind === 'org' && r.policy ? ' · ' + r.policy.residency + ' data only' : ''}`;
  }

  rt(id: string): WorkspaceRuntime {
    const r = this.runtimes.get(id);
    if (!r) throw Object.assign(new Error('Workspace not found'), { status: 404 });
    return r;
  }

  createWorkspace(name: string, kind: WorkspaceKind, opts: { providers?: Provider[]; defaultModel?: string } = {}): WorkspaceRuntime {
    const clean = name.trim();
    if (!clean) throw new Error('Name the workspace');
    const id = kind === 'org' ? 'org_' + slugify(clean) : uid('ws');
    if (this.runtimes.has(id)) throw new Error('You are already in ' + clean);
    const ws = makeWorkspace(id, clean, kind, harnessHome());
    ws.providers = opts.providers ?? (kind === 'org' ? [] : [{ ...PROVIDER_PRESETS.opencode, isDefault: true }]);
    ws.defaultModel = opts.defaultModel ?? (ws.providers[0] ? ws.providers[0].id + '/' + ws.providers[0].models[0].id : '');
    const rt = new WorkspaceRuntime(ws, this.hooks());
    this.runtimes.set(id, rt);
    rt.save();
    rt.writeScopesManifest();
    rt.audit('workspace.create', `${clean} (${kind})`);
    this.settings.activeWorkspace ||= id;
    this.saveSettings();
    return rt;
  }

  async deleteWorkspace(id: string): Promise<void> {
    const rt = this.rt(id);
    if (rt.ws.kind === 'personal' && this.ordered().filter((r) => r.ws.kind === 'personal').length === 1) throw new Error('Your personal workspace can’t be deleted');
    await rt.destroy();
    this.runtimes.delete(id);
    for (const c of this.computers) delete c.access[id];
    if (this.settings.activeWorkspace === id) this.settings.activeWorkspace = this.ordered()[0]?.id ?? null;
    this.saveComputers();
    this.saveSettings();
  }

  activate(id: string): void {
    this.rt(id);
    this.settings.activeWorkspace = id;
    this.saveSettings();
  }

  // ---------- organization (R-07, R-08, R-36) ----------
  /**
   * Join an organization through SSO. The demo org is simulated end to end: an OIDC-style sign-in result,
   * a policy signed by the org's issuer key, verification against the pinned key, then opencode.json written from policy.
   */
  joinOrganization(name: string, sso: { issuer: string; subject: string; idp: string }, overrides: Pick<OrgPolicy, 'auditUrl' | 'auditEveryMinutes' | 'logContent' | 'gateway'> = {} as never): WorkspaceRuntime {
    const issuer = demoIssuer();
    // The simulated org's policy; a real org's policy server sets the audit collector the same way.
    const policy: OrgPolicy = { ...demoOrgPolicy(), org: name, ...Object.fromEntries(Object.entries(overrides).filter(([, v]) => v !== undefined)) };
    const signed: SignedPolicy = signPolicy(policy, issuer.privateKey, issuer.keyId);
    const rt = this.createWorkspace(name, 'org', { providers: [], defaultModel: policy.allowedModels[0] });
    const check = acceptPolicy(signed, issuer.publicKey, null);
    if (!check.ok || !check.policy) throw new Error(check.reason || 'Policy rejected');
    rt.policy = check.policy;
    writeJson(rt.paths.policyJson, { ...signed, pinnedKey: issuer.publicKey });
    rt.ws.org = { issuer: sso.issuer, idp: sso.idp, subject: sso.subject, policyVersion: policy.version, policyVerified: true, lastPolicyFetch: Date.now() };
    rt.ws.web = { ...rt.ws.web, mode: policy.web.mode, searxngUrl: policy.web.searxngUrl, locked: policy.web.locked };
    rt.ws.mcp = [...policy.lockedMcp, { id: 'gitlab', name: 'GitLab', kind: 'MCP · http', type: 'remote', url: 'https://gitlab.[ORG-HOST]/api/mcp', on: true }];
    rt.ws.keepChats = policy.retention;
    rt.ws.publicLinks = !policy.blockExternalSharing;
    rt.ws.agents = rt.ws.agents.filter((a) => policy.allowedAgents.includes(a.id));
    if (policy.opencodeVersion) rt.ws.engine = { ...rt.ws.engine, version: policy.opencodeVersion };
    rt.ws.members = [{ name: 'You', email: sso.subject, role: 'Admin' }, { name: '[Member]', role: 'Member' }, { name: '[Member]', role: 'Viewer' }];
    rt.audit('sso.signin', `${sso.subject} via ${sso.idp}`, sso.subject);
    rt.audit('policy.accept', `v${policy.version} · key ${issuer.keyId} · verified`, 'harness');
    rt.save();
    rt.writeConfig();
    return rt;
  }

  /** Re-fetch and verify the org policy (on a timer and at sign-in). */
  refreshPolicy(id: string, tamper = false): { ok: boolean; reason?: string } {
    const rt = this.rt(id);
    if (rt.ws.kind !== 'org') return { ok: true };
    const stored = readJson<(SignedPolicy & { pinnedKey: string }) | null>(rt.paths.policyJson, null);
    if (!stored) return { ok: false, reason: 'No policy on file' };
    const issuer = demoIssuer();
    const fresh = signPolicy({ ...stored.policy }, issuer.privateKey, stored.keyId);
    if (tamper) fresh.policy = { ...fresh.policy, blockPersonalKeys: false };
    const check = acceptPolicy(fresh, stored.pinnedKey, rt.policy);
    if (rt.ws.org) { rt.ws.org.policyVerified = check.ok; rt.ws.org.lastPolicyFetch = Date.now(); }
    rt.audit('policy.refresh', check.ok ? 'verified' : String(check.reason), 'harness');
    rt.changed();
    return { ok: check.ok, reason: check.reason };
  }

  /** Org rows are locked; anything outside policy is refused (R-36). */
  assertAllowed(rt: WorkspaceRuntime, what: 'providers' | 'members' | 'artifacts' | 'data' | 'agents' | 'web' | 'engine' | 'name'): void {
    if (rt.ws.kind !== 'org') return;
    if (['providers', 'agents', 'web', 'engine', 'name', 'data'].includes(what)) {
      rt.audit('policy.block', `attempt to change ${what}`, 'harness');
      throw Object.assign(new Error('Locked by org policy'), { status: 403 });
    }
  }

  sendAllowed(rt: WorkspaceRuntime): void {
    if (rt.ws.kind === 'org' && rt.ws.org && !rt.ws.org.policyVerified) throw Object.assign(new Error('Organization policy could not be verified; sending is blocked until it is.'), { status: 423 });
  }

  // ---------- onboarding (R-29) ----------
  async finishOnboarding(o: {
    workspaceName: string; accent?: string;
    engine: { mode: 'managed' | 'remote'; url?: string; username?: string; password?: string; binary?: string };
    provider: { kind: 'existing' | 'opencode' | 'anthropic' | 'openrouter' | 'ollama' | 'custom'; key?: string; url?: string; model?: string; ollamaModels?: string[] };
    web: Workspace['web'] & { key?: string };
    memory: Workspace['memory']; graphifyFolders?: string[]; graphifyWatch?: boolean;
    project: { name: string; folder: string; agents: string };
    importSessions?: OcSession[]; sync?: boolean;
    theme?: AppSettings['theme']; reduceMotion?: boolean; followSystem?: boolean;
  }): Promise<WorkspaceRuntime> {
    const providers: Provider[] = [];
    const pk = o.provider.kind;
    if (pk === 'existing') providers.push({ ...PROVIDER_PRESETS.opencode, note: 'from your opencode setup', fromOpencode: true, isDefault: true });
    else if (pk === 'custom') providers.push({ ...PROVIDER_PRESETS.custom, baseURL: o.provider.url || PROVIDER_PRESETS.custom.baseURL, models: [{ id: o.provider.model || 'my-model' }], isDefault: true });
    else if (pk === 'ollama') providers.push({ ...PROVIDER_PRESETS.ollama, models: (o.provider.ollamaModels?.length ? o.provider.ollamaModels : ['qwen3-coder']).map((id) => ({ id })), isDefault: true });
    else providers.push({ ...PROVIDER_PRESETS[pk], isDefault: true });
    if (pk !== 'opencode' && pk !== 'existing') providers.push({ ...PROVIDER_PRESETS.opencode });
    const def = providers[0];
    const rt = this.createWorkspace(o.workspaceName || 'Personal', 'personal', { providers, defaultModel: def.id + '/' + def.models[0].id });
    if (o.provider.key && def.keyEnv) keychain.set(rt.id, def.keyEnv, o.provider.key);
    rt.ws.engine = { ...rt.ws.engine, mode: o.engine.mode, url: o.engine.url || rt.ws.engine.url, username: o.engine.username || 'opencode', binary: o.engine.binary || 'opencode', sync: o.sync !== false };
    if (o.engine.mode === 'remote' && o.engine.password) { keychain.set(rt.id, 'OPENCODE_SERVER_PASSWORD', o.engine.password); rt.ws.engine.passwordSecret = 'OPENCODE_SERVER_PASSWORD'; }
    const { key: webKey, ...web } = o.web;
    rt.ws.web = { ...rt.ws.web, ...web, locked: false };
    if (webKey && web.mode === 'mcp') keychain.set(rt.id, { Tavily: 'TAVILY_API_KEY', Brave: 'BRAVE_API_KEY', Firecrawl: 'FIRECRAWL_API_KEY' }[web.provider], webKey);
    rt.ws.memory = o.memory;
    rt.ws.graphifyFolders = o.graphifyFolders || [];
    rt.ws.graphifyWatch = o.graphifyWatch !== false;
    if (o.accent) rt.ws.accent = o.accent as Workspace['accent'];
    const proj = rt.createProject(o.project.name || 'My project', null, o.project.folder || undefined);
    writeText(join(expandHome(proj.folder), 'AGENTS.md'), o.project.agents || AGENTS_TEMPLATES.Software(proj.name));
    rt.ws.defaultProject = rt.general().id;
    if (o.importSessions?.length) rt.importSessions(o.importSessions);
    rt.save();
    rt.writeConfig();
    if (o.theme) this.settings.theme = o.theme;
    if (o.reduceMotion != null) this.settings.reduceMotion = o.reduceMotion;
    if (o.followSystem != null) this.settings.followSystem = o.followSystem;
    this.settings.onboarded = true;
    this.settings.activeWorkspace = rt.id;
    this.saveSettings();
    rt.audit('onboarding.finish', `${rt.ws.name} · ${proj.name}`);
    return rt;
  }

  /** Test a remote opencode server before connecting (onboarding and Settings › Engine). */
  async testRemote(url: string, username: string, password: string): Promise<{ ok: boolean; version?: string; sessions?: number; folders?: number; providers?: number; error?: string }> {
    const allowed = remoteUrlAllowed(url);
    if (!allowed.ok) return { ok: false, error: allowed.reason };
    try {
      const c = new OpencodeClient(url, username || 'opencode', password);
      const info = await c.info();
      const sessions = await c.listSessions({ limit: 500 }).catch(() => []);
      const providers = await c.providers().catch(() => []);
      return { ok: true, version: info.version, sessions: sessions.length, folders: new Set(sessions.map((s) => s.location?.directory)).size, providers: providers.length };
    } catch (e) {
      return { ok: false, error: String((e as Error).message || e).includes('401') ? 'The password was rejected.' : 'Can’t reach that server — check the URL and the password.' };
    }
  }

  // ---------- demo data (R-29 "skip setup") ----------
  async loadDemo(): Promise<void> {
    const demoRoot = join(harnessHome(), 'demo');
    const personal = this.createWorkspace('Personal', 'personal', {
      providers: [{ ...PROVIDER_PRESETS.opencode, isDefault: true }, { ...PROVIDER_PRESETS.anthropic }, { ...PROVIDER_PRESETS.ollama }],
      defaultModel: 'opencode/big-pickle',
    });
    const mk = (name: string, parent: string | null, sub: string, agents: string) => {
      const p = personal.createProject(name, parent, join(demoRoot, sub));
      if (agents) writeText(join(p.folder, 'AGENTS.md'), agents);
      return p;
    };
    const cf = mk('Castform', null, 'castform', '# Castform\n- Units are millimetres everywhere.\n- Run pnpm test before calling a change done.\n');
    const sim = mk('Simulation engine', cf.id, 'castform/sim', '# Simulation engine\n- Keep /sim free of UI imports.\n- Seed any randomness.\n');
    mk('Solver', sim.id, 'castform/sim/solver', '');
    mk('Marketing site', cf.id, 'castform/site', '');
    const eq = mk('Equivaleat', null, 'equivaleat', '# Equivaleat\n- Never give medical advice.\n');
    personal.addFact('Default mold wall is 8 mm for FDM masters.', sim.id);
    personal.addFact('Investor deck stays under 6 slides.', cf.id);
    personal.addFact('Prefers short, direct answers.', 'ws');
    personal.addFact('The beta landing page uses the Castform brand.', cf.id, 'suggested', 'chat: Castform teaser deck');
    personal.ws.agents = personal.ws.agents.map((a) => (a.id === 'claude' ? { ...a, on: true } : a));
    personal.ws.mcp.push({ id: 'hf', name: 'Hugging Face', kind: 'MCP · http', type: 'remote', url: 'https://huggingface.co/mcp', on: true }, { id: 'blender', name: 'Blender', kind: 'MCP · stdio · this computer', type: 'local', command: ['uvx', 'blender-mcp'], on: false });
    personal.state.graphs = [];
    personal.save();
    void eq;
    const org = this.joinOrganization('BSC AI Factory', { issuer: 'https://login.[ORG-HOST]/realms/caim', subject: 'you@[ORG-HOST]', idp: 'MyAccessID' });
    const caim = org.createProject('CAIM', null, join(demoRoot, 'work/caim'));
    writeText(join(caim.folder, 'AGENTS.md'), '# CAIM\n- Never print secrets from realm exports.\n');
    org.createProject('Federation', caim.id, join(demoRoot, 'work/caim/federation'));
    org.createProject('Realm ops', caim.id, join(demoRoot, 'work/caim/realm-ops'));
    org.createProject('Data Portal + ORVM', null, join(demoRoot, 'work/data-portal'));
    org.addFact('Staging and prod realms differ in two attribute mappers.', caim.id);
    this.computers = [
      { id: uid('pc'), name: 'This computer', os: process.platform === 'darwin' ? 'macOS' : process.platform, status: 'online', lastSeen: Date.now(), capabilities: { files: true, commands: true, browser: true, computerUse: false, wake: false }, allowedApps: ['Terminal', 'Browser'], askFirst: { deleteFiles: true, sendMessages: true, purchases: true, installSoftware: true, systemSettings: true, openApps: false }, access: { [personal.id]: { allowed: true, folders: [{ path: join(demoRoot, 'castform'), mode: 'rw' }] }, [org.id]: { allowed: false, folders: [] } }, activity: [{ at: Date.now() - 3600e3, text: 'Ran pnpm test in castform/sim' }] },
      { id: uid('pc'), name: 'Studio desktop', os: 'linux', status: 'asleep', lastSeen: Date.now() - 6 * 3600e3, capabilities: { files: true, commands: true, browser: true, computerUse: true, wake: true }, allowedApps: ['Blender', 'Terminal'], askFirst: { deleteFiles: true, sendMessages: true, purchases: true, installSoftware: true, systemSettings: true, openApps: true }, access: { [personal.id]: { allowed: true, folders: [{ path: '~/renders', mode: 'ro' }] } }, activity: [] },
    ];
    this.saveComputers();
    this.settings.onboarded = true;
    this.settings.activeWorkspace = personal.id;
    this.saveSettings();
  }

  updateComputer(id: string, patch: Partial<Computer>): Computer {
    const c = this.computers.find((x) => x.id === id);
    if (!c) throw Object.assign(new Error('Computer not found'), { status: 404 });
    if (typeof patch.name === 'string' && patch.name.trim()) c.name = patch.name.trim().slice(0, 80);
    if (patch.capabilities) c.capabilities = { ...c.capabilities, ...patch.capabilities };
    if (patch.askFirst) c.askFirst = { ...c.askFirst, ...patch.askFirst };
    if (Array.isArray(patch.allowedApps)) c.allowedApps = patch.allowedApps.map(String).filter(Boolean);
    if (typeof patch.mac === 'string') c.mac = patch.mac.trim() || undefined;
    if (patch.access) c.access = patch.access;
    if (patch.status === 'online' || patch.status === 'asleep' || patch.status === 'offline') c.status = patch.status;
    c.lastSeen = Date.now();
    c.activity = [{ at: Date.now(), text: 'Settings updated' }, ...c.activity].slice(0, 40);
    this.saveComputers();
    return c;
  }

  pairComputer(name: string): Computer {
    const clean = name.trim();
    if (!clean) throw new Error('Name the computer');
    const c: Computer = {
      id: uid('pc'), name: clean.slice(0, 80), os: process.platform, status: 'online', lastSeen: Date.now(),
      capabilities: { files: true, commands: true, browser: true, computerUse: false, wake: false },
      allowedApps: [], askFirst: { deleteFiles: true, sendMessages: true, purchases: true, installSoftware: true, systemSettings: true, openApps: true },
      access: {}, activity: [{ at: Date.now(), text: 'Paired with harness' }],
    };
    this.computers.push(c);
    this.saveComputers();
    return c;
  }

  /** Wake: a Wake-on-LAN magic packet when the computer has a MAC and allows waking (R-28). */
  async wakeComputer(id: string): Promise<Computer> {
    const c = this.computers.find((x) => x.id === id);
    if (!c) throw Object.assign(new Error('Computer not found'), { status: 404 });
    if (c.status === 'online') { c.activity = [{ at: Date.now(), text: 'Already awake' }, ...c.activity].slice(0, 60); this.saveComputers(); return c; }
    if (!c.capabilities.wake) throw new Error(`${c.name} does not allow waking — turn on “Wake for routines” first`);
    if (!c.mac) throw new Error(`Add ${c.name}’s MAC address to send a wake packet`);
    const { sendWake } = await import('./computers.ts');
    await sendWake(c.mac);
    c.activity = [{ at: Date.now(), text: `Wake packet sent to ${c.mac}; it shows online when its desktop app checks in` }, ...c.activity].slice(0, 60);
    this.saveComputers();
    return c;
  }

  /** The other computer's desktop app reports in (heartbeat). */
  checkIn(id: string): Computer {
    const c = this.computers.find((x) => x.id === id);
    if (!c) throw Object.assign(new Error('Computer not found'), { status: 404 });
    if (c.status !== 'online') c.activity = [{ at: Date.now(), text: 'Back online' }, ...c.activity].slice(0, 60);
    c.status = 'online'; c.lastSeen = Date.now();
    this.saveComputers();
    return c;
  }

  unlinkComputer(id: string): void {
    if (!this.computers.some((c) => c.id === id)) throw Object.assign(new Error('Computer not found'), { status: 404 });
    this.computers = this.computers.filter((c) => c.id !== id);
    this.saveComputers();
  }

  async shutdown(): Promise<void> {
    await Promise.all(Array.from(this.runtimes.values()).map((r) => r.stopEngine().catch(() => {})));
  }

  initialsFor(name: string) { return initialsOf(name); }
  pathsFor(id: string) { return wsPaths(id); }
}
