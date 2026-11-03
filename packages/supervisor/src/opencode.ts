// opencode is the harness: every session, prompt and model call goes through `opencode serve` (R-03).
// One managed server per workspace on 127.0.0.1 with a random password (R-35), or a server the user runs (R-17).
import { spawn, type ChildProcess } from 'node:child_process';
import { createServer } from 'node:net';
import { randomBytes } from 'node:crypto';
import { createWriteStream, existsSync, mkdirSync, type WriteStream } from 'node:fs';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { harnessHome } from './paths.ts';

export interface OcSession {
  id: string; title: string; projectID?: string; parentID?: string;
  model?: { id: string; providerID: string };
  location?: { directory: string };
  time: { created: number; updated: number; archived?: number };
  agent?: string;
}

export interface OcEvent { id?: string; type: string; created?: number; location?: { directory: string }; data: Record<string, unknown> }

/** A pending permission request. `sessionID` is the session that owns it — not always the chat's root session. */
export interface OcPermission { id: string; sessionID: string; action: string; resources?: string[]; save?: string[]; metadata?: Record<string, unknown>; message?: string }

export class OpencodeError extends Error {
  status: number;
  constructor(message: string, status: number) { super(message); this.status = status; }
}

export class OpencodeClient {
  readonly url: string;
  private auth: string;
  constructor(url: string, username: string, password: string) {
    this.url = url.replace(/\/$/, '');
    this.auth = 'Basic ' + Buffer.from(`${username}:${password}`).toString('base64');
  }

  async req<T = unknown>(method: string, path: string, body?: unknown, query?: Record<string, string | number | undefined>, timeoutMs = 30000): Promise<T> {
    const u = new URL(this.url + path);
    for (const [k, v] of Object.entries(query || {})) if (v !== undefined && v !== '') u.searchParams.set(k, String(v));
    const res = await fetch(u, {
      method,
      headers: { authorization: this.auth, accept: 'application/json', ...(body !== undefined ? { 'content-type': 'application/json' } : {}) },
      body: body !== undefined ? JSON.stringify(body) : undefined,
      signal: AbortSignal.timeout(timeoutMs),
    });
    const text = await res.text();
    if (!res.ok) throw new OpencodeError(`${method} ${path} → ${res.status} ${text.slice(0, 300)}`, res.status);
    if (text.trimStart().startsWith('<')) throw new OpencodeError(`${path} returned HTML; is this an opencode v2 server?`, 502);
    return (text ? JSON.parse(text) : {}) as T;
  }

