// Project mind maps. A map has two layers: what the workspace already knows (chats, artifacts, graphs, runs, facts,
// routines), derived on every read so it is always current; and what was added on the map (own nodes, moved items,
// links, hidden items), saved in the project folder. Positions are computed by each view and never saved.
import { join } from 'node:path';
import type { MapLink, MapOp, MapOwnNode, MapSide, MapTaskStatus, ProjectMap, ResolvedMap, ResolvedMapNode } from '../../shared/src/types.ts';
import { chatsWithin, childProjects, rolledUpArtifacts, uid } from '../../shared/src/util.ts';
import { fromMarkdown, type OutlineItem } from '../../shared/src/mindmap.ts';
import { readJson, writeJson } from './jsonfile.ts';
import { chatMemory } from './recall.ts';
import { cachedMemoryGraph, nodeLinks } from './graphify.ts';
import { expandHome } from './paths.ts';
import { repoNodes } from './gitlab.ts';
import type { WorkspaceRuntime } from './runtime.ts';

/** `ws` is the workspace overview; anything else is a project id. */
export type MapScope = string;

const fail = (message: string, status = 400): never => { throw Object.assign(new Error(message), { status }); };
const EMPTY = (): ProjectMap => ({ nodes: [], placements: {}, links: [], hidden: [], layout: 'balance' });

function mapFile(rt: WorkspaceRuntime, scope: MapScope): string {
  if (scope === 'ws') return join(rt.paths.data, 'map.json');
  const p = rt.project(scope) ?? fail('Project not found', 404);
  return join(expandHome(p!.folder), '.harness', 'map.json');
}

export function loadMap(rt: WorkspaceRuntime, scope: MapScope): ProjectMap {
  return { ...EMPTY(), ...readJson<Partial<ProjectMap>>(mapFile(rt, scope), {}) };
}

const when = (t: number) => new Date(t).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });

function projectCounts(rt: WorkspaceRuntime, pid: string): string {
  const { projects, chats, artifacts, graphs } = rt.state;
  const parts = [[chatsWithin(projects, chats, pid).length, 'chat'], [rolledUpArtifacts(projects, artifacts, pid).length, 'artifact'], [graphs.filter((g) => g.project === pid).length, 'graph']] as [number, string][];
  return parts.filter(([n]) => n).map(([n, w]) => `${n} ${w}${n === 1 ? '' : 's'}`).join(' · ');
}

