// Graphify memory: read Graphify's own graph.json when it exists, otherwise build a deterministic
// folder/file index for this workspace. Nothing here leaves the workspace folder (R-06, R-23).
import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, realpathSync, statSync, watch, type Dirent, type FSWatcher } from 'node:fs';
import { basename, dirname, extname, join, relative, resolve, sep } from 'node:path';
import type { GraphifyStatus, MemoryGraph, MemoryGraphEdge, MemoryGraphNode, MemoryNodeDetail } from '../../shared/src/types.ts';
import type { WorkspaceRuntime } from './runtime.ts';
import { expandHome, harnessHome } from './paths.ts';
import { readJson, readText, writeJson } from './jsonfile.ts';
import { baseEnv } from './opencode.ts';
import { keychain } from './keychain.ts';
import { effectiveProviders } from '../../shared/src/opencode-config.ts';

const run = promisify(execFile);

const COLORS = ['#cc785c', '#5db8a6', '#5c9cf5', '#e8a55a', '#a78bfa', '#5db872', '#e06c9f', '#7dd3fc', '#f472b6', '#facc15'];

export const SKIP = new Set(['.git', 'node_modules', '.next', 'dist', 'build', 'out', '.cache', '.turbo', 'target', 'vendor', '.venv', 'venv', '__pycache__', '.harness']);
const MAX_FILES = 1800;
const MAX_NODES = 3200;

function dir(rt: WorkspaceRuntime): string { return join(rt.paths.data, 'graphify'); }
function cacheFile(rt: WorkspaceRuntime): string { return join(dir(rt), 'graph.json'); }
function reviewFile(rt: WorkspaceRuntime): string { return join(dir(rt), 'review.json'); }

function palette(i: number): string { return COLORS[Math.abs(i) % COLORS.length]; }

/** Where the harness installs graphify itself when neither uv nor pipx is available. */
export function graphifyVenv(): string { return join(harnessHome(), 'vendor', 'graphify'); }
const venvBin = () => join(graphifyVenv(), 'bin', 'graphify');

function isInstalled(): boolean {
  if (existsSync(venvBin())) return true;
  const path = (process.env.PATH || '').split(':');
  return path.some((p) => p && existsSync(resolve(p, 'graphify')));
}

export function realGraphFile(rt: WorkspaceRuntime): string | null {
  for (const f of rt.ws.graphifyFolders) {
    const file = join(expandHome(f), 'graphify-out', 'graph.json');
    if (existsSync(file)) return file;
  }
  return null;
}

export function graphifyStatus(rt: WorkspaceRuntime): GraphifyStatus {
  const real = realGraphFile(rt);
  const cache = readJson<MemoryGraph | null>(cacheFile(rt), null);
  const review = loadReview(rt);
  const base: GraphifyStatus = {
    installed: graphifyInstalled(),
    folders: rt.ws.graphifyFolders.slice(),
    watching: !!rt.ws.graphifyWatch,
    graphFile: real || (cache ? cacheFile(rt) : undefined),
    builtAt: cache?.builtAt,
    nodes: cache?.nodes.length || 0,
    edges: cache?.edges.length || 0,
    communities: cache?.communities.length || 0,
    pendingLinks: review.filter((a) => a.status === 'pending').length,
  };
  if (real) {
    try { base.builtAt = statSync(real).mtimeMs; } catch { /* ignore */ }
  }
  return base;
}

export async function graphifyVersion(): Promise<string | undefined> {
  if (!isInstalled()) return undefined;
  try {
    const r = await run(graphifyBin(), ['--version'], { timeout: 4000, maxBuffer: 1 << 20 });
    const line = String(r.stdout || r.stderr || '').trim().split('\n')[0];
    return line ? line.slice(0, 80) : 'installed';
  } catch { return 'installed'; }
}

interface ReviewRow { id: string; from: string; to: string; reason: string; status: 'pending' | 'kept' | 'dropped' }

export function loadReview(rt: WorkspaceRuntime): ReviewRow[] {
  const raw = readJson<ReviewRow[] | null>(reviewFile(rt), null);
  return Array.isArray(raw) ? raw.filter((r) => r && r.id && r.from && r.to) : [];
}

