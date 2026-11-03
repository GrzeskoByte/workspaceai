// Open Design as the artifacts manager, unmodified (R-04): one daemon per workspace on 127.0.0.1 with its data
// dir inside the workspace folder, and its stdio MCP server (`od mcp`) registered in that workspace's opencode.json.
// Install is user-triggered from onboarding or Settings › Engine: clone + pnpm install + daemon build.
import { spawn, execFile, type ChildProcess } from 'node:child_process';
import { existsSync, readFileSync, mkdirSync, createWriteStream, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';
import { harnessHome, expandHome } from './paths.ts';
import type { Artifact, LintFinding } from '../../shared/src/types.ts';
import { freePort, baseEnv } from './opencode.ts';
import type { WorkspaceRuntime } from './runtime.ts';

const run = promisify(execFile);
const OD_HOST = join(dirname(fileURLToPath(import.meta.url)), 'od-host.ts');
const REPO = 'https://github.com/nexu-io/open-design.git';

interface Daemon { proc: ChildProcess; port: number; url: string }

/** One workspace's daemon, as Settings › Open Design shows it. */
export interface OdWorkspaceStatus { state: 'not-installed' | 'stopped' | 'starting' | 'running' | 'error'; url?: string; pid?: number; startedAt?: number; error?: string; health?: unknown }

class OpenDesignManager {
  private daemons = new Map<string, Daemon & { startedAt: number }>();
  private starting = new Map<string, Promise<string | null>>();
  private lastError = new Map<string, string>();
  private installing = false;
  private progress = 0;
  private log: string[] = [];
  private error = '';

  installDir(): string { return process.env.HARNESS_OD_DIR || join(harnessHome(), 'vendor', 'open-design'); }

  /** Path to the daemon CLI entry (`od`), if Open Design is installed. */
  cliPath(): string | null {
    const candidates = [process.env.OD_BIN, join(this.installDir(), 'apps/daemon/dist/cli.js')].filter(Boolean) as string[];
    for (const c of candidates) if (existsSync(c)) return c;
    return null;
  }

  version(): string {
    try { return JSON.parse(readFileSync(join(this.installDir(), 'package.json'), 'utf8')).version; } catch { return ''; }
  }

  status() {
    const p = this.cliPath();
    return { installed: !!p, path: p || undefined, version: p ? this.version() : undefined, installing: this.installing, progress: this.progress, log: this.log.slice(-6).join('\n'), error: this.error || undefined, running: Array.from(this.daemons.entries()).map(([ws, d]) => ({ ws, url: d.url })) };
  }

  async install(): Promise<{ started: boolean }> {
    if (this.installing || this.cliPath()) return { started: false };
    return this.build(false);
  }

  /** Pull the latest Open Design and rebuild the daemon. Running daemons keep the old build until restarted. */
  async update(): Promise<{ started: boolean }> {
    if (this.installing || !this.cliPath()) return { started: false };
    return this.build(true);
  }

  private async build(update: boolean): Promise<{ started: boolean }> {
    this.installing = true; this.progress = 2; this.error = ''; this.log = [];
    const dir = this.installDir();
    mkdirSync(join(dir, '..'), { recursive: true });
    const step = async (pct: number, label: string, cmd: string, args: string[], cwd: string) => {
      this.log.push('$ ' + [cmd, ...args].join(' '));
      await new Promise<void>((resolve, reject) => {
        const p = spawn(cmd, args, { cwd, env: { ...process.env, CI: '1' }, stdio: ['ignore', 'pipe', 'pipe'] });
        const out = createWriteStream(join(harnessHome(), 'open-design-install.log'), { flags: 'a' });
        const onData = (b: Buffer) => { out.write(b); const last = b.toString().trim().split('\n').pop(); if (last) { this.log.push(last.slice(0, 160)); this.log = this.log.slice(-40); } };
        p.stdout.on('data', onData); p.stderr.on('data', onData);
        p.on('exit', (code) => { out.end(); code === 0 ? resolve() : reject(new Error(`${label} failed (exit ${code})`)); });
        p.on('error', reject);
      });
      this.progress = pct;
    };
    (async () => {
      try {
        if (update) await step(25, 'pull', 'git', ['pull', '--ff-only'], dir);
        else if (!existsSync(join(dir, 'package.json'))) await step(25, 'clone', 'git', ['clone', '--depth', '1', REPO, dir], join(dir, '..'));
        await step(75, 'install', 'npx', ['-y', 'pnpm@10.33.2', 'install', '--frozen-lockfile'], dir);
        await step(98, 'build', 'npx', ['-y', 'pnpm@10.33.2', '--filter', '@open-design/daemon', 'build'], dir);
        this.progress = 100;
      } catch (e) { this.error = (e as Error).message + ' — see ~/.harness/open-design-install.log'; }
      finally { this.installing = false; }
    })();
    return { started: true };
  }

  /** Start this workspace's daemon (idempotent). Data stays in the workspace folder; bound to loopback (R-35). */
  async ensure(rt: WorkspaceRuntime): Promise<string | null> {
    const cli = this.cliPath();
    if (!cli) return null;
    const cur = this.daemons.get(rt.id);
    if (cur && cur.proc.exitCode === null) return cur.url;
    const pending = this.starting.get(rt.id);
    if (pending) return pending;
    const p = this.spawnDaemon(rt, cli).finally(() => this.starting.delete(rt.id));
    this.starting.set(rt.id, p);
    return p;
  }

  private async spawnDaemon(rt: WorkspaceRuntime, cli: string): Promise<string | null> {
    this.lastError.delete(rt.id);
    const port = await freePort();
    const env = { ...baseEnv(), OD_DATA_DIR: rt.paths.openDesign, OD_HOST: '127.0.0.1', OD_PORT: String(port) };
    // Prefer the harness host: the same daemon, plus the renderer hooks Open Design's desktop app would supply,
    // so PDF and PPTX export work without Electron. Falls back to the plain CLI if the entry is missing.
    const startup = join(this.installDir(), 'apps/daemon/dist/daemon-startup.js');
    const args = existsSync(startup) ? [OD_HOST, this.installDir(), String(port)] : [cli, '--port', String(port), '--host', '127.0.0.1'];
    const proc = spawn(process.execPath, args, { env, detached: true, stdio: ['ignore', 'pipe', 'pipe'], cwd: rt.paths.openDesign });
    const log = createWriteStream(join(rt.paths.logs, 'open-design.log'), { flags: 'a' });
    proc.stdout?.pipe(log); proc.stderr?.pipe(log);
    const url = `http://127.0.0.1:${port}`;
    this.daemons.set(rt.id, { proc, port, url, startedAt: Date.now() });
    proc.on('exit', () => { if (this.daemons.get(rt.id)?.proc === proc) this.daemons.delete(rt.id); });
    for (let i = 0; i < 60; i++) {
      try { const r = await fetch(url + '/api/health', { signal: AbortSignal.timeout(1000) }); if (r.ok) { rt.audit('open-design.start', url, 'harness'); return url; } } catch { /* starting */ }
      await new Promise((r) => setTimeout(r, 500));
    }
    rt.audit('open-design.error', 'daemon did not become healthy', 'harness');
    this.lastError.set(rt.id, 'The daemon did not become healthy within 30 s — see its log');
    await this.stop(rt);
    return null;
  }

  /**
   * The running daemon for an artifact save when Open Design is the artifact engine. Throws a message the agent
   * can pass on when it is missing, so no artifact is ever kept outside Open Design without the user knowing.
   */
  async require(rt: WorkspaceRuntime): Promise<string> {
    if (!this.cliPath()) throw new Error(this.installing ? 'Open Design is still installing — try again when Settings › Open Design shows it ready' : 'Open Design is not installed. Install it in Settings › Open Design (or switch this workspace to the built-in artifact store there).');
    const url = await this.ensure(rt);
    if (!url) throw new Error('Open Design did not start: ' + (this.lastError.get(rt.id) || 'see Settings › Open Design for its log'));
    return url;
  }

  async wsStatus(rt: WorkspaceRuntime): Promise<OdWorkspaceStatus> {
    if (!this.cliPath()) return { state: 'not-installed' };
    if (this.starting.has(rt.id)) return { state: 'starting' };
    const d = this.daemons.get(rt.id);
    if (!d || d.proc.exitCode !== null) return this.lastError.has(rt.id) ? { state: 'error', error: this.lastError.get(rt.id) } : { state: 'stopped' };
    const health = await fetch(d.url + '/api/health', { signal: AbortSignal.timeout(2000) }).then((r) => r.json()).catch(() => null);
    return { state: health ? 'running' : 'error', url: d.url, pid: d.proc.pid, startedAt: d.startedAt, ...(health ? { health } : { error: 'The daemon is not answering' }) };
  }

  async restart(rt: WorkspaceRuntime): Promise<string | null> { await this.stop(rt); return this.ensure(rt); }

  mcpCommand(rt: WorkspaceRuntime): { command: string[]; environment: Record<string, string> } | null {
    const cli = this.cliPath();
    const d = this.daemons.get(rt.id);
    if (!cli || !d) return null;
    return { command: [process.execPath, cli, 'mcp', '--daemon-url', d.url], environment: { OD_DATA_DIR: rt.paths.openDesign } };
  }

  private framework: { directive: string; skeleton: string } | undefined;
  /**
   * Open Design's deck framework, loaded from its build: the directive its own agents follow for decks, and the
   * skeleton (1920×1080 canvas, navigation, print) every deck is built on. Null until Open Design is installed.
   */
  async deckFramework(): Promise<{ directive: string; skeleton: string } | null> {
    if (this.framework) return this.framework;
    const f = join(this.installDir(), 'apps/daemon/dist/prompts/deck-framework.js');
    if (!existsSync(f)) return null;
    try {
      const m = await import(pathToFileURL(f).href);
      // Its "final handoff" tells the agent to answer with an <artifact> tag; the harness tools replace that.
      this.framework = { directive: String(m.renderDeckFrameworkDirective('text_artifact')).split(/\n## Final handoff/)[0].trim(), skeleton: String(m.DECK_SKELETON_HTML) };
      return this.framework;
    } catch { return null; }
  }
  deckSkeleton(): string | null { return this.framework?.skeleton ?? null; }

  url(rt: WorkspaceRuntime): string | null { return this.daemons.get(rt.id)?.url ?? null; }

  private async req<T = any>(url: string, path: string, body?: unknown): Promise<T> {
    const r = await fetch(url + path, { method: body === undefined ? 'GET' : 'POST', headers: body === undefined ? {} : { 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(30000) });
    const text = await r.text();
    let j: any = {};
    try { j = text ? JSON.parse(text) : {}; } catch { j = { raw: text }; }
    if (!r.ok) throw Object.assign(new Error(j?.error?.message || j?.error || `${path} → ${r.status}`), { code: j?.error?.code, status: r.status });
    return j as T;
  }

  /** One Open Design project per harness project, rooted in <project>/.harness/artifacts so files move with the folder (R-13). */
  async projectFor(rt: WorkspaceRuntime, projectId: string, url: string): Promise<string> {
    const map = (rt.state.odProjects ||= {});
    const p = rt.project(projectId);
    if (!p) throw new Error('Project not found');
    const baseDir = join(expandHome(p.folder), '.harness', 'artifacts');
    mkdirSync(baseDir, { recursive: true });
    if (map[projectId]) {
      try { const cur = await this.req<{ project: { metadata?: { baseDir?: string } } }>(url, `/api/projects/${map[projectId]}`); if (cur.project?.metadata?.baseDir === baseDir) return map[projectId]; } catch { /* re-import below */ }
    }
    const r = await this.req<{ project: { id: string } }>(url, '/api/import/folder', { baseDir, name: p.name });
    map[projectId] = r.project.id;
    rt.save();
    return r.project.id;
  }

  /**
   * Lint, then write the page as `<artifact id>/index.html` (and a Markdown source as `<artifact id>/source.md`);
   * Open Design keeps its own version of each write.
   */
  async saveVersion(rt: WorkspaceRuntime, a: Artifact, html: string, source: 'ai' | 'manual' | 'restore', o: { lint?: boolean; markdown?: string } = {}): Promise<{ project: string; file: string; lint: LintFinding[] } | null> {
    const url = this.url(rt) || (await this.ensure(rt));
    if (!url) return null;
    const lint = o.lint === false ? { findings: [] } : await this.req<{ findings: LintFinding[] }>(url, '/api/artifacts/lint', { html });
    const project = await this.projectFor(rt, a.project, url);
    const file = `${a.id}/index.html`;
    await this.req(url, `/api/projects/${project}/files`, { name: file, content: html, encoding: 'utf8', versionSource: source });
    if (o.markdown !== undefined) await this.req(url, `/api/projects/${project}/files`, { name: `${a.id}/source.md`, content: o.markdown, encoding: 'utf8', versionSource: source });
    return { project, file, lint: lint.findings || [] };
  }

  async exportFile(rt: WorkspaceRuntime, a: Artifact, format: 'html' | 'pdf' | 'pptx'): Promise<{ ok: boolean; body?: string | Buffer; mime?: string; filename?: string; error?: string }> {
    const url = this.url(rt) || (await this.ensure(rt));
    const od = a.versions[a.versions.length - 1]?.od;
    if (!url || !od) return { ok: false, error: 'Open Design is not running' };
    const project = (rt.state.odProjects || {})[a.project] || od.project;
    const body = { fileName: od.file, title: a.title, deck: a.type === 'Deck' };
    const r = await fetch(`${url}/api/projects/${project}/export/${format}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body), signal: AbortSignal.timeout(180000) });
    const ct = r.headers.get('content-type') || '';
    if (!r.ok) { const j = await r.json().catch(() => ({})); return { ok: false, error: j?.error?.message || `Open Design export answered ${r.status}` }; }
    const MIME: Record<string, string> = { html: 'text/html', pdf: 'application/pdf', pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation' };
    if (ct.includes('application/json')) {
      const j = await r.json();
      if (typeof j.html === 'string') return { ok: true, body: j.html, mime: 'text/html' };
      if (j.ok === false) return { ok: false, error: j.error || 'Open Design could not export' };
      // The renderer wrote a file (PDF export); Open Design hands back its path.
      if (typeof j.path === 'string' && existsSync(j.path)) { const buf = readFileSync(j.path); rmSync(dirname(j.path), { recursive: true, force: true }); return { ok: true, body: format === 'html' ? buf.toString('utf8') : buf, mime: MIME[format] }; }
      return { ok: false, error: 'Open Design returned no file' };
    }
    const buf = Buffer.from(await r.arrayBuffer());
    return { ok: true, body: format === 'html' ? buf.toString('utf8') : buf, mime: ct.split(';')[0] || MIME[format] };
  }

  async stop(rt: WorkspaceRuntime): Promise<void> {
    const d = this.daemons.get(rt.id);
    if (!d || d.proc.pid == null) return;
    try { process.kill(-d.proc.pid, 'SIGTERM'); } catch { /* gone */ }
    this.daemons.delete(rt.id);
    await new Promise<void>((r) => { if (d.proc.exitCode !== null) r(); else { const t = setTimeout(r, 3000); d.proc.once('exit', () => { clearTimeout(t); r(); }); } });
  }

  async stopAll(): Promise<void> { for (const d of this.daemons.values()) { try { if (d.proc.pid) process.kill(-d.proc.pid, 'SIGTERM'); } catch { /* gone */ } } this.daemons.clear(); }

  async gitAvailable(): Promise<boolean> { try { await run('git', ['--version']); return true; } catch { return false; } }
}

export const openDesign = new OpenDesignManager();
