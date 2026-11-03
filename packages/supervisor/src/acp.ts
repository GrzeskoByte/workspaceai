// Minimal ACP client (Agent Client Protocol) over stdio JSON-RPC. Used for subscription agents:
// Grok speaks ACP natively (`grok agent stdio`), opencode does too (`opencode acp`), and Claude Code
// is reached through its ACP adapter. One process per agent per workspace; sessions live inside it.
import { spawn, type ChildProcess } from 'node:child_process';
import { baseEnv } from './opencode.ts';
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, isAbsolute, resolve } from 'node:path';

export interface AcpToolUpdate {
  toolCallId: string;
  title?: string;
  kind?: string;
  status?: string;
  rawInput?: unknown;
  rawOutput?: unknown;
  content?: unknown;
}

export interface AcpPermissionRequest {
  id: string;
  title: string;
  detail: string;
  options: { optionId: string; name: string; kind?: string }[];
}

export interface AcpClientOptions {
  id: string;
  command: string;
  cwd: string;
  env: Record<string, string | undefined>;
  onUpdate: (sessionId: string, update: Record<string, any>) => void;
  onPermission: (sessionId: string, req: AcpPermissionRequest) => Promise<string | null>;
  onExit?: (code: number | null) => void;
  log?: (line: string) => void;
}

export function splitCommand(command: string): string[] {
  const out: string[] = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(command))) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}

interface Pending { resolve: (v: any) => void; reject: (e: Error) => void; timer?: NodeJS.Timeout }

/**
 * A model the agent offers for its sessions (ACP `session/new` → `models.availableModels`), with the effort
 * (reasoning) levels it accepts when the agent says so. No `efforts` means the model has no effort setting.
 */
export interface AcpModel { id: string; name: string; description?: string; efforts?: string[]; defaultEffort?: string }

function readModels(res: any): { models: AcpModel[]; current: string } | null {
  const m = res?.models;
  if (!m || !Array.isArray(m.availableModels)) return null;
  const models = m.availableModels.map((x: any): AcpModel => {
    // Grok lists each model's reasoning levels in `_meta.reasoningEfforts`.
    const re: any[] = Array.isArray(x._meta?.reasoningEfforts) ? x._meta.reasoningEfforts : [];
    const efforts = re.map((e) => String(e.value ?? e.id ?? '')).filter(Boolean);
    const def = re.find((e) => e.default)?.value ?? x._meta?.reasoningEffort;
    return {
      id: String(x.modelId ?? x.id ?? ''), name: String(x.name || x.modelId || x.id || ''),
      ...(x.description ? { description: String(x.description) } : {}),
      ...(efforts.length ? { efforts, ...(def ? { defaultEffort: String(def) } : {}) } : {}),
    };
  }).filter((x: AcpModel) => x.id);
  return { models, current: String(m.currentModelId || models[0]?.id || '') };
}

/** The session config option that sets the reasoning level (ACP `configOptions`, category `thought_level`). */
function readEffortOption(res: any): string {
  const opts: any[] = Array.isArray(res?.configOptions) ? res.configOptions : [];
  return String(opts.find((o) => o?.category === 'thought_level')?.id || '');
}

/** The first `cmd` on PATH, or '' when it is not installed. */
export function findOnPath(cmd: string): string {
  if (cmd.includes('/')) return existsSync(cmd) ? cmd : '';
  for (const p of (process.env.PATH || '').split(':')) if (p && existsSync(resolve(p, cmd))) return resolve(p, cmd);
  return '';
}

/**
 * Claude Code's own model catalog, which says which effort levels each model takes (the ACP adapter drops that).
 * Asks the CLI with an `initialize` control request in stream-json mode; no prompt is sent, so it costs nothing.
 */