function saveReview(rt: WorkspaceRuntime, rows: ReviewRow[]): void {
  mkdirSync(dir(rt), { recursive: true });
  writeJson(reviewFile(rt), rows.slice(0, 500));
}

export function reviewLink(rt: WorkspaceRuntime, id: string, status: 'pending' | 'kept' | 'dropped'): ReviewRow[] {
  const rows = loadReview(rt).map((r) => (r.id === id ? { ...r, status } : r));
  saveReview(rt, rows);
  rt.audit('graphify.review', `${id} ${status}`);
  rt.changed();
  return rows;
}

function communityName(folder: string): string { return basename(folder) || folder; }

interface IndexResult { nodes: MemoryGraphNode[]; edges: MemoryGraphEdge[]; communities: MemoryGraph['communities']; fileCount: number; truncated: boolean; ambiguous: ReviewRow[] }

function indexFolder(folder: string, community: string, color: string, budget: { files: number; folders: number }): IndexResult {
  const root = resolve(expandHome(folder));
  const nodes: MemoryGraphNode[] = [];
  const edges: MemoryGraphEdge[] = [];
  const ambiguous: ReviewRow[] = [];
  const seenNames = new Map<string, string>();
  let fileCount = 0;
  let truncated = false;
  const rootId = 'folder:' + root;
  nodes.push({ id: rootId, label: basename(root) || root, kind: 'folder', community, size: 18 });

  const walk = (path: string, parentId: string, depth: number): void => {
    if (depth > 12) return;
    let entries: string[];
    try { entries = readdirSync(path); } catch { return; }
    entries.sort();
    for (const name of entries) {
      if (SKIP.has(name) || name.startsWith('.')) continue;
      const full = join(path, name);
      let st;
      try { st = statSync(full); } catch { continue; }
      if (st.isDirectory()) {
        if (budget.folders-- <= 0) { truncated = true; continue; }
        const id = 'folder:' + full;
        nodes.push({ id, label: name, kind: 'folder', community, size: 12 });
        edges.push({ from: parentId, to: id, kind: 'contains', weight: 1 });
        walk(full, id, depth + 1);
        continue;
      }
      if (!st.isFile()) continue;
      if (budget.files-- <= 0) { truncated = true; continue; }
      fileCount++;
      const id = 'file:' + full;
      const ext = extname(name).toLowerCase();
      nodes.push({ id, label: name, kind: 'file', community, size: Math.min(10, 2 + Math.log2(1 + st.size / 1024)) });
      edges.push({ from: parentId, to: id, kind: 'contains', weight: 1 });
      // Same file name in two places is a real ambiguity: Graphify would queue it for review.
      const prev = seenNames.get(name);
      if (prev && prev !== id) {
        const aid = 'amb_' + ambiguous.length;
        ambiguous.push({ id: aid, from: prev, to: id, reason: 'Two files share the name “' + name + '”', status: 'pending' });
        edges.push({ from: prev, to: id, kind: 'same-name', weight: 0.5, ambiguous: true });
      } else seenNames.set(name, id);
      void ext;
    }
  };
  walk(root, rootId, 0);
  if (truncated) {
    nodes.push({ id: 'note:truncated', label: '…index truncated', kind: 'concept', community, size: 8 });
    edges.push({ from: rootId, to: 'note:truncated', kind: 'note' });
  }
  return { nodes, edges, communities: [{ id: community, name: communityName(root), size: fileCount, color }], fileCount, truncated, ambiguous };
}

function buildIndex(folders: string[]): MemoryGraph {
  const nodes: MemoryGraphNode[] = [];
  const edges: MemoryGraphEdge[] = [];
  const communities: MemoryGraph['communities'] = [];
  const ambiguous: ReviewRow[] = [];
  let fileCount = 0;
  let truncated = false;
  folders.slice(0, 20).forEach((folder, i) => {
    const id = 'c' + i;
    const res = indexFolder(folder, id, palette(i), { files: Math.floor(MAX_FILES / Math.max(1, folders.length)), folders: 400 });
    nodes.push(...res.nodes);
    edges.push(...res.edges);
    communities.push(...res.communities);
    ambiguous.push(...res.ambiguous);
    fileCount += res.fileCount;
    truncated ||= res.truncated;
  });
  return { source: 'harness-index', builtAt: Date.now(), nodes: nodes.slice(0, MAX_NODES), edges: edges.slice(0, MAX_NODES * 2), communities, ambiguous, folders, fileCount, truncated };
}