  info() { return this.req<{ version: string; pid: number; urls: string[] }>('GET', '/api/info', undefined, undefined, 5000); }
  async listSessions(q: { directory?: string; limit?: number; parentID?: string } = {}): Promise<OcSession[]> {
    const out: OcSession[] = [];
    let cursor: string | undefined;
    for (let page = 0; page < 50; page++) {
      const r = await this.req<{ data: OcSession[]; cursor: { next: unknown } }>('GET', '/api/session', undefined, { directory: q.directory, parentID: q.parentID, limit: q.limit ?? 200, cursor });
      out.push(...r.data);
      if (!r.cursor?.next || (q.limit && out.length >= q.limit)) break;
      cursor = typeof r.cursor.next === 'string' ? r.cursor.next : JSON.stringify(r.cursor.next);
    }
    return out;
  }
  async createSession(o: { title: string; directory: string; model?: string; variant?: string; parentID?: string }): Promise<OcSession> {
    const body: Record<string, unknown> = { title: o.title, location: { directory: o.directory } };
    if (o.model && o.model.includes('/')) { const i = o.model.indexOf('/'); body.model = { providerID: o.model.slice(0, i), id: o.model.slice(i + 1), ...(o.variant ? { variant: o.variant } : {}) }; }
    if (o.parentID) body.parentID = o.parentID;
    return (await this.req<{ data: OcSession }>('POST', '/api/session', body)).data;
  }
  async getSession(id: string) { return (await this.req<{ data: OcSession }>('GET', `/api/session/${id}`)).data; }
  async renameSession(id: string, title: string) { return (await this.req<{ data: OcSession }>('PATCH', `/api/session/${id}`, { title })).data; }
  /** Replace the session's permission ruleset. `[]` clears it back to the workspace config (which asks first). */
  setSessionPermissions(id: string, permissions: Array<{ action: string; resource: string; effect: 'allow' | 'deny' | 'ask' }>) {
    return this.req('PATCH', `/api/session/${id}`, { permissions });
  }
  async archiveSession(id: string, archived: boolean) { return (await this.req<{ data: OcSession }>('PATCH', `/api/session/${id}`, { archived })).data; }
  deleteSession(id: string) { return this.req('DELETE', `/api/session/${id}`); }
  async forkSession(id: string, before?: string) { return (await this.req<{ data: OcSession }>('POST', `/api/session/${id}/fork`, before ? { before } : {})).data; }
  /** `variant` is opencode's name for a model's reasoning level (low/high/max…); omit it for the model's default. */
  async switchModel(id: string, model: string, variant?: string) { const i = model.indexOf('/'); return this.req('POST', `/api/session/${id}/model`, { model: { providerID: model.slice(0, i), id: model.slice(i + 1), ...(variant ? { variant } : {}) } }); }
  prompt(id: string, text: string, metadata?: Record<string, unknown>) { return this.req('POST', `/api/session/${id}/prompt`, { text, ...(metadata ? { metadata } : {}) }); }
  /** opencode's own commands (built-in `init`, `review`, `goal`, plus the user's custom ones). */
  async commands() { return (await this.req<{ data: { name: string; description?: string }[] }>('GET', '/api/command')).data; }
  runCommand(id: string, name: string, text: string) { return this.req('POST', `/api/session/${id}/command`, { name, text }); }
  compact(id: string) { return this.req('POST', `/api/session/${id}/compact`, {}); }
  interrupt(id: string) { return this.req<{ interrupted: boolean }>('POST', `/api/session/${id}/interrupt`); }
  waitIdle(id: string, timeoutMs = 120000) { return this.req('POST', `/api/experimental/session/${id}/wait`, undefined, undefined, timeoutMs); }
  async messages(id: string): Promise<Record<string, unknown>[]> { return (await this.req<{ data: Record<string, unknown>[] }>('GET', `/api/session/${id}/message`, undefined, { limit: 200 })).data; }
  async models(directory?: string) { return (await this.req<{ data: { id: string; providerID: string; name?: string; status?: string; enabled?: boolean; variants?: { id: string }[]; limit?: { context: number; output: number } }[] }>('GET', '/api/model', undefined, { location: directory })).data; }
  async providers() { return (await this.req<{ data: { id: string; name: string; activation?: string }[] }>('GET', '/api/provider')).data; }
  async mcp() { return (await this.req<{ data: { name: string; status: { status: string } }[] }>('GET', '/api/mcp')).data; }
  async permissions(id: string): Promise<OcPermission[]> { return (await this.req<{ data: OcPermission[] }>('GET', `/api/session/${id}/permission`)).data; }
  /** Every pending permission request for a folder, including ones owned by child (subagent) sessions. */
  async pendingPermissions(directory?: string): Promise<OcPermission[]> {
    const query = directory ? { 'location[directory]': directory } : {};
    return (await this.req<{ data: OcPermission[] }>('GET', '/api/permission/request', undefined, query)).data;
  }
  replyPermission(id: string, req: string, decision: 'once' | 'always' | 'reject') { return this.req('POST', `/api/session/${id}/permission/${req}/reply`, { decision }); }
  exportSession(id: string) { return this.req<{ data?: unknown; info?: unknown; messages?: unknown[] }>('GET', `/api/experimental/session/${id}/export`); }
  importSession(info: unknown, messages: unknown[], directory?: string) { return this.req<{ data: OcSession }>('POST', '/api/experimental/session/import', { info, messages, ...(directory ? { location: { directory } } : {}) }); }
  reload() { return this.req('POST', '/api/location/reload'); }

  /** Reads the SSE stream until aborted. Reconnection is the caller's job. */
  async events(onEvent: (e: OcEvent) => void, signal: AbortSignal): Promise<void> {
    const res = await fetch(this.url + '/api/event', { headers: { authorization: this.auth, accept: 'text/event-stream' }, signal });
    if (!res.ok || !res.body) throw new OpencodeError('event stream ' + res.status, res.status);
    const reader = res.body.getReader();
    const dec = new TextDecoder();
    let buf = '';
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      buf += dec.decode(value, { stream: true });
      let i: number;
      while ((i = buf.indexOf('\n\n')) >= 0) {
        const chunk = buf.slice(0, i); buf = buf.slice(i + 2);
        const data = chunk.split('\n').filter((l) => l.startsWith('data:')).map((l) => l.slice(5).trimStart()).join('\n');
        if (!data) continue;
        try { onEvent(JSON.parse(data)); } catch { /* ignore malformed */ }
      }
    }
  }
}

/** Remote servers must use HTTPS unless they are on loopback (tunnels land on loopback) (R-35). */
export function remoteUrlAllowed(url: string): { ok: boolean; reason?: string } {
  let u: URL;
  try { u = new URL(url); } catch { return { ok: false, reason: 'Not a URL — use http:// or https://' }; }
  if (u.protocol === 'https:') return { ok: true };
  if (u.protocol === 'http:' && ['127.0.0.1', 'localhost', '[::1]', '::1'].includes(u.hostname)) return { ok: true };
  return { ok: false, reason: 'Remote servers must be reached over HTTPS or an SSH tunnel to localhost' };
}

export async function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.unref();
    s.on('error', reject);
    s.listen(0, '127.0.0.1', () => { const a = s.address(); const port = typeof a === 'object' && a ? a.port : 0; s.close(() => resolve(port)); });
  });
}

