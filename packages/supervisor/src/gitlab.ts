// Open GitLab tickets for a project's git repository, for the map. The map is built synchronously on every read, so
// it shows what is cached and a stale entry refreshes in the background, then asks viewers to reload.
import { execFile } from 'node:child_process';
import { join } from 'node:path';
import { promisify } from 'node:util';
import type { ResolvedMapNode } from '../../shared/src/types.ts';
import { readJson, writeJson } from './jsonfile.ts';
import { keychain } from './keychain.ts';
import { baseEnv } from './opencode.ts';
import { expandHome } from './paths.ts';
import type { WorkspaceRuntime } from './runtime.ts';

const run = promisify(execFile);
const FRESH_MS = 5 * 60_000;
// ponytail: 500 open tickets per repo and no closed ones; follow X-Next-Page further if a map ever needs more.
const MAX_PAGES = 5;

export interface Remote { origin: string; host: string; path: string }
interface Milestone { id: number; title: string; due_date?: string | null; web_url?: string }
interface Ticket { id: number; iid: number; title: string; web_url: string; description?: string | null; milestone?: Milestone | null; assignee?: { name: string } | null; labels?: string[] }
interface Entry { at: number; remote: Remote | null; milestones?: Milestone[]; tickets?: Ticket[]; error?: string }

/** Where a git remote's GitLab lives. Credentials in the URL are dropped; only https/http are spoken to. */
export function parseRemote(url: string): Remote | null {
  const u = url.trim();
  const scp = /^(?:[^@/]+@)?([^:/]+):(?!\/\/)(.+)$/.exec(u);
  let host = '', path = '', origin = '';
  if (/^[a-z+]+:\/\//i.test(u)) {
    try {
      const p = new URL(u);
      host = p.hostname; path = p.pathname.slice(1);
      origin = /^https?:$/.test(p.protocol) ? p.origin : 'https://' + p.hostname;
    } catch { return null; }
  } else if (scp) { host = scp[1]; path = scp[2]; origin = 'https://' + host; }
  path = path.replace(/\/+$/, '').replace(/\.git$/, '');
  // The host goes into git's credential protocol, which is line-based, so nothing but a host name gets through.
  return /^[A-Za-z0-9.-]+$/.test(host) && path.includes('/') ? { origin, host, path } : null;
}

export const tokenName = (host: string) => 'GITLAB_TOKEN_' + host.toUpperCase().replace(/[^A-Z0-9]/g, '_');

/**
 * A token only ever for this host: a cloned repo chooses its own remote, so one shared token could be sent anywhere.
 * The workspace's saved one, or else what git already stores for the host.
 */
async function tokenFor(rt: WorkspaceRuntime, host: string): Promise<string> {
  const saved = keychain.get(rt.id, tokenName(host));
  if (saved) return saved;
  return new Promise((done) => {
    const p = execFile('git', ['credential', 'fill'], { env: { ...baseEnv(), GIT_TERMINAL_PROMPT: '0', GIT_ASKPASS: '', SSH_ASKPASS: '' }, timeout: 5000 }, (e, out) => done(e ? '' : /^password=(.*)$/m.exec(String(out))?.[1] || ''));
    p.stdin?.end(`protocol=https\nhost=${host}\n\n`);
  });
}

async function pages<T>(url: string, token: string): Promise<T[]> {
  const out: T[] = [];
  for (let page = '1', n = 0; page && n < MAX_PAGES; n++) {
    const r = await fetch(`${url}&per_page=100&page=${page}`, { headers: token ? { 'PRIVATE-TOKEN': token } : {}, signal: AbortSignal.timeout(15000) });
    if (!r.ok) throw Object.assign(new Error('GitLab answered ' + r.status), { status: r.status });
    out.push(...(await r.json() as T[]));
    page = r.headers.get('x-next-page') || '';
  }
  return out;
}

const cacheFile = (rt: WorkspaceRuntime) => join(rt.paths.data, 'gitlab-cache.json');
const refreshing = new Set<string>();