function kindOf(raw: any, label: string): MemoryGraphNode['kind'] {
  const k = String(raw.kind || raw.type || raw.nodeType || '').toLowerCase();
  if (/folder|dir|package|module/.test(k)) return 'folder';
  if (/file|document|source/.test(k)) return 'file';
  if (/symbol|function|class|method|variable/.test(k)) return 'symbol';
  if (/community|cluster|group/.test(k)) return 'community';
  return extname(label) ? 'file' : 'concept';
}

function parseReal(file: string, folders: string[]): MemoryGraph | null {
  let raw: any;
  try { raw = JSON.parse(readText(file)); } catch { return null; }
  const rawNodes: any[] = Array.isArray(raw?.nodes) ? raw.nodes : Array.isArray(raw?.vertices) ? raw.vertices : [];
  const rawEdges: any[] = Array.isArray(raw?.edges) ? raw.edges : Array.isArray(raw?.links) ? raw.links : [];
  if (!rawNodes.length) return null;
  const idOf = (n: any, i: number): string => String(n.id ?? n.key ?? n.name ?? n.label ?? 'n' + i);
  const nodes: MemoryGraphNode[] = [];
  const communityIndex = new Map<string, number>();
  const communities: MemoryGraph['communities'] = [];
  for (let i = 0; i < rawNodes.length && nodes.length < MAX_NODES; i++) {
    const n = rawNodes[i];
    const label = String(n.label ?? n.name ?? n.title ?? n.id ?? 'node');
    const cName = String(n.community ?? n.cluster ?? n.group ?? n.communityId ?? 'general');
    if (!communityIndex.has(cName)) {
      communityIndex.set(cName, communities.length);
      communities.push({ id: 'c' + communities.length, name: cName, size: 0, color: palette(communities.length) });
    }
    const ci = communityIndex.get(cName) as number;
    communities[ci].size++;
    nodes.push({
      id: idOf(n, i), label, kind: kindOf(n, label), community: 'c' + ci,
      size: typeof n.size === 'number' ? n.size : typeof n.weight === 'number' ? n.weight : undefined,
      x: typeof n.x === 'number' ? n.x : undefined,
      y: typeof n.y === 'number' ? n.y : undefined,
      path: typeof (n.source_file ?? n.file ?? n.path) === 'string' ? resolve(dirname(dirname(file)), n.source_file ?? n.file ?? n.path) : undefined,
    });
  }
  const known = new Set(nodes.map((n) => n.id));
  const edges: MemoryGraphEdge[] = [];
  const ambiguous: ReviewRow[] = [];
  for (const e of rawEdges) {
    const from = String(e.source ?? e.from ?? e.src ?? '');
    const to = String(e.target ?? e.to ?? e.dst ?? '');
    if (!from || !to || !known.has(from) || !known.has(to)) continue;
    const edge: MemoryGraphEdge = { from, to, kind: String(e.relation ?? e.label ?? e.type ?? 'link'), weight: typeof e.weight === 'number' ? e.weight : 1, ambiguous: !!e.ambiguous };
    edges.push(edge);
    if (edge.ambiguous) ambiguous.push({ id: 'amb_' + ambiguous.length, from, to, reason: String(e.reason || 'Graphify marked this link ambiguous'), status: 'pending' });
  }
  return { source: 'graphify', builtAt: Date.now(), nodes, edges, communities, ambiguous, folders, fileCount: nodes.filter((n) => n.kind === 'file').length, truncated: rawNodes.length > MAX_NODES };
}

