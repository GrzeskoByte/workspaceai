// Subscription-backed agents (opencode, Claude Code, Grok). The harness never stores a subscription
// token in a config file: it reads the login the user already made on this computer and copies it into
// that workspace's own agent home, so one workspace never sees another's history or credentials.
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import type { SubscriptionId, SubscriptionStatus, Workspace } from '../../shared/src/types.ts';
import type { WorkspaceRuntime } from './runtime.ts';
import { findOnPath } from './acp.ts';

const run = promisify(execFile);

interface Def {
  id: SubscriptionId;
  name: string;
  command: string;
  versionArgs: string[];
  login: string;
  /** env var that redirects the agent's own state into this workspace. */
  homeEnv: string;
}

export const SUBSCRIPTIONS: Record<SubscriptionId, Def> = {
  opencode: { id: 'opencode', name: 'opencode', command: 'opencode', versionArgs: ['--version'], login: 'opencode auth login', homeEnv: 'XDG_DATA_HOME' },
  claude: { id: 'claude', name: 'Claude Code', command: 'claude', versionArgs: ['--version'], login: 'claude auth login', homeEnv: 'CLAUDE_CONFIG_DIR' },
  grok: { id: 'grok', name: 'Grok', command: 'grok', versionArgs: ['--version'], login: 'grok login', homeEnv: 'GROK_HOME' },
};

const ORDER: SubscriptionId[] = ['opencode', 'claude', 'grok'];

interface Detected {
  installed: boolean;
  version?: string;
  signedIn: boolean;
  account?: string;
  models: string[];
  detail: string;
}

let cache: { at: number; rows: Record<SubscriptionId, Detected> } | null = null;

async function tryRun(cmd: string, args: string[], timeout = 8000, env?: NodeJS.ProcessEnv): Promise<{ ok: boolean; out: string }> {
  try {
    const r = await run(cmd, args, { timeout, maxBuffer: 2 << 20, env: { ...process.env, NO_COLOR: '1', CI: '1', ...(env || {}) } });
    return { ok: true, out: String(r.stdout || '') + String(r.stderr || '') };
  } catch (e) {
    const err = e as { stdout?: string; stderr?: string };
    return { ok: false, out: String(err.stdout || '') + String(err.stderr || '') };
  }
}

function cleanVersion(out: string): string | undefined {
  const line = out.split('\n').map((s) => s.trim()).find(Boolean);
  if (!line) return undefined;
  return line.replace(/^.*?v?(\d+\.\d+\.\d+(?:[-+][^\s]*)?).*$/i, '$1').slice(0, 40);
}

function jsonFrom(out: string): any {
  const start = out.indexOf('{');
  const arr = out.indexOf('[');
  const at = start < 0 ? arr : arr < 0 ? start : Math.min(start, arr);
  if (at < 0) return null;
  try { return JSON.parse(out.slice(at)); } catch { return null; }
}

function claudeAccount(): string | undefined {
  try {
    const d = JSON.parse(readFileSync(join(homedir(), '.claude.json'), 'utf8')) as { oauthAccount?: { emailAddress?: string; displayName?: string; fullName?: string; organizationName?: string } };
    const o = d.oauthAccount;
    return o?.emailAddress || o?.displayName || o?.fullName || o?.organizationName;
  } catch { return undefined; }
}

function grokAccount(): string | undefined {
  try {
    const d = JSON.parse(readFileSync(join(homedir(), '.grok', 'auth.json'), 'utf8')) as Record<string, { first_name?: string; email?: string; user_id?: string }>;
    for (const v of Object.values(d)) if (v && (v.first_name || v.email)) return v.first_name || v.email;
  } catch { /* not signed in */ }
  return undefined;
}