export function claudeEffortCatalog(env: Record<string, string>, timeoutMs = 30000): Promise<Map<string, string[]>> {
  return new Promise((done) => {
    const bin = env.CLAUDE_CODE_EXECUTABLE || findOnPath('claude');
    const out = new Map<string, string[]>();
    if (!bin) { done(out); return; }
    const proc = spawn(bin, ['--print', '--input-format', 'stream-json', '--output-format', 'stream-json', '--verbose'], { env: { ...baseEnv(), ...env }, stdio: ['pipe', 'pipe', 'ignore'] });
    let buf = '';
    const finish = () => { clearTimeout(t); try { proc.kill('SIGTERM'); } catch { /* gone */ } done(out); };
    const t = setTimeout(finish, timeoutMs);
    proc.on('error', finish);
    proc.on('exit', finish);
    proc.stdout?.setEncoding('utf8');
    proc.stdout?.on('data', (chunk: string) => {
      buf += chunk;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i); buf = buf.slice(i + 1);
        try {
          const m = JSON.parse(line);
          if (m.type !== 'control_response' || m.response?.request_id !== 'models') continue;
          for (const x of m.response.response?.models || []) if (x?.value) out.set(String(x.value), Array.isArray(x.supportedEffortLevels) ? x.supportedEffortLevels.map(String) : []);
          finish();
        } catch { /* not JSON */ }
      }
    });
    proc.stdin?.write(JSON.stringify({ type: 'control_request', request_id: 'models', request: { subtype: 'initialize' } }) + '\n');
  });
}

export class AcpClient {
  readonly id: string;
  private opts: AcpClientOptions;
  private proc: ChildProcess | null = null;
  private pending = new Map<number, Pending>();
  private nextId = 1;
  private sessions = new Set<string>();
  private starting: Promise<void> | null = null;
  private ready = false;
  private closed = false;
  private stderrTail: string[] = [];
  /** Models the agent reported for its last session, and the one each session runs on now. */
  models: AcpModel[] = [];
  defaultModel = '';
  private sessionModel = new Map<string, string>();
  /** Config option id for the reasoning level, when the agent offers one (Grok: `reasoning_effort`). */
  effortOption = '';
  /** The effort each session was opened or last set with ('' = the agent's default). */
  readonly sessionEffort = new Map<string, string>();

  constructor(opts: AcpClientOptions) {
    this.id = opts.id;
    this.opts = opts;
  }

  get running(): boolean { return !!this.proc && !this.closed; }

  async start(): Promise<void> {
    if (this.ready) return;
    if (this.starting) return this.starting;
    this.starting = this.spawnAndInitialize().finally(() => { this.starting = null; });
    return this.starting;
  }

  private spawnAndInitialize(): Promise<void> {
    return new Promise((resolveStart, rejectStart) => {
      this.closed = false;
      const [cmd, ...args] = splitCommand(this.opts.command);
      if (!cmd) { rejectStart(new Error('No ACP command configured')); return; }
      const proc = spawn(cmd, args, { cwd: this.opts.cwd, env: { ...baseEnv(), ...this.opts.env }, stdio: ['pipe', 'pipe', 'pipe'] });
      this.proc = proc;
      let settled = false;
      const fail = (e: Error) => { if (!settled) { settled = true; rejectStart(e); } };

      let buf = '';
      proc.stdout?.setEncoding('utf8');
      proc.stdout?.on('data', (chunk: string) => {
        buf += chunk;
        let i: number;
        while ((i = buf.indexOf('\n')) >= 0) {
          const line = buf.slice(0, i).trim();
          buf = buf.slice(i + 1);
          if (line) this.onLine(line);
        }
      });
      proc.stderr?.setEncoding('utf8');
      proc.stderr?.on('data', (chunk: string) => {
        const text = String(chunk).trim();
        if (text) {
          this.stderrTail.push(text);
          this.stderrTail = this.stderrTail.slice(-8);
          this.opts.log?.(text.slice(0, 4000));
        }
        // The adapter may print a login URL or a version banner; keep it for the settings screen.
      });
      proc.on('error', (e) => fail(e));
      proc.on('exit', (code) => {
        this.closed = true;
        this.ready = false;
        for (const [, p] of this.pending) { if (p.timer) clearTimeout(p.timer); p.reject(new Error(`ACP agent ${this.id} exited (${code ?? 'signal'})`)); }
        this.pending.clear();
        this.sessions.clear();
        this.opts.onExit?.(code);
        fail(new Error(`ACP agent ${this.id} exited before it was ready`));
      });

      this.request('initialize', {
        protocolVersion: 1,
        clientInfo: { name: 'harness', version: '0.1.0' },
        clientCapabilities: { fs: { readTextFile: true, writeTextFile: true }, terminal: true },
      }, 20000).then((res) => {
        settled = true;
        this.ready = true;
        this.opts.log?.(`${this.id} ready${res?.agentCapabilities ? '' : ''}`);
        resolveStart();
      }).catch((e) => fail(e));
    });
  }