/** Build (or rebuild) the graph for this workspace. Real Graphify output wins when it is there. */
export function buildMemoryGraph(rt: WorkspaceRuntime): MemoryGraph {
  const folders = rt.ws.graphifyFolders.slice();
  const real = realGraphFile(rt);
  const graph = (real ? parseReal(real, folders) : null) || buildIndex(folders);
  const generated = graph.ambiguous || [];
  const stored = loadReview(rt);
  const rows = new Map(stored.map((r) => [r.id, r]));
  for (const a of generated) {
    const id = 'amb_' + createHash('sha1').update(a.from + '\n' + a.to).digest('hex').slice(0, 10);
    if (![...rows.values()].some((r) => (r.from === a.from && r.to === a.to) || r.id === id)) rows.set(id, { ...a, id, status: 'pending' });
  }
  const all = [...rows.values()].slice(0, 500);
  saveReview(rt, all);
  graph.ambiguous = all.map((r) => ({ ...r }));
  graph.edges = graph.edges.map((e) => {
    const row = all.find((r) => r.from === e.from && r.to === e.to);
    return row ? { ...e, ambiguous: true } : e;
  }).filter((e) => {
    const row = all.find((r) => r.from === e.from && r.to === e.to);
    return !(row && row.status === 'dropped');
  });
  graph.folders = folders;
  graph.builtAt = Date.now();
  mkdirSync(dir(rt), { recursive: true });
  writeJson(cacheFile(rt), graph);
  rt.audit('graphify.build', `${graph.source} · ${graph.nodes.length} nodes · ${graph.edges.length} edges`);
  // Chats on the map list graph nodes, so a map on screen has to drop any that are gone.
  rt.emit({ type: 'map', workspace: rt.id, scope: 'ws' });
  return graph;
}

export function getMemoryGraph(rt: WorkspaceRuntime, rebuild = false): MemoryGraph {
  const cached = readJson<MemoryGraph | null>(cacheFile(rt), null);
  const real = realGraphFile(rt);
  const foldersChanged = !!cached && JSON.stringify(cached.folders) !== JSON.stringify(rt.ws.graphifyFolders);
  let stale = rebuild || !cached || foldersChanged;
  if (!stale && cached && !real && cached.source === 'graphify') stale = true;
  if (!stale && cached && real) {
    try { stale = cached.source !== 'graphify' || statSync(real).mtimeMs > cached.builtAt; } catch { /* ignore */ }
  }
  return stale ? buildMemoryGraph(rt) : (cached as MemoryGraph);
}

const STOP = new Set('about above after again against also because been before being below between both could does doing down during each from further have having here into itself just more most other over same should some such than that their them then there these they this those through under until very were what when where which while will with would your yours make made want need please show tell help like using used want find look file files code'.split(' '));

/** The words in a message worth looking up: names, paths and longer words, minus common ones. */
export function queryTerms(text: string): string[] {
  const words = text.toLowerCase().match(/[a-z0-9][a-z0-9_./-]*[a-z0-9]/g) || [];
  return [...new Set(words.filter((w) => (w.length >= 4 || /[._/-]/.test(w)) && !STOP.has(w)))].slice(0, 16);
}

/** Nodes ranked by how many terms their label or id contains (names count double). */
export function rankNodes(graph: MemoryGraph, terms: string[], limit: number): MemoryGraphNode[] {
  if (!terms.length) return [];
  const scored: { n: MemoryGraphNode; s: number }[] = [];
  for (const n of graph.nodes) {
    const label = n.label.toLowerCase(), id = n.id.toLowerCase();
    let s = 0;
    for (const t of terms) s += label === t ? 3 : label.includes(t) ? 2 : id.includes(t) ? 1 : 0;
    if (s) scored.push({ n, s: s + (n.kind === 'community' ? -0.5 : 0) + Math.min(1, (n.size || 0) / 40) });
  }
  return scored.sort((a, b) => b.s - a.s).slice(0, limit).map((x) => x.n);
}

let memo: { file: string; mtime: number; graph: MemoryGraph } | null = null;
/** The last built graph, without building one (fast enough to run on every message). */
export function cachedMemoryGraph(rt: WorkspaceRuntime): MemoryGraph | null {
  const file = cacheFile(rt);
  let mtime = 0;
  try { mtime = statSync(file).mtimeMs; } catch { return null; }
  if (memo?.file === file && memo.mtime === mtime) return memo.graph;
  const graph = readJson<MemoryGraph | null>(file, null);
  memo = graph ? { file, mtime, graph } : null;
  return graph;
}