async function refresh(rt: WorkspaceRuntime, scope: string, folder: string): Promise<void> {
  const out = await run('git', ['-C', folder, 'config', '--get', 'remote.origin.url'], { timeout: 5000 }).then((r) => r.stdout, () => '');
  const remote = parseRemote(out);
  const entry: Entry = { at: Date.now(), remote };
  if (remote && remote.host !== 'github.com') {
    const api = `${remote.origin}/api/v4/projects/${encodeURIComponent(remote.path)}`;
    try {
      // Never over plain http, where anyone on the way could read it.
      const token = remote.origin.startsWith('https:') ? await tokenFor(rt, remote.host) : '';
      [entry.milestones, entry.tickets] = await Promise.all([pages<Milestone>(`${api}/milestones?state=active`, token), pages<Ticket>(`${api}/issues?state=opened`, token)]);
    } catch (e) {
      const status = (e as { status?: number }).status;
      entry.error = status === 401 || status === 403 || status === 404 ? 'needs a GitLab token' : 'GitLab not reachable';
      // Keep showing the last tickets through a failure, so a dropped connection doesn't empty the map.
      const old = readJson<Record<string, Entry>>(cacheFile(rt), {})[folder];
      if (old?.remote?.path === remote.path) { entry.milestones = old.milestones; entry.tickets = old.tickets; }
    }
  }
  writeJson(cacheFile(rt), { ...readJson<Record<string, Entry>>(cacheFile(rt), {}), [folder]: entry });
  rt.emit({ type: 'map', workspace: rt.id, scope });
}

/** Forget a project's cached answer, so its map asks GitLab again (a new token). */
export function refreshGitlab(rt: WorkspaceRuntime, folder: string): void {
  const all = readJson<Record<string, Entry>>(cacheFile(rt), {});
  if (all[folder]) { all[folder].at = 0; writeJson(cacheFile(rt), all); }
}

/** A cached ticket as GitLab returned it, for its preview: GitLab's own pages refuse to be shown in a frame. */
export function ticketDetail(rt: WorkspaceRuntime, projectFolder: string, iid: number): Ticket | null {
  return readJson<Record<string, Entry>>(cacheFile(rt), {})[expandHome(projectFolder)]?.tickets?.find((t) => t.iid === iid) ?? null;
}

/** The repository under `root`, with its milestones and each one's tickets beneath them. */
export function repoNodes(rt: WorkspaceRuntime, scope: string, projectFolder: string, root: string): ResolvedMapNode[] {
  const folder = expandHome(projectFolder);
  const entry = readJson<Record<string, Entry>>(cacheFile(rt), {})[folder];
  const key = rt.id + folder;
  if ((!entry || Date.now() - entry.at > FRESH_MS) && !refreshing.has(key)) {
    refreshing.add(key);
    refresh(rt, scope, folder).catch(() => {}).finally(() => refreshing.delete(key));
  }
  if (!entry?.remote) return [];
  const { remote, tickets = [], milestones = [], error } = entry;
  const repo = 'repo:' + remote.path;
  const count = `${tickets.length} open ticket${tickets.length === 1 ? '' : 's'}`;
  const nodes: ResolvedMapNode[] = [{ id: repo, kind: 'repo', ref: remote.host, title: remote.path, meta: error ? (tickets.length ? `${count} · ${error}` : error) : remote.host === 'github.com' ? 'GitHub' : count, url: `${remote.origin}/${remote.path}`, parent: root, order: Number.MAX_SAFE_INTEGER - 4, ...(error ? { needsToken: error === 'needs a GitLab token' } : {}) }];
  const shown = new Map<number, Milestone>(milestones.map((m) => [m.id, m]));
  // A ticket can sit in a closed milestone, which the active list leaves out.
  for (const t of tickets) if (t.milestone && !shown.has(t.milestone.id)) shown.set(t.milestone.id, t.milestone);
  const size = (id: number | null) => tickets.filter((t) => (t.milestone?.id ?? null) === id).length;
  for (const m of shown.values()) {
    const n = size(m.id);
    nodes.push({ id: 'milestone:' + m.id, kind: 'milestone', ref: String(m.id), title: m.title, meta: `${n} open${m.due_date ? ' · due ' + m.due_date : ''}`, ...(m.web_url ? { url: m.web_url } : {}), parent: repo, order: m.due_date ? Date.parse(m.due_date) : Number.MAX_SAFE_INTEGER - 1, folded: n > 0 });
  }
  const loose = size(null);
  if (loose) nodes.push({ id: 'milestone:none:' + remote.path, kind: 'milestone', title: 'No milestone', meta: `${loose} open`, parent: repo, order: Number.MAX_SAFE_INTEGER, folded: true });
  for (const t of tickets) nodes.push({ id: 'ticket:' + t.id, kind: 'ticket', ref: String(t.iid), title: `#${t.iid} ${t.title}`, meta: [t.assignee?.name, ...(t.labels || [])].filter(Boolean).join(' · '), url: t.web_url, parent: t.milestone ? 'milestone:' + t.milestone.id : 'milestone:none:' + remote.path, order: t.iid });
  return nodes;
}
