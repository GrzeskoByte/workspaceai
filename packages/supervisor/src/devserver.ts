// The browser beside the chat while developing: run the project's dev server (its own command, detected from the
// project), find dev servers already listening from the project folder, and check whether a page may be embedded.
import { spawn, execFile, type ChildProcess } from 'node:child_process';
import { existsSync, readFileSync, readlinkSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';
import { baseEnv } from './opencode.ts';

const run = promisify(execFile);

/** The dev command a project folder suggests: its package.json dev/start script, or Django's runserver. */
export function detectDevCommand(folder: string): string {
  try {
    const pkg = JSON.parse(readFileSync(join(folder, 'package.json'), 'utf8')) as { scripts?: Record<string, string> };
    const script = ['dev', 'start', 'serve', 'preview'].find((k) => pkg.scripts?.[k]);
    if (script) {
      const pm = existsSync(join(folder, 'pnpm-lock.yaml')) ? 'pnpm' : existsSync(join(folder, 'yarn.lock')) ? 'yarn' : existsSync(join(folder, 'bun.lockb')) || existsSync(join(folder, 'bun.lock')) ? 'bun' : 'npm';
      return `${pm} run ${script}`;
    }
  } catch { /* no package.json */ }
  if (existsSync(join(folder, 'manage.py'))) return 'python manage.py runserver';
  if (existsSync(join(folder, 'index.html'))) return 'python3 -m http.server 8000';
  return '';
}

const URL_RE = /(https?:\/\/(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\]|[\w-]+\.localhost)(?::\d+)?(?:\/[^\s'"\x1b)]*)?)/;
/** The first local URL a dev server prints ("Local: http://localhost:5173/"). */
export function devUrlIn(text: string): string {
  const m = URL_RE.exec(text.replace(/\x1b\[[0-9;]*m/g, ''));
  return m ? m[1].replace('0.0.0.0', 'localhost').replace(/\[::1?\]/, 'localhost') : '';
}

interface Dev { proc: ChildProcess; command: string; log: string[]; url: string; startedAt: number; exit: number | null }
const devs = new Map<string, Dev>();

export interface DevStatus { command: string; running: boolean; url: string; log: string[]; startedAt?: number; exit?: number | null }

export function devStatus(key: string, command: string): DevStatus {
  const d = devs.get(key);
  if (!d) return { command, running: false, url: '', log: [] };
  return { command: d.command, running: d.exit === null, url: d.url, log: d.log.slice(-60), startedAt: d.startedAt, exit: d.exit };
}

export function startDev(key: string, folder: string, command: string): DevStatus {
  const cur = devs.get(key);
  if (cur && cur.exit === null) return devStatus(key, command);
  if (!command.trim()) throw Object.assign(new Error('Set the command that starts this project’s dev server'), { status: 400 });
  // Its own process group, so stopping it stops what it started. BROWSER=none: the preview opens beside the chat.
  const proc = spawn('sh', ['-c', command], { cwd: folder, env: { ...baseEnv(), BROWSER: 'none', FORCE_COLOR: '0', CI: '', PYTHONUNBUFFERED: '1' }, stdio: ['ignore', 'pipe', 'pipe'], detached: true });
  const d: Dev = { proc, command, log: [], url: '', startedAt: Date.now(), exit: null };
  devs.set(key, d);
  const read = (b: Buffer) => {
    for (const line of b.toString().replace(/\x1b\[[0-9;]*[A-Za-z]/g, '').split(/\r?\n/)) {
      if (!line.trim()) continue;
      d.log.push(line.slice(0, 400));
      if (!d.url) d.url = devUrlIn(line);
    }
    if (d.log.length > 400) d.log.splice(0, d.log.length - 400);
  };
  proc.stdout?.on('data', read);
  proc.stderr?.on('data', read);
  proc.on('error', (e) => { d.log.push(e.message); d.exit = -1; });
  proc.on('exit', (code) => { d.exit = code ?? -1; d.log.push(`— exited ${code ?? ''}`); });
  return devStatus(key, command);
}

export function stopDev(key: string): void {
  const d = devs.get(key);
  if (!d || d.exit !== null || !d.proc.pid) return;
  try { process.kill(-d.proc.pid, 'SIGTERM'); } catch { d.proc.kill('SIGTERM'); }
  setTimeout(() => { if (d.exit === null && d.proc.pid) try { process.kill(-d.proc.pid, 'SIGKILL'); } catch { /* gone */ } }, 4000).unref();
}

export function stopAllDev(): void { for (const k of devs.keys()) stopDev(k); }
process.once('exit', () => { for (const d of devs.values()) if (d.exit === null && d.proc.pid) try { process.kill(-d.proc.pid, 'SIGTERM'); } catch { /* gone */ } });

export interface ListeningServer { port: number; url: string; process: string; pid: number }

/** Servers listening on this computer whose process runs from inside `folder` (any dev server, however started). */
export async function serversIn(folder: string): Promise<ListeningServer[]> {
  const root = resolve(folder);
  const out = await run('ss', ['-ltnpH'], { timeout: 4000 }).then((r) => r.stdout, () => '');
  const found = new Map<number, ListeningServer>();
  for (const line of out.split('\n')) {
    const cols = line.trim().split(/\s+/);
    const local = cols[3] || '';
    const port = Number(local.slice(local.lastIndexOf(':') + 1));
    const users = /users:\(\("([^"]+)",pid=(\d+)/.exec(line);
    if (!port || !users || found.has(port)) continue;
    if (!/^(127\.0\.0\.1|0\.0\.0\.0|\*|\[::1?\]|\[::\]|::1?|localhost)(%\w+)?:/.test(local)) continue;
    const pid = Number(users[2]);
    let cwd = '';
    try { cwd = readlinkSync(`/proc/${pid}/cwd`); } catch { continue; }
    if (cwd !== root && !cwd.startsWith(root + '/')) continue;
    found.set(port, { port, url: `http://localhost:${port}/`, process: users[1], pid });
  }
  return [...found.values()].sort((a, b) => a.port - b.port);
}

/** Open a web page in the user's own browser; the desktop window can't open links itself. */
export function openExternal(raw: string): Promise<void> {
  let url = '';
  try { const u = new URL(raw); if (/^https?:$/.test(u.protocol)) url = u.href; } catch { /* refused below */ }
  if (!url) throw Object.assign(new Error('Only http and https pages can be opened'), { status: 400 });
  const [cmd, ...args] = process.platform === 'darwin' ? ['open', url] : process.platform === 'win32' ? ['rundll32', 'url.dll,FileProtocolHandler', url] : ['xdg-open', url];
  return new Promise((done, fail) => {
    const p = spawn(cmd, args, { detached: true, stdio: 'ignore' });
    p.once('error', () => fail(new Error(`Could not open your browser (${cmd} failed)`)));
    p.once('spawn', () => { p.unref(); done(); });
  });
}

/** Whether a page may be shown in a frame: its X-Frame-Options and CSP frame-ancestors. */
export async function framePolicy(url: string): Promise<{ ok: boolean; status: number; embeddable: boolean; reason?: string }> {
  if (!/^https?:\/\//i.test(url)) return { ok: false, status: 0, embeddable: false, reason: 'Only http and https pages open here' };
  try {
    const r = await fetch(url, { redirect: 'follow', signal: AbortSignal.timeout(5000) });
    void r.body?.cancel().catch(() => {});
    const xfo = (r.headers.get('x-frame-options') || '').toLowerCase();
    const fa = /frame-ancestors\s+([^;]+)/i.exec(r.headers.get('content-security-policy') || '')?.[1]?.trim().toLowerCase() || '';
    const blocked = /deny|sameorigin/.test(xfo) ? `the page sends X-Frame-Options: ${xfo.toUpperCase()}` : fa && !/\*/.test(fa) ? `the page only allows frames from ${fa}` : '';
    return { ok: r.ok, status: r.status, embeddable: !blocked, ...(blocked ? { reason: blocked } : {}) };
  } catch (e) {
    return { ok: false, status: 0, embeddable: true, reason: `Nothing answered at ${url} (${(e as Error).message})` };
  }
}