export function queryMemoryGraph(rt: WorkspaceRuntime, q: string): { graph: MemoryGraph; matches: MemoryGraphNode[]; edges: MemoryGraphEdge[] } {
  const graph = getMemoryGraph(rt);
  const needle = q.trim().toLowerCase();
  const exact = needle ? graph.nodes.filter((n) => n.label.toLowerCase().includes(needle) || n.id.toLowerCase().includes(needle)).slice(0, 24) : [];
  // A question rather than a name: rank by its words.
  const matches = !needle ? [...graph.nodes].sort((a, b) => (b.size || 0) - (a.size || 0)).slice(0, 12)
    : exact.length ? exact : rankNodes(graph, queryTerms(needle), 24);
  const ids = new Set(matches.map((n) => n.id));
  const edges = graph.edges.filter((e) => ids.has(e.from) || ids.has(e.to)).slice(0, 120);
  return { graph, matches, edges };
}


/** What a node connects to, closest first as the graph lists them. */
export function nodeLinks(graph: MemoryGraph, id: string, limit: number): MemoryNodeDetail['links'] {
  const byId = new Map(graph.nodes.map((n) => [n.id, n]));
  const out: MemoryNodeDetail['links'] = [];
  for (const e of graph.edges) {
    if (out.length >= limit) break;
    if (e.from !== id && e.to !== id) continue;
    const other = byId.get(e.from === id ? e.to : e.from);
    if (other && other.id !== id) out.push({ id: other.id, label: other.label, kind: other.kind, relation: e.kind });
  }
  return out;
}

const PREVIEW_TEXT = 200 * 1024, PREVIEW_MAX = 2 * 1024 * 1024, PREVIEW_IMAGE = 5 * 1024 * 1024;
const IMAGE_TYPE: Record<string, string> = { '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.webp': 'image/webp', '.svg': 'image/svg+xml' };
const SECRET = /^\.env|^id_(rsa|dsa|ecdsa|ed25519)$|\.(pem|key|p12|pfx)$/i;

/** A file's preview. graph.json is only data, so a path counts only if it really is inside a mapped folder. */
function previewFile(rt: WorkspaceRuntime, path: string): NonNullable<MemoryNodeDetail['file']> {
  const none = (note: string, size = 0) => ({ path, size, note });
  let real: string;
  try { real = realpathSync(path); } catch { return none('This file is no longer there.'); }
  const inside = rt.ws.graphifyFolders.some((f) => { try { return real.startsWith(realpathSync(expandHome(f)) + sep); } catch { return false; } });
  if (!inside) return none('This file is outside the mapped folders, so it isn’t shown.');
  if (SECRET.test(basename(real))) return none('This looks like a secrets file, so it isn’t shown.');
  const stat = statSync(real);
  const size = stat.size;
  if (!stat.isFile()) return none('Only files can be previewed.');
  const type = IMAGE_TYPE[extname(real).toLowerCase()];
  if (type) return size > PREVIEW_IMAGE ? none('This image is too large to preview.', size) : { path, size, image: `data:${type};base64,${readFileSync(real).toString('base64')}` };
  if (size > PREVIEW_MAX) return none('This file is too large to preview.', size);
  const bytes = readFileSync(real);
  if (bytes.includes(0)) return none('This is a binary file.', size);
  return { path, size, text: bytes.subarray(0, PREVIEW_TEXT).toString('utf8'), truncated: size > PREVIEW_TEXT };
}

function readPreview(rt: WorkspaceRuntime, path: string): NonNullable<MemoryNodeDetail['file']> {
  try { return previewFile(rt, path); } catch (e) { return { path, size: 0, note: 'This file can’t be read (' + ((e as NodeJS.ErrnoException).code || 'error') + ').' }; }
}

export function memoryNodeDetail(rt: WorkspaceRuntime, id: string): MemoryNodeDetail | null {
  const graph = getMemoryGraph(rt);
  const node = graph.nodes.find((n) => n.id === id);
  if (!node) return null;
  const path = node.path || (node.kind === 'file' && id.startsWith('file:') ? id.slice(5) : undefined);
  return { node, community: graph.communities.find((c) => c.id === node.community)?.name, links: nodeLinks(graph, id, 40), ...(path ? { file: readPreview(rt, path) } : {}) };
}

// ---------- running Graphify itself (R-23) ----------

/**
 * Environment for a Graphify run. Its docs pass may only use a model from this workspace's allowed providers:
 * Ollama → backend ollama; Anthropic with a key → claude; an OpenAI-compatible provider with a key → openai at its URL;
 * nothing suitable → GRAPHIFY_BACKEND=none (code-only graph, nothing leaves the machine). No other workspace's keys,
 * and none of the parent process's, are inherited.
 */