  private onLine(line: string): void {
    let msg: any;
    try { msg = JSON.parse(line); } catch { this.opts.log?.('acp non-json: ' + line.slice(0, 300)); return; }
    if (msg.id !== undefined && (msg.result !== undefined || msg.error !== undefined)) {
      const p = this.pending.get(Number(msg.id));
      if (!p) return;
      this.pending.delete(Number(msg.id));
      if (p.timer) clearTimeout(p.timer);
      if (msg.error) {
        const detail = String(msg.error.message || msg.error);
        const hint = this.stderrTail.slice(-2).join(' | ').replace(/\s+/g, ' ').trim();
        p.reject(new Error(hint && !detail.includes(hint) ? detail + ' — ' + hint.slice(0, 400) : detail));
      } else p.resolve(msg.result);
      return;
    }
    if (msg.method && msg.id !== undefined) { this.onRequest(msg); return; }
    if (msg.method === 'session/update') {
      const sid = String(msg.params?.sessionId || '');
      const update = msg.params?.update || {};
      if (sid) this.opts.onUpdate(sid, update);
    }
  }

  private onRequest(msg: any): void {
    const reply = (result: unknown) => this.write({ jsonrpc: '2.0', id: msg.id, result });
    const error = (code: number, message: string) => this.write({ jsonrpc: '2.0', id: msg.id, error: { code, message } });
    const params = msg.params || {};
    if (msg.method === 'fs/read_text_file') {
      try { reply({ content: readFileSync(this.safePath(String(params.path || ''))).toString('utf8') }); } catch (e) { error(-32000, String((e as Error).message || e)); }
      return;
    }
    if (msg.method === 'fs/write_text_file') {
      try { const p = this.safePath(String(params.path || '')); mkdirSync(dirname(p), { recursive: true }); writeFileSync(p, String(params.content ?? '')); reply({}); } catch (e) { error(-32000, String((e as Error).message || e)); }
      return;
    }
    if (msg.method.startsWith('terminal/')) {
      Promise.resolve().then(() => this.terminal(msg.method.slice(9), params)).then(reply, (e) => error(-32000, String((e as Error).message || e)));
      return;
    }
    if (msg.method === 'session/request_permission') {
      const sessionId = String(params.sessionId || '');
      const options = Array.isArray(params.options) ? params.options : [];
      const req: AcpPermissionRequest = {
        id: 'acp_' + String(msg.id),
        title: String(params.toolCall?.title || params.toolCall?.kind || 'Use a tool'),
        detail: JSON.stringify(params.toolCall?.rawInput ?? params.toolCall?.content ?? '').slice(0, 500),
        options: options.map((o: any) => ({ optionId: String(o.optionId ?? o.id ?? ''), name: String(o.name || o.kind || o.optionId || ''), kind: o.kind })),
      };
      this.opts.onPermission(sessionId, req).then((optionId) => {
        if (!optionId) reply({ outcome: { outcome: 'cancelled' } });
        else reply({ outcome: { outcome: 'selected', optionId } });
      }).catch(() => reply({ outcome: { outcome: 'cancelled' } }));
      return;
    }
    error(-32601, 'Unsupported client method ' + msg.method);
  }