/** The workspace's data as a tree, before anything done on the map. */
export function deriveMap(rt: WorkspaceRuntime, scope: MapScope): { root: string; nodes: ResolvedMapNode[]; links: MapLink[] } {
  const nodes: ResolvedMapNode[] = [];
  const links: MapLink[] = [];
  if (scope === 'ws') {
    const root = 'workspace:' + rt.id;
    nodes.push({ id: root, kind: 'workspace', title: rt.ws.name, parent: null, order: 0 });
    const walk = (pid: string | null, parent: string) => {
      for (const p of childProjects(rt.state.projects, pid).filter((x) => !x.archived)) {
        nodes.push({ id: 'project:' + p.id, kind: 'project', ref: p.id, title: p.name, meta: projectCounts(rt, p.id), parent, order: p.createdAt });
        walk(p.id, 'project:' + p.id);
      }
    };
    walk(null, root);
    return { root, nodes, links };
  }
  const project = rt.project(scope) ?? fail('Project not found', 404);
  const pid = project!.id;
  const root = 'project:' + pid;
  nodes.push({ id: root, kind: 'project', ref: pid, title: project!.name, parent: null, order: 0 });

  for (const p of childProjects(rt.state.projects, pid).filter((x) => !x.archived)) nodes.push({ id: 'project:' + p.id, kind: 'project', ref: p.id, title: p.name, meta: projectCounts(rt, p.id) || 'empty', parent: root, order: p.createdAt });

  const chats = rt.state.chats.filter((c) => c.project === pid);
  const graphs = rt.state.graphs.filter((g) => g.project === pid);
  const graphIds = new Set(graphs.map((g) => g.id));
  const chatIds = new Set(chats.map((c) => c.id));
  const runs = rt.listStoredRuns().map((r) => r.run).filter((r) => graphIds.has(r.graph) || (r.from && chatIds.has(r.from)));
  const runChats = new Set(runs.flatMap((r) => [...(r.chats || []), ...(r.chat ? [r.chat] : [])]));

  // Runs sit under the chat that started them, or else under their graph; only the latest of each.
  const perParent = new Map<string, number>();
  const shownRuns = new Set<string>();
  const chatParent = new Map<string, string>();
  for (const r of [...runs].sort((a, b) => b.startedAt - a.startedAt)) {
    const parent = r.from && chatIds.has(r.from) ? 'chat:' + r.from : graphIds.has(r.graph) ? 'graph:' + r.graph : '';
    if (!parent) continue;
    const seen = perParent.get(parent) || 0;
    if (seen >= (parent.startsWith('chat:') ? 10 : 5)) continue;
    perParent.set(parent, seen + 1);
    shownRuns.add(r.id);
    const name = rt.state.graphs.find((g) => g.id === r.graph)?.name || 'Graph';
    nodes.push({ id: 'run:' + r.id, kind: 'run', ref: r.id, title: `Run of ${name}`, meta: `${r.dryRun ? 'dry · ' : ''}${r.status} · ${when(r.startedAt)}`, parent, order: r.startedAt });
    for (const c of [...(r.chats || []), ...(r.chat ? [r.chat] : [])]) if (chatIds.has(c) && !chatParent.has(c) && c !== r.from) chatParent.set(c, 'run:' + r.id);
  }

  // A graph run's own chats belong to the run; ones whose run is no longer shown are gathered in one place.
  let looseRunChats = false;
  for (const c of chats) {
    const isRunChat = runChats.has(c.id) || c.title.startsWith('Run · ');
    const parent = chatParent.get(c.id) || (isRunChat ? 'group:run-chats' : root);
    if (parent === 'group:run-chats') looseRunChats = true;
    nodes.push({ id: 'chat:' + c.id, kind: 'chat', ref: c.id, title: c.title, meta: c.agent === 'opencode' ? 'opencode' : rt.ws.agents.find((a) => a.id === c.agent)?.name || c.agent, parent, order: c.createdAt ?? c.updatedAt });
  }
  // What Graphify holds on each chat, folded under it.
  for (const c of chats) {
    const related = chatMemory(rt, c);
    if (!related.length) continue;
    const group = 'group:chat-memory:' + c.id;
    nodes.push({ id: group, kind: 'group', title: 'Memory', meta: `${related.length} related`, parent: 'chat:' + c.id, order: Number.MAX_SAFE_INTEGER - 2, folded: true });
    const graph = cachedMemoryGraph(rt)!;
    related.forEach((n, i) => {
      const id = `memory:${c.id}:${n.id}`;
      const near = nodeLinks(graph, n.id, 8);
      nodes.push({ id, kind: 'memory', ref: n.id, title: n.label, meta: n.kind, parent: group, order: i, folded: near.length > 0 });
      near.forEach((o, k) => nodes.push({ id: `${id}>${o.id}`, kind: 'memory', ref: o.id, title: o.label, meta: `${o.relation} · ${o.kind}`, parent: id, order: k }));
    });
  }
  if (looseRunChats) nodes.push({ id: 'group:run-chats', kind: 'group', title: 'Earlier run chats', parent: root, order: Number.MAX_SAFE_INTEGER - 1, folded: true });

  for (const a of rt.state.artifacts.filter((x) => x.project === pid)) nodes.push({ id: 'artifact:' + a.id, kind: 'artifact', ref: a.id, subtype: a.type, title: a.title, meta: `${a.type.toLowerCase()} v${a.version}`, parent: a.chat && chatIds.has(a.chat) ? 'chat:' + a.chat : root, order: a.createdAt });
  for (const g of graphs) nodes.push({ id: 'graph:' + g.id, kind: 'graph', ref: g.id, title: g.name, meta: g.file.split('/').slice(-2).join('/'), parent: g.chat && chatIds.has(g.chat) ? 'chat:' + g.chat : root, order: g.updatedAt });

  for (const r of rt.state.routines.filter((x) => x.project === pid)) {
    nodes.push({ id: 'routine:' + r.id, kind: 'routine', ref: r.id, title: r.name, meta: r.phrase, parent: root, order: Number.MAX_SAFE_INTEGER - 3 });
    // What a routine runs is linked to it.
    const target = r.what.kind === 'graph' ? 'graph:' + r.what.graph : r.what.kind === 'artifact' ? 'artifact:' + r.what.artifact : '';
    if (target) links.push({ id: `auto:${r.id}`, from: 'routine:' + r.id, to: target, label: 'runs' });
  }

  nodes.push(...repoNodes(rt, scope, project!.folder, root));

  const facts = rt.scopeFacts(pid).filter((f) => f.status === 'approved');
  if (facts.length) {
    nodes.push({ id: 'group:memory', kind: 'group', title: 'Memory', meta: `${facts.length} fact${facts.length === 1 ? '' : 's'}`, parent: root, order: Number.MAX_SAFE_INTEGER - 2, folded: true });
    for (const f of facts) nodes.push({ id: 'fact:' + f.id, kind: 'fact', ref: f.id, title: f.text.length > 90 ? f.text.slice(0, 89) + '…' : f.text, parent: 'group:memory', order: f.createdAt });
  }
  return { root, nodes, links };
}