export function graphifyEnv(rt: WorkspaceRuntime): Record<string, string> {
  const env: Record<string, string> = { ...baseEnv() };
  const providers = effectiveProviders(rt.ws, rt.policy);
  const key = (name?: string) => (name ? keychain.get(rt.id, name) : null);
  const ollama = providers.find((p) => p.preset === 'ollama' || /11434/.test(p.baseURL || ''));
  const anthropic = providers.find((p) => (p.id === 'anthropic' || p.preset === 'anthropic') && key(p.keyEnv));
  const compat = providers.find((p) => p.baseURL && /openai/.test(p.npm || '') && key(p.keyEnv) && !/11434/.test(p.baseURL));
  if (anthropic) { env.GRAPHIFY_BACKEND = 'claude'; env.ANTHROPIC_API_KEY = key(anthropic.keyEnv) as string; }
  else if (compat) { env.GRAPHIFY_BACKEND = 'openai'; env.OPENAI_API_KEY = key(compat.keyEnv) as string; env.OPENAI_BASE_URL = compat.baseURL as string; }
  else if (ollama) { env.GRAPHIFY_BACKEND = 'ollama'; env.OLLAMA_BASE_URL = (ollama.baseURL || 'http://localhost:11434').replace(/\/v1\/?$/, ''); }
  else env.GRAPHIFY_BACKEND = 'none';
  return env;
}

function graphifyBin(): string { return process.env.HARNESS_GRAPHIFY_BIN || (existsSync(venvBin()) ? venvBin() : 'graphify'); }

export function graphifyInstalled(): boolean {
  if (process.env.HARNESS_GRAPHIFY_BIN) return existsSync(process.env.HARNESS_GRAPHIFY_BIN);
  return isInstalled();
}