export async function detectSubscriptions(force = false): Promise<Record<SubscriptionId, Detected>> {
  if (!force && cache && Date.now() - cache.at < 30000) return cache.rows;
  const rows = {} as Record<SubscriptionId, Detected>;

  const ocV = await tryRun('opencode', ['--version']);
  const ocAuth = ocV.ok ? await tryRun('opencode', ['auth', 'list', '--format', 'json']) : { ok: false, out: '' };
  const ocList = jsonFrom(ocAuth.out);
  const ocProviders = Array.isArray(ocList) ? ocList.map((x: any) => String(x.name || x.id || x.provider || '')).filter(Boolean) : [];
  rows.opencode = {
    installed: ocV.ok,
    version: ocV.ok ? cleanVersion(ocV.out) : undefined,
    signedIn: ocProviders.length > 0,
    account: ocProviders.length ? ocProviders.join(', ') : undefined,
    models: [],
    detail: ocV.ok ? (ocProviders.length ? 'Signed in to ' + ocProviders.join(', ') : 'Installed; no subscription signed in') : 'Not installed',
  };

  const clV = await tryRun('claude', ['--version']);
  const clAuth = clV.ok ? await tryRun('claude', ['auth', 'status']) : { ok: false, out: '' };
  const clJson = jsonFrom(clAuth.out);
  const clIn = !!clJson?.loggedIn;
  rows.claude = {
    installed: clV.ok,
    version: clV.ok ? cleanVersion(clV.out) : undefined,
    signedIn: clIn,
    account: claudeAccount(),
    models: [],
    detail: clV.ok ? (clIn ? 'Signed in with a Claude subscription' : 'Installed; sign in with “claude auth login”') : 'Not installed',
  };

  const grV = await tryRun('grok', ['--version']);
  let grIn = false;
  let grModels: string[] = [];
  if (grV.ok) {
    try {
      const d = JSON.parse(readFileSync(join(homedir(), '.grok', 'auth.json'), 'utf8')) as Record<string, { expires_at?: string }>;
      const main = Object.values(d).find((v) => v && v.expires_at);
      grIn = !!main && (!main.expires_at || Date.parse(main.expires_at) > Date.now());
    } catch { /* not signed in */ }
    const models = await tryRun('grok', ['models'], 10000);
    grModels = models.out.split('\n').map((s) => /^\s*[*-]\s+([a-z0-9][a-z0-9._-]+)/i.exec(s)?.[1]).filter((x): x is string => !!x).slice(0, 12);
  }
  rows.grok = {
    installed: grV.ok,
    version: grV.ok ? cleanVersion(grV.out) : undefined,
    signedIn: grIn,
    account: grIn ? grokAccount() : undefined,
    models: grModels,
    detail: grV.ok ? (grIn ? 'Signed in with a Grok subscription' : 'Installed; run “grok login”') : 'Not installed',
  };

  cache = { at: Date.now(), rows };
  return rows;
}

export function agentHome(rt: WorkspaceRuntime, id: SubscriptionId): string {
  return join(rt.paths.data, 'agents', id);
}

function copyIfThere(from: string, to: string): boolean {
  if (!existsSync(from)) return false;
  mkdirSync(join(to, '..'), { recursive: true });
  copyFileSync(from, to);
  return true;
}

/** Environment overrides that keep this subscription agent inside the workspace's own folder. */
export function agentEnv(rt: WorkspaceRuntime, id: SubscriptionId): Record<string, string> {
  const home = agentHome(rt, id);
  if (id === 'grok') return { GROK_HOME: home };
  if (id === 'claude') {
    // The ACP adapter bundles an old Claude Code whose model list is stale; point it at the installed CLI so the
    // subscription's current models (and their effort levels) are the ones offered.
    const cli = findOnPath('claude');
    return { CLAUDE_CONFIG_DIR: home, ...(cli ? { CLAUDE_CODE_EXECUTABLE: cli } : {}) };
  }
  return {
    XDG_DATA_HOME: join(home, 'xdg-data'),
    XDG_CONFIG_HOME: join(home, 'xdg-config'),
    XDG_STATE_HOME: join(home, 'xdg-state'),
    XDG_CACHE_HOME: join(home, 'xdg-cache'),
  };
}