  /** ACP terminals: commands run with the working directory inside the project folder, never outside it. */
  private terminals = new Map<string, { proc: ChildProcess; output: string; limit: number; truncated: boolean; exit: { exitCode: number | null; signal: string | null } | null; waiters: ((v: unknown) => void)[] }>();
  private nextTerminal = 1;
  private terminal(op: string, params: any): any {
    if (op === 'create') {
      const cwd = this.safePath(String(params.cwd || this.opts.cwd));
      const env: Record<string, string> = { ...baseEnv(), ...Object.fromEntries((params.env || []).map((e: { name: string; value: string }) => [e.name, e.value])) };
      const proc = spawn(String(params.command), (params.args || []).map(String), { cwd, env, stdio: ['ignore', 'pipe', 'pipe'], shell: !(params.args || []).length });
      const id = 'term_' + this.nextTerminal++;
      const t = { proc, output: '', limit: Number(params.outputByteLimit) || 1 << 20, truncated: false, exit: null as any, waiters: [] as ((v: unknown) => void)[] };
      const add = (b: Buffer) => { t.output += b.toString('utf8'); if (t.output.length > t.limit) { t.output = t.output.slice(-t.limit); t.truncated = true; } };
      proc.stdout?.on('data', add); proc.stderr?.on('data', add);
      proc.on('close', (code, signal) => { t.exit = { exitCode: code, signal: signal || null }; t.waiters.splice(0).forEach((w) => w(t.exit)); });
      proc.on('error', (e) => { add(Buffer.from(String(e.message))); t.exit = { exitCode: -1, signal: null }; t.waiters.splice(0).forEach((w) => w(t.exit)); });
      this.terminals.set(id, t);
      this.opts.log?.(`terminal ${id} · ${params.command} ${(params.args || []).join(' ')} · ${cwd}`);
      return { terminalId: id };
    }
    const t = this.terminals.get(String(params.terminalId));
    if (!t) throw new Error('Unknown terminal ' + params.terminalId);
    if (op === 'output') return { output: t.output, truncated: t.truncated, exitStatus: t.exit };
    if (op === 'wait_for_exit') return t.exit ? t.exit : new Promise((r) => t.waiters.push(r));
    if (op === 'kill') { try { t.proc.kill('SIGTERM'); } catch { /* gone */ } return {}; }
    if (op === 'release') { try { if (!t.exit) t.proc.kill('SIGKILL'); } catch { /* gone */ } this.terminals.delete(String(params.terminalId)); return {}; }
    throw new Error('Unsupported terminal method ' + op);
  }

  private safePath(p: string): string {
    const abs = isAbsolute(p) ? p : resolve(this.opts.cwd, p);
    const root = resolve(this.opts.cwd);
    if (abs !== root && !abs.startsWith(root + '/')) throw new Error('ACP file access is limited to this project folder');
    return abs;
  }

  private write(obj: unknown): void {
    try { this.proc?.stdin?.write(JSON.stringify(obj) + '\n'); } catch { /* process gone */ }
  }

  request(method: string, params: unknown, timeoutMs = 60000): Promise<any> {
    if (this.closed) return Promise.reject(new Error('ACP agent is not running'));
    const id = this.nextId++;
    return new Promise((resolveReq, rejectReq) => {
      const timer = timeoutMs > 0 ? setTimeout(() => { this.pending.delete(id); rejectReq(new Error(`ACP ${method} timed out`)); }, timeoutMs) : undefined;
      this.pending.set(id, { resolve: resolveReq, reject: rejectReq, timer });
      this.write({ jsonrpc: '2.0', id, method, params });
    });
  }

  notify(method: string, params: unknown): void {
    this.write({ jsonrpc: '2.0', method, params });
  }

  async newSession(o: { cwd: string; mcpServers?: unknown[]; meta?: Record<string, unknown>; effort?: string }): Promise<string> {
    await this.start();
    const res = await this.request('session/new', { cwd: o.cwd, mcpServers: o.mcpServers || [], _meta: { yoloMode: false, ...o.meta } }, 60000);
    const sid = String(res?.sessionId || res?.session_id || '');
    if (!sid) throw new Error('The agent did not return a session id');
    this.sessions.add(sid);
    this.noteModels(sid, res);
    if (o.effort !== undefined) this.sessionEffort.set(sid, o.effort);
    return sid;
  }