export interface ManagedOptions {
  binary: string;
  cwd: string;
  env: Record<string, string>;
  logDir: string;
  onExit?: (code: number | null) => void;
}

/** Base environment for a workspace process group: nothing from other workspaces or the parent leaks in (R-06). */
// ---------- version pin per workspace (R-03): an org can hold a version while personal moves ahead ----------

export function pinnedOpencodeDir(version: string): string { return join(harnessHome(), 'vendor', 'opencode', version.replace(/[^0-9A-Za-z.\-]/g, '')); }
export function pinnedOpencodeBin(version: string): string { return join(pinnedOpencodeDir(version), 'node_modules', '.bin', 'opencode'); }

/** The unmodified opencode binary a workspace runs: its pinned release, or the one on PATH (or a custom path). */
export function opencodeBinary(engine: { version?: string; binary?: string }): string {
  const v = (engine.version || 'system').trim();
  if (v && v !== 'system') {
    const bin = pinnedOpencodeBin(v);
    if (!existsSync(bin)) throw new Error(`opencode ${v} is pinned for this workspace but not installed — Settings › Engine › Install`);
    return bin;
  }
  return engine.binary || 'opencode';
}

/** Installs the official `opencode-ai` npm release as is; nothing is patched. */
export async function installOpencode(version: string): Promise<string> {
  if (!/^\d+\.\d+\.\d+([-.][0-9A-Za-z.]+)?$/.test(version)) throw new Error('Use an exact opencode version such as 2.2.1');
  const dir = pinnedOpencodeDir(version);
  mkdirSync(dir, { recursive: true });
  await promisify(execFile)('npm', ['install', '--prefix', dir, '--no-audit', '--no-fund', `opencode-ai@${version}`], { timeout: 10 * 60000, maxBuffer: 16 << 20 });
  const bin = pinnedOpencodeBin(version);
  if (!existsSync(bin)) throw new Error('npm finished but no opencode binary was installed');
  return bin;
}

export function baseEnv(): Record<string, string> {
  const keep = ['PATH', 'HOME', 'USER', 'LOGNAME', 'LANG', 'LC_ALL', 'TERM', 'SHELL', 'TMPDIR', 'DISPLAY', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR', 'DBUS_SESSION_BUS_ADDRESS'];
  const out: Record<string, string> = {};
  for (const k of keep) if (process.env[k]) out[k] = process.env[k] as string;
  return out;
}

export class ManagedServer {
  port = 0;
  password = '';
  proc: ChildProcess | null = null;
  version = '';
  private log: WriteStream | null = null;
  private opts: ManagedOptions;
  private stopping = false;
  private restarts = 0;
  constructor(opts: ManagedOptions) { this.opts = opts; }

  get url() { return `http://127.0.0.1:${this.port}`; }
  client() { return new OpencodeClient(this.url, 'opencode', this.password); }

  async start(): Promise<OpencodeClient> {
    this.stopping = false;
    this.port = await freePort();
    this.password = randomBytes(24).toString('base64url');
    this.log = createWriteStream(join(this.opts.logDir, 'opencode.log'), { flags: 'a' });
    const env = { ...this.opts.env, OPENCODE_SERVER_PASSWORD: this.password };
    // Bound to loopback only; detached so the whole process group can be stopped together.
    this.proc = spawn(this.opts.binary, ['serve', '--hostname', '127.0.0.1', '--port', String(this.port)], { cwd: this.opts.cwd, env, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
    this.proc.stdout?.pipe(this.log, { end: false });
    this.proc.stderr?.pipe(this.log, { end: false });
    const p = this.proc;
    p.on('exit', (code) => { if (this.proc === p) this.proc = null; this.opts.onExit?.(this.stopping ? 0 : code); });
    const c = this.client();
    const deadline = Date.now() + 30000;
    let lastErr: unknown;
    while (Date.now() < deadline) {
      if (!this.proc) throw new Error('opencode exited during start; see ' + join(this.opts.logDir, 'opencode.log'));
      try { const i = await c.info(); this.version = i.version; this.restarts = 0; return c; } catch (e) { lastErr = e; }
      await new Promise((r) => setTimeout(r, 300));
    }
    await this.stop();
    throw new Error('opencode did not become healthy: ' + String(lastErr));
  }

  /** Backoff for crash restarts: 1 s, 2 s, 4 s … capped at 30 s. */
  nextBackoff(): number { this.restarts++; return Math.min(30000, 1000 * 2 ** (this.restarts - 1)); }

  async stop(): Promise<void> {
    this.stopping = true;
    const p = this.proc;
    if (!p || p.pid == null) return;
    const pid = p.pid;
    try { process.kill(-pid, 'SIGTERM'); } catch { /* gone */ }
    await new Promise<void>((resolve) => {
      const t = setTimeout(() => { try { process.kill(-pid, 'SIGKILL'); } catch { /* gone */ } resolve(); }, 3000);
      p.once('exit', () => { clearTimeout(t); resolve(); });
    });
    this.proc = null;
    this.log?.end();
  }
}