/** Lay what was done on the map over the derived tree. */
function merge(scope: MapScope, derived: ReturnType<typeof deriveMap>, stored: ProjectMap): ResolvedMap {
  const hidden = new Set(stored.hidden);
  hidden.delete(derived.root);
  const all = new Map<string, ResolvedMapNode>();
  for (const n of derived.nodes) all.set(n.id, { ...n });
  for (const n of stored.nodes) all.set(n.id, { id: n.id, kind: n.kind, title: n.text, parent: n.parent, order: n.order, own: true, ...(n.side ? { side: n.side } : {}), ...(n.status ? { status: n.status } : {}), ...(n.kind === 'task' ? { status: n.status || 'todo' } : {}) });
  for (const [id, p] of Object.entries(stored.placements)) {
    const n = all.get(id);
    if (!n || id === derived.root || !all.has(p.parent) || hidden.has(p.parent)) continue;
    n.parent = p.parent; n.order = p.order;
    if (p.side) n.side = p.side;
  }
  for (const id of hidden) all.delete(id);
  const placed = (n: ResolvedMapNode) => !!n.own || !!stored.placements[n.id];
  // Under something hidden or gone: what was made or moved here returns to the root; the rest stays out of view.
  for (let changed = true; changed;) {
    changed = false;
    for (const n of [...all.values()]) {
      if (!n.parent || all.has(n.parent)) continue;
      if (placed(n)) n.parent = derived.root; else all.delete(n.id);
      changed = true;
    }
  }
  // A parent chain can't loop: moves are refused when they would, but derived parents change over time.
  for (const n of all.values()) {
    const seen = new Set<string>([n.id]);
    for (let p = n.parent; p; p = all.get(p)?.parent ?? null) {
      if (seen.has(p)) { n.parent = derived.root; break; }
      seen.add(p);
    }
  }
  const links = [...derived.links, ...stored.links].filter((l) => all.has(l.from) && all.has(l.to));
  const hiddenItems = derived.nodes.filter((n) => hidden.has(n.id)).map((n) => ({ id: n.id, title: n.title }));
  return { scope, root: derived.root, layout: stored.layout, nodes: [...all.values()], links, ...(hiddenItems.length ? { hiddenItems } : {}) };
}

export function resolveMap(rt: WorkspaceRuntime, scope: MapScope): ResolvedMap {
  return merge(scope, deriveMap(rt, scope), loadMap(rt, scope));
}