  private noteModels(sid: string, res: any): void {
    this.effortOption = readEffortOption(res) || this.effortOption;
    const m = readModels(res);
    if (!m) return;
    this.models = m.models;
    this.defaultModel ||= m.current;
    if (m.current) this.sessionModel.set(sid, m.current);
  }

  /** Switch a session's model (ACP `session/set_model`). No-op when it already runs on that model. */
  async setModel(sessionId: string, modelId: string): Promise<void> {
    if (!modelId || this.sessionModel.get(sessionId) === modelId) return;
    if (this.models.length && !this.models.some((m) => m.id === modelId)) throw new Error(`${this.id} does not offer the model ${modelId}`);
    await this.request('session/set_model', { sessionId, modelId }, 15000);
    this.sessionModel.set(sessionId, modelId);
  }

  /**
   * Set a session's reasoning level through the agent's `thought_level` config option. Returns false when the
   * agent has no such option (Claude Code takes effort only when a session is opened; see `newSession`'s `meta`).
   */
  async setEffort(sessionId: string, effort: string): Promise<boolean> {
    if (!this.effortOption) return false;
    if (!effort || this.sessionEffort.get(sessionId) === effort) return true;
    await this.request('session/set_config_option', { sessionId, configId: this.effortOption, value: effort }, 15000);
    this.sessionEffort.set(sessionId, effort);
    return true;
  }

  /** Re-attach to a session after the agent process restarted. Agents that do not support it throw. */
  async loadSession(o: { cwd: string; sessionId: string; mcpServers?: unknown[]; meta?: Record<string, unknown>; effort?: string }): Promise<string> {
    await this.start();
    const res = await this.request('session/load', { sessionId: o.sessionId, cwd: o.cwd, mcpServers: o.mcpServers || [], ...(o.meta ? { _meta: o.meta } : {}) }, 30000);
    const sid = String(res?.sessionId || res?.session_id || o.sessionId);
    this.sessions.add(sid);
    this.sessionModel.delete(sid);
    this.noteModels(sid, res);
    if (o.effort !== undefined) this.sessionEffort.set(sid, o.effort);
    return sid;
  }

  async prompt(sessionId: string, text: string): Promise<string> {
    await this.start();
    const res = await this.request('session/prompt', { sessionId, prompt: [{ type: 'text', text }] }, 0);
    return String(res?.stopReason || 'end_turn');
  }

  async cancel(sessionId: string): Promise<void> {
    if (!this.running) return;
    this.notify('session/cancel', { sessionId });
  }

  async closeSession(sessionId: string): Promise<void> {
    if (!this.running) return;
    await this.request('session/close', { sessionId }, 5000).catch(() => {});
    this.sessions.delete(sessionId);
  }

  async close(): Promise<void> {
    this.closed = true;
    this.ready = false;
    const proc = this.proc;
    this.proc = null;
    if (!proc) return;
    try { proc.stdin?.end(); } catch { /* gone */ }
    await new Promise<void>((r) => {
      const t = setTimeout(() => { try { proc.kill('SIGTERM'); } catch { /* gone */ } r(); }, 1500);
      proc.once('exit', () => { clearTimeout(t); r(); });
    });
  }
}

export function acpCommandFor(agent: { id: string; command?: string }): string {
  if (agent.command) return agent.command;
  if (agent.id === 'grok') return 'grok agent stdio';
  if (agent.id === 'claude') return 'npx -y @zed-industries/claude-code-acp';
  return 'opencode acp';
}

export function acpInstalled(command: string): boolean {
  const [cmd] = splitCommand(command);
  if (!cmd) return false;
  if (cmd.includes('/')) return existsSync(cmd);
  const path = (process.env.PATH || '').split(':');
  return path.some((p) => p && existsSync(resolve(p, cmd)));
}