/** `graphify extract <folder>` (full) or `graphify update <folder>` (changed files only, SHA-256 cache). */
export function runGraphify(rt: WorkspaceRuntime, folder: string, mode: 'extract' | 'update'): Promise<{ code: number; output: string }> {
  const cwd = resolve(expandHome(folder));
  return new Promise((done) => {
    const p = spawn(graphifyBin(), [mode, cwd], { cwd, env: graphifyEnv(rt), stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    const t = setTimeout(() => p.kill('SIGKILL'), 30 * 60000);
    p.stdout.on('data', (b) => { out = (out + b).slice(-8000); });
    p.stderr.on('data', (b) => { out = (out + b).slice(-8000); });
    p.on('error', (e) => { clearTimeout(t); done({ code: -1, output: String(e.message) }); });
    p.on('close', (code) => { clearTimeout(t); rt.audit('graphify.' + mode, `${cwd} · exit ${code} · backend ${graphifyEnv(rt).GRAPHIFY_BACKEND}`, 'workspaceai'); done({ code: code ?? -1, output: out }); });
  });
}

/** Rebuild: run Graphify on every folder when it is installed, then load what it wrote (else the harness index). */
export async function rebuildMemoryGraph(rt: WorkspaceRuntime, mode: 'extract' | 'update' = 'extract'): Promise<MemoryGraph & { runs?: { folder: string; code: number; output: string }[] }> {
  const runs: { folder: string; code: number; output: string }[] = [];
  if (graphifyInstalled()) for (const f of rt.ws.graphifyFolders) runs.push({ folder: f, ...(await runGraphify(rt, f, mode)) });
  const g = buildMemoryGraph(rt);
  rt.changed();
  return { ...g, runs };
}

/**
 * Watch a folder tree one directory at a time, leaving out SKIP, dot folders and Graphify's output. Node's recursive
 * fs.watch on Linux takes an inotify watch for every file, node_modules included, so a big repo exhausts the system
 * limit, and that failure arrives as an 'error' event that kills the process. Hitting the limit here only logs.
 */
export function watchTree(root: string, onChange: (rel: string) => void): { close(): void } {
  const open = new Map<string, FSWatcher>();
  const skip = (name: string) => SKIP.has(name) || name.startsWith('.') || name.startsWith('graphify-out');
  let warned = false;
  const warn = (dir: string, e: unknown) => {
    if (!warned && (e as NodeJS.ErrnoException).code !== 'ENOENT') console.error(`[watch] incomplete under ${root}: ${dir}: ${e}`);
    warned = true;
  };
  const add = (dir: string): void => {
    if (open.has(dir)) return;
    try {
      const w = watch(dir, (_e, name) => {
        const n = String(name || '');
        if (!n || skip(n)) return;
        const child = join(dir, n);
        let st;
        try { st = lstatSync(child, { throwIfNoEntry: false }); } catch { return; }
        if (st?.isDirectory()) add(child);
        else if (!st) for (const [p, x] of open) if (p === child || p.startsWith(child + sep)) { x.close(); open.delete(p); }
        onChange(relative(root, child));
      });
      w.on('error', (e) => { warn(dir, e); w.close(); open.delete(dir); });
      open.set(dir, w);
    } catch (e) { warn(dir, e); return; }
    let entries: Dirent[] = [];
    try { entries = readdirSync(dir, { withFileTypes: true }); } catch { /* a file, or it vanished */ }
    for (const e of entries) if (e.isDirectory() && !skip(e.name)) add(join(dir, e.name));
  };
  add(root);
  return { close() { for (const w of open.values()) w.close(); open.clear(); } };
}

/** Watch mode: changes in a mapped folder trigger an incremental update a few seconds later. */
export class GraphifyWatcher {
  private watchers = new Map<string, { key: string; list: { close(): void }[] }>();
  sync(rt: WorkspaceRuntime): void {
    const want = rt.ws.graphifyWatch && (rt.ws.memory === 'graph' || rt.ws.memory === 'both') && rt.ws.graphifyFolders.length > 0;
    const key = want ? JSON.stringify(rt.ws.graphifyFolders) : '';
    const cur = this.watchers.get(rt.id);
    if (cur && cur.key === key) return;
    cur?.list.forEach((w) => w.close());
    this.watchers.delete(rt.id);
    if (!want) return;
    let timer: NodeJS.Timeout | null = null;
    const list: { close(): void }[] = [];
    for (const f of rt.ws.graphifyFolders) {
      const folder = resolve(expandHome(f));
      if (!existsSync(folder)) continue;
      list.push(watchTree(folder, () => {
        if (timer) clearTimeout(timer);
        timer = setTimeout(() => { void rebuildMemoryGraph(rt, 'update'); }, 3000);
      }));
    }
    this.watchers.set(rt.id, { key, list });
  }
  stop(): void { for (const w of this.watchers.values()) w.list.forEach((x) => x.close()); this.watchers.clear(); }
}

/**
 * User-triggered install with the MCP and watch extras: as a uv tool, else with pipx, else into a Python
 * environment of the harness's own (~/.harness/vendor/graphify) so it works with nothing but python3.
 */
export async function installGraphify(): Promise<{ ok: boolean; output: string }> {
  const pkg = 'graphifyy[mcp,watch]';
  const opts = { timeout: 15 * 60000, maxBuffer: 8 << 20 };
  const tool = await run('sh', ['-c', 'command -v uv || command -v pipx']).then((r) => r.stdout.trim()).catch(() => '');
  try {
    if (tool) { const r = await run(tool, tool.endsWith('uv') ? ['tool', 'install', pkg] : ['install', pkg], opts); return { ok: true, output: (r.stdout + r.stderr).slice(-2000) }; }
    const python = await run('sh', ['-c', 'command -v python3 || command -v python']).then((r) => r.stdout.trim()).catch(() => '');
    if (!python) return { ok: false, output: 'graphify needs Python 3.10 or newer. Install Python (or uv: https://docs.astral.sh/uv), then try again.' };
    const venv = graphifyVenv();
    if (!existsSync(join(venv, 'bin', 'pip'))) { mkdirSync(venv, { recursive: true }); await run(python, ['-m', 'venv', venv], opts); }
    const r = await run(join(venv, 'bin', 'pip'), ['install', '--upgrade', pkg], opts);
    if (!existsSync(venvBin())) return { ok: false, output: 'pip finished but no graphify command was installed.\n' + (r.stdout + r.stderr).slice(-1800) };
    return { ok: true, output: `Installed into ${venv} (no uv or pipx found).\n` + (r.stdout + r.stderr).slice(-1800) };
  } catch (e) { return { ok: false, output: String((e as Error).message).slice(-2000) }; }
}