/** Apply edits in order; every one is checked. Returns the map after them and the ids of the nodes added. */
export function applyOps(rt: WorkspaceRuntime, scope: MapScope, ops: MapOp[]): { map: ResolvedMap; added: string[] } {
  const derived = deriveMap(rt, scope);
  const stored = loadMap(rt, scope);
  const natural = new Set(derived.nodes.map((n) => n.id));
  const added: string[] = [];
  const current = () => merge(scope, derived, stored);
  const siblings = (m: ResolvedMap, parent: string, except?: string) => m.nodes.filter((n) => n.parent === parent && n.id !== except).sort((a, b) => a.order - b.order || a.id.localeCompare(b.id));
  const setOrder = (id: string, order: number, parent: string, side?: MapSide) => {
    const own = stored.nodes.find((n) => n.id === id);
    if (own) { own.order = order; own.parent = parent; if (side) own.side = side; return; }
    const prev = stored.placements[id];
    stored.placements[id] = { parent, order, ...(side || prev?.side ? { side: side || prev?.side } : {}) };
  };
  /** An order key that places a node before `before`, or last. Renumbers when two keys are too close to split. */
  const orderFor = (parent: string, before: string | undefined, except?: string): number => {
    let m = current();
    let list = siblings(m, parent, except);
    if (!before) return list.length ? list[list.length - 1].order + 1000 : Date.now();
    let i = list.findIndex((n) => n.id === before);
    if (i < 0) fail('That position is not on this branch');
    if (i > 0 && list[i].order - list[i - 1].order < 1e-6) {
      list.forEach((n, k) => setOrder(n.id, k * 1000, parent));
      m = current(); list = siblings(m, parent, except); i = list.findIndex((n) => n.id === before);
    }
    return i === 0 ? list[0].order - 1000 : (list[i - 1].order + list[i].order) / 2;
  };
  const needNode = (m: ResolvedMap, id: string) => m.nodes.find((n) => n.id === id) ?? fail(`There is no node ${id} on this map — read the map for its ids`, 404);
  const addOwn = (parent: string, kind: MapOwnNode['kind'], text: string, o: { status?: MapTaskStatus; side?: MapSide; before?: string; by?: 'user' | 'agent' }) => {
    const t = text.trim();
    if (!t) fail('A node needs some text');
    const n: MapOwnNode = { id: uid('mn'), kind, text: t.slice(0, 500), parent, order: orderFor(parent, o.before), createdBy: o.by || 'user', createdAt: Date.now(), ...(kind === 'task' ? { status: o.status || 'todo' } : {}), ...(o.side && parent === derived.root ? { side: o.side } : {}) };
    stored.nodes.push(n);
    added.push(n.id);
    return n;
  };
  for (const op of ops) {
    const m = current();
    switch (op.op) {
      case 'add': {
        needNode(m, op.parent);
        addOwn(op.parent, op.kind || (op.status ? 'task' : 'idea'), op.text, op);
        break;
      }
      case 'import': {
        needNode(m, op.parent);
        const walk = (items: OutlineItem[], parent: string) => { for (const it of items) walk(it.children, addOwn(parent, it.status ? 'task' : 'idea', it.text, { status: it.status, by: op.by }).id); };
        const items = fromMarkdown(op.markdown);
        if (!items.length) fail('Nothing to add: write it as a nested bullet list');
        walk(items, op.parent);
        break;
      }
      case 'update': {
        const n = stored.nodes.find((x) => x.id === op.id) ?? fail('Only nodes made on the map can be edited; rename the item itself', 409);
        if (op.text !== undefined) { const t = op.text.trim(); if (!t) fail('A node needs some text'); n!.text = t.slice(0, 500); }
        if (op.kind) n!.kind = op.kind;
        if (op.status !== undefined) { if (op.status) { n!.status = op.status; n!.kind = 'task'; } else delete n!.status; }
        if (n!.kind === 'task' && !n!.status) n!.status = 'todo';
        break;
      }
      case 'move': {
        if (op.id === derived.root) fail('The centre of the map can’t be moved');
        const n = needNode(m, op.id)!;
        const parent = op.parent || (n.parent as string);
        needNode(m, parent);
        // Refused rather than repaired, so nothing is lost: a node can't go under itself.
        for (let p: string | null | undefined = parent; p; p = m.nodes.find((x) => x.id === p)?.parent) if (p === op.id) fail('A node can’t be moved under itself');
        const side = parent === derived.root ? op.side : undefined;
        const order = op.parent || op.before ? orderFor(parent, op.before, op.id) : n.order;
        setOrder(op.id, order, parent, side);
        if (parent !== derived.root) { const own = stored.nodes.find((x) => x.id === op.id); if (own) delete own.side; else if (stored.placements[op.id]) delete stored.placements[op.id].side; }
        break;
      }
      case 'link': {
        needNode(m, op.from); needNode(m, op.to);
        if (op.from === op.to) fail('A node can’t link to itself');
        if (m.links.some((l) => (l.from === op.from && l.to === op.to) || (l.from === op.to && l.to === op.from))) fail('These are already linked', 409);
        stored.links.push({ id: uid('ml'), from: op.from, to: op.to, ...(op.label?.trim() ? { label: op.label.trim().slice(0, 80) } : {}) });
        break;
      }
      case 'unlink': {
        if (!stored.links.some((l) => l.id === op.id)) fail('That link can’t be removed', 409);
        stored.links = stored.links.filter((l) => l.id !== op.id);
        break;
      }
      case 'hide': {
        if (op.id === derived.root) fail('The centre of the map can’t be hidden');
        if (!natural.has(op.id)) fail('Only items can be hidden; remove nodes made on the map', 409);
        if (!stored.hidden.includes(op.id)) stored.hidden.push(op.id);
        break;
      }
      case 'unhide': { stored.hidden = stored.hidden.filter((x) => x !== op.id); break; }
      case 'remove': {
        const n = stored.nodes.find((x) => x.id === op.id) ?? fail('Only nodes made on the map can be removed; hide items instead', 409);
        // What hung off it moves up; moved items go back to where they belong.
        for (const c of stored.nodes) if (c.parent === op.id) c.parent = n!.parent;
        for (const [id, p] of Object.entries(stored.placements)) if (p.parent === op.id) delete stored.placements[id];
        stored.nodes = stored.nodes.filter((x) => x.id !== op.id);
        break;
      }
      case 'setLayout': {
        if (op.layout !== 'right' && op.layout !== 'balance') fail('Layout must be right or balance');
        stored.layout = op.layout;
        break;
      }
      default: fail('Unknown map edit');
    }
  }
  // Drop what points at items that no longer exist.
  const live = new Set([...natural, ...stored.nodes.map((n) => n.id)]);
  stored.links = stored.links.filter((l) => live.has(l.from) && live.has(l.to));
  stored.hidden = stored.hidden.filter((id) => natural.has(id));
  for (const id of Object.keys(stored.placements)) if (!natural.has(id)) delete stored.placements[id];
  writeJson(mapFile(rt, scope), stored);
  rt.emit({ type: 'map', workspace: rt.id, scope });
  return { map: merge(scope, derived, stored), added };
}