/** Copy the login already on this computer into the workspace's own agent home (R-06, R-34). */
export async function connectSubscription(rt: WorkspaceRuntime, id: SubscriptionId): Promise<SubscriptionStatus> {
  const def = SUBSCRIPTIONS[id];
  const home = agentHome(rt, id);
  await rt.stopAgents(rt.ws.agents.filter((a) => a.subscription === id).map((a) => a.id));
  mkdirSync(home, { recursive: true, mode: 0o700 });
  if (id === 'grok') {
    copyIfThere(join(homedir(), '.grok', 'auth.json'), join(home, 'auth.json'));
    copyIfThere(join(homedir(), '.grok', 'config.toml'), join(home, 'config.toml'));
  } else if (id === 'claude') {
    // Keep the account and preferences, but never import the global project history into a workspace.
    const global = join(homedir(), '.claude.json');
    if (existsSync(global)) {
      try {
        const data = JSON.parse(readFileSync(global, 'utf8')) as Record<string, unknown>;
        delete data.projects;
        writeFileSync(join(home, '.claude.json'), JSON.stringify(data, null, 2), { mode: 0o600 });
      } catch { copyIfThere(global, join(home, '.claude.json')); }
    }
    copyIfThere(join(homedir(), '.claude', '.credentials.json'), join(home, '.credentials.json'));
  } else {
    // opencode keeps credentials in its data dir. Export once and import into the isolated dirs.
    const env = agentEnv(rt, id);
    for (const d of Object.values(env)) mkdirSync(d, { recursive: true, mode: 0o700 });
    const exp = await tryRun('opencode', ['auth', 'export'], 8000, env);
    const parsed = jsonFrom(exp.out);
    if (Array.isArray(parsed) && parsed.length) {
      const file = join(home, 'auth-export.json');
      writeFileSync(file, JSON.stringify(parsed), { mode: 0o600 });
      await tryRun('opencode', ['auth', 'import', file], 8000, env);
      rmSync(file, { force: true });
    }
  }
  const agent = rt.ws.agents.find((a) => a.subscription === id);
  if (agent) agent.on = true;
  else rt.ws.agents.push({ id, name: def.name, via: 'ACP · ' + def.name, command: id === 'grok' ? 'grok agent stdio' : id === 'opencode' ? 'opencode acp' : 'npx -y @zed-industries/claude-code-acp', on: true, subscription: id });
  rt.audit('subscription.connect', def.name + ' · workspace-local agent home');
  rt.save();
  rt.changed();
  return (await subscriptionStatus(rt, true)).find((s) => s.id === id) as SubscriptionStatus;
}

export async function disconnectSubscription(rt: WorkspaceRuntime, id: SubscriptionId): Promise<SubscriptionStatus> {
  await rt.stopAgents(rt.ws.agents.filter((a) => a.subscription === id).map((a) => a.id));
  const home = agentHome(rt, id);
  if (existsSync(home)) rmSync(home, { recursive: true, force: true });
  const agent = rt.ws.agents.find((a) => a.subscription === id);
  if (agent) agent.on = false;
  // The agent's own child process can still write its config while it shuts down; sweep once more after that.
  setTimeout(() => { if (!rt.ws.agents.some((a) => a.subscription === id && a.on)) rmSync(home, { recursive: true, force: true }); }, 3000).unref();
  if (agent && rt.ws.defaultAgent === agent.id) rt.ws.defaultAgent = 'opencode';
  rt.audit('subscription.disconnect', SUBSCRIPTIONS[id].name + ' · local agent data removed');
  rt.save();
  rt.changed();
  return subscriptionStatusSync(rt, id);
}

export function subscriptionStatusSync(rt: WorkspaceRuntime, id: SubscriptionId): SubscriptionStatus {
  const def = SUBSCRIPTIONS[id];
  const agent = rt.ws.agents.find((a) => a.subscription === id);
  const connected = existsSync(agentHome(rt, id));
  return {
    id, name: def.name, installed: false, signedIn: false, account: undefined, version: undefined,
    dataDir: agentHome(rt, id), command: agent?.command || def.login, connected,
    agentOn: !!agent?.on, models: [], detail: connected ? 'Connected to this workspace' : 'Not connected to this workspace',
  };
}

/** Per-workspace rows for Settings › Agents. */
export async function subscriptionStatus(rt: WorkspaceRuntime, force = false): Promise<SubscriptionStatus[]> {
  const detected = await detectSubscriptions(force);
  return ORDER.map((id) => {
    const d = detected[id];
    const agent = rt.ws.agents.find((a) => a.subscription === id);
    const connected = existsSync(agentHome(rt, id));
    return {
      id, name: SUBSCRIPTIONS[id].name,
      installed: d.installed, signedIn: d.signedIn, version: d.version, account: d.account,
      dataDir: agentHome(rt, id), command: agent?.command || SUBSCRIPTIONS[id].login,
      connected, agentOn: !!agent?.on, models: d.models,
      detail: connected ? (d.signedIn ? d.detail + ' · stored only in this workspace' : 'Connected, but sign in again: ' + SUBSCRIPTIONS[id].login) : d.detail,
    };
  });
}

/** First-run detection, before a workspace exists. */
export async function detectedRows(force = false): Promise<SubscriptionStatus[]> {
  const detected = await detectSubscriptions(force);
  return ORDER.map((id) => {
    const d = detected[id];
    return {
      id, name: SUBSCRIPTIONS[id].name, installed: d.installed, signedIn: d.signedIn,
      version: d.version, account: d.account, dataDir: '', command: SUBSCRIPTIONS[id].login,
      connected: false, agentOn: false, models: d.models, detail: d.detail,
    };
  });
}

/** The agent record for a subscription id; used by ACP chats. */
export function acpAgentFor(ws: Workspace, id: string) {
  return ws.agents.find((a) => a.id === id);
}
