// A project's mind map: built from the workspace's data, with what was done on the map laid over it.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { existsSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fromMarkdown, layoutMap, LEVEL_GAP, sideOf, SIBLING_GAP, toMarkdown, toOutline } from '../../shared/src/mindmap.ts';
import { graphPreview } from '../../shared/src/bpmnPreview.ts';
import { blankGraph } from '../../orchestrator/src/model.ts';
import type { ResolvedMap, ResolvedMapNode } from '../../shared/src/types.ts';

const home = mkdtempSync(join(tmpdir(), 'harness-map-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'none');

const agent = join(home, 'quiet-acp.cjs');
writeFileSync(agent, `
const rl = require('readline').createInterface({ input: process.stdin });
const send = (m) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...m }) + '\\n');
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.method === 'initialize') send({ id: m.id, result: { protocolVersion: 1, agentCapabilities: {} } });
  else if (m.method === 'session/new') send({ id: m.id, result: { sessionId: 'Q' + m.id } });
  else if (m.id !== undefined) send({ id: m.id, result: { stopReason: 'end_turn' } });
});
`);

let base = '', token = '', sup: any, close = async () => {};
const call = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); return { status: r.status, body: t ? JSON.parse(t) : {} };
};
const ok = async (m: string, p: string, b?: unknown) => { const r = await call(m, p, b); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${JSON.stringify(r.body)}`); return r.body; };

beforeAll(async () => {
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  const { installExtensions } = await import('../src/extensions.ts');
  sup = new Supervisor();
  const router = buildRouter();
  const server = await startServer(sup, router, {});
  installExtensions(sup, router, server);
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); };
});
afterAll(async () => { await close(); });

describe('project maps', () => {
  let W = '', P = '', chat: any, base2 = '';
  const node = (m: ResolvedMap, id: string) => m.nodes.find((n) => n.id === id);
  const read = () => ok('GET', base2) as Promise<ResolvedMap>;
  const ops = (...list: unknown[]) => ok('POST', base2 + '/ops', { ops: list });

  it('builds the tree from what the project holds', async () => {
    W = (await ok('POST', '/api/workspaces', { name: 'Maps', kind: 'personal' })).id;
    P = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Site', parent: null })).id;
    base2 = `/api/ws/${W}/projects/${P}/map`;
    const rt = sup.rt(W);
    await ok('PATCH', `/api/ws/${W}`, { agents: [...rt.ws.agents, { id: 'quiet', name: 'Quiet', via: 'ACP', command: `${process.execPath} ${agent}`, on: true }] });
    chat = await ok('POST', `/api/ws/${W}/chats`, { project: P, title: 'Plan the launch', agent: 'quiet' });
    rt.state.artifacts.push({ id: 'art1', project: P, chat: chat.id, originSession: null, type: 'Doc', title: 'Launch brief', version: 2, versions: [], designSystem: '', createdAt: 1, updatedAt: 1 });
    rt.state.artifacts.push({ id: 'art2', project: P, chat: null, originSession: null, type: 'Deck', title: 'Loose deck', version: 1, versions: [], designSystem: '', createdAt: 2, updatedAt: 2 });
    const made = rt.createGraph({ name: 'Release', project: P, xml: '<x/>', chat: chat.id });
    const free = rt.createGraph({ name: 'Nightly', project: P, xml: '<x/>' });
    rt.saveStoredRun({ id: 'run1', graph: free.id, status: 'completed', dryRun: false, startedAt: 5, updatedAt: 5, log: [], completed: [], from: chat.id }, null);
    rt.saveStoredRun({ id: 'run2', graph: free.id, status: 'failed', dryRun: true, startedAt: 6, updatedAt: 6, log: [], completed: [] }, null);
    await ok('POST', `/api/ws/${W}/facts`, { text: 'Walls are 8 mm', scope: P });

    const m = await read();
    expect(m.root).toBe('project:' + P);
    expect(node(m, 'chat:' + chat.id)?.parent).toBe(m.root);
    expect(node(m, 'artifact:art1')?.parent).toBe('chat:' + chat.id);
    expect(node(m, 'artifact:art1')?.meta).toBe('doc v2');
    expect(node(m, 'artifact:art2')?.parent).toBe(m.root);
    expect(node(m, 'graph:' + made.id)?.parent).toBe('chat:' + chat.id);
    expect(node(m, 'graph:' + free.id)?.parent).toBe(m.root);
    expect(node(m, 'run:run1')?.parent).toBe('chat:' + chat.id);
    expect(node(m, 'run:run2')?.parent).toBe('graph:' + free.id);
    expect(node(m, 'group:memory')).toMatchObject({ parent: m.root, folded: true, meta: '1 fact' });
    expect(m.nodes.find((n) => n.kind === 'fact')?.parent).toBe('group:memory');
    // Nothing is saved until something is done on the map.
    expect(existsSync(join(rt.project(P).folder, '.harness', 'map.json'))).toBe(false);
  });

  it('keeps nodes, moves, links and hidden items, on disk', async () => {
    const { added } = await ops({ op: 'add', parent: 'project:' + P, kind: 'task', text: 'Ship it' }, { op: 'add', parent: 'project:' + P, text: 'An idea', side: 'left' });
    const [task, idea] = added;
    await ops({ op: 'move', id: 'artifact:art2', parent: task }, { op: 'link', from: idea, to: 'artifact:art1', label: 'see' }, { op: 'hide', id: 'graph:' + sup.rt(W).findGraph('Nightly').id }, { op: 'update', id: task, status: 'doing' });
    const m = await read();
    expect(node(m, task)).toMatchObject({ kind: 'task', status: 'doing', own: true, title: 'Ship it' });
    expect(node(m, idea)).toMatchObject({ kind: 'idea', side: 'left' });
    expect(node(m, 'artifact:art2')?.parent).toBe(task);
    expect(m.links).toEqual([expect.objectContaining({ from: idea, to: 'artifact:art1', label: 'see' })]);
    // A hidden graph takes the runs under it out of view.
    expect(node(m, 'graph:' + sup.rt(W).findGraph('Nightly').id)).toBeUndefined();
    expect(node(m, 'run:run2')).toBeUndefined();
    expect(existsSync(join(sup.rt(W).project(P).folder, '.harness', 'map.json'))).toBe(true);
    // Items can't be edited or removed as if they were own nodes, and the centre stays put.
    expect((await call('POST', base2 + '/ops', { ops: [{ op: 'update', id: 'chat:' + chat.id, text: 'x' }] })).status).toBe(409);
    expect((await call('POST', base2 + '/ops', { ops: [{ op: 'remove', id: 'chat:' + chat.id }] })).status).toBe(409);
    expect((await call('POST', base2 + '/ops', { ops: [{ op: 'move', id: 'project:' + P, parent: task }] })).status).toBe(400);
  });

  it('refuses a move that would put a node under itself', async () => {
    const { added: [a] } = await ops({ op: 'add', parent: 'project:' + P, text: 'Outer' });
    const { added: [b] } = await ops({ op: 'add', parent: a, text: 'Inner' });
    const before = await read();
    const r = await call('POST', base2 + '/ops', { ops: [{ op: 'move', id: a, parent: b }] });
    expect(r.status).toBe(400);
    expect(r.body.error).toContain('under itself');
    expect((await call('POST', base2 + '/ops', { ops: [{ op: 'move', id: 'chat:' + chat.id, parent: 'artifact:art1' }] })).status).toBe(400);
    expect(node(await read(), a)?.parent).toBe(before.root);
  });

  it('orders siblings and can place one before another', async () => {
    const { added: [parent] } = await ops({ op: 'add', parent: 'project:' + P, text: 'Steps' });
    const { added: [one, two, three] } = await ops({ op: 'add', parent, text: '1' }, { op: 'add', parent, text: '2' }, { op: 'add', parent, text: '3' });
    await ops({ op: 'move', id: three, parent, before: two });
    const m = await read();
    const list = m.nodes.filter((n) => n.parent === parent).sort((a, b) => a.order - b.order).map((n) => n.id);
    expect(list).toEqual([one, three, two]);
    // Many moves into the same gap stay correct.
    for (let i = 0; i < 60; i++) await ops({ op: 'move', id: i % 2 ? two : three, parent, before: i % 2 ? three : two });
    const again = (await read()).nodes.filter((n) => n.parent === parent).sort((a, b) => a.order - b.order).map((n) => n.id);
    expect(new Set(again).size).toBe(3);
  });

  it('drops what points at deleted items, and returns what was under them', async () => {
    const rt = sup.rt(W);
    const other = await ok('POST', `/api/ws/${W}/chats`, { project: P, title: 'Going away', agent: 'quiet' });
    const { added: [note] } = await ops({ op: 'add', parent: 'chat:' + other.id, kind: 'note', text: 'Under the chat' });
    await ops({ op: 'link', from: note, to: 'chat:' + other.id });
    expect(node(await read(), 'chat:' + other.id)).toBeTruthy();
    await ok('DELETE', `/api/ws/${W}/chats/${other.id}`);
    const m = await read();
    expect(node(m, 'chat:' + other.id)).toBeUndefined();
    expect(node(m, note)?.parent).toBe(m.root);
    expect(m.links.some((l) => l.to === 'chat:' + other.id)).toBe(false);
    expect(rt.chat(other.id)).toBeUndefined();
  });

  it('shows a workspace overview of its projects', async () => {
    const m = await ok('GET', `/api/ws/${W}/map`);
    expect(m.root).toBe('workspace:' + W);
    expect(node(m, 'project:' + P)).toMatchObject({ title: 'Site', parent: m.root });
    expect(node(m, 'project:' + P)?.meta).toContain('chat');
  });

  it('lets agents read and edit the map of their project', async () => {
    const before = await ok('GET', `/api/ws/${W}/agent/map?project=${P}`);
    expect(before.outline).toContain('Plan the launch (chat');
    const r = await ok('POST', `/api/ws/${W}/agent/map/ops`, { project: P, ops: [{ op: 'import', parent: 'chat:' + chat.id, markdown: '- [ ] Write copy\n  - [x] Draft\n- Think about pricing' }] });
    expect(r.added).toHaveLength(3);
    expect(r.outline).toContain('[ ] Write copy (task');
    expect(r.outline).toContain('[x] Draft (task');
    const m = await read();
    const copy = m.nodes.find((n) => n.title === 'Write copy')!;
    expect(copy.parent).toBe('chat:' + chat.id);
    expect(m.nodes.find((n) => n.title === 'Draft')?.parent).toBe(copy.id);
    expect(m.nodes.find((n) => n.title === 'Think about pricing')?.kind).toBe('idea');
    expect(sup.rt(W).state.chats.length).toBeGreaterThan(0);
  });
});

describe('map layout and formats', () => {
  const n = (id: string, parent: string | null, order = 0, extra: Partial<ResolvedMapNode> = {}): ResolvedMapNode => ({ id, kind: 'idea', title: id, parent, order, ...extra });
  const size = () => ({ w: 120, h: 36 });
  const sample: ResolvedMap = { scope: 'p', root: 'r', layout: 'right', links: [], nodes: [n('r', null), n('a', 'r', 1), n('b', 'r', 2), n('a1', 'a', 1), n('a2', 'a', 2), n('a3', 'a', 3), n('b1', 'b', 1)] };

  it('lays out with no overlaps and the set spacing', () => {
    const { boxes, edges } = layoutMap(sample, size);
    expect(boxes.size).toBe(7);
    expect(edges).toHaveLength(6);
    const list = [...boxes.values()];
    for (const a of list) for (const b of list) {
      if (a === b) continue;
      expect(a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h, `${a.id} overlaps ${b.id}`).toBe(false);
    }
    expect(boxes.get('a')!.x - (boxes.get('r')!.x + 120)).toBe(LEVEL_GAP);
    expect(boxes.get('a2')!.y - boxes.get('a1')!.y).toBe(36 + SIBLING_GAP);
    // A parent is centred on its children.
    expect(boxes.get('a')!.y).toBe(boxes.get('a2')!.y);
  });

  it('collapses branches, for the viewer or by default', () => {
    const open = layoutMap(sample, size);
    const closed = layoutMap(sample, size, { a: true });
    expect(closed.boxes.has('a1')).toBe(false);
    expect(closed.boxes.get('a')!.hiddenBelow).toBe(3);
    expect(closed.bounds.h).toBeLessThan(open.bounds.h);
    const folded: ResolvedMap = { ...sample, nodes: sample.nodes.map((x) => (x.id === 'a' ? { ...x, folded: true } : x)) };
    expect(layoutMap(folded, size).boxes.has('a1')).toBe(false);
    expect(layoutMap(folded, size, { a: false }).boxes.has('a1')).toBe(true);
  });

  it('keeps branches on their side when more are added', () => {
    const base: ResolvedMap = { scope: 'p', root: 'r', layout: 'balance', links: [], nodes: [n('r', null), ...Array.from({ length: 8 }, (_, i) => n('branch' + i, 'r', i))] };
    const before = layoutMap(base, size);
    const more: ResolvedMap = { ...base, nodes: [...base.nodes, n('first', 'r', -1), ...Array.from({ length: 5 }, (_, i) => n('late' + i, 'r', 100 + i))] };
    const after = layoutMap(more, size);
    for (let i = 0; i < 8; i++) expect(after.boxes.get('branch' + i)!.side).toBe(before.boxes.get('branch' + i)!.side);
    // A saved side wins, and left-hand boxes end where the level gap starts.
    expect(sideOf(n('x', 'r', 0, { side: 'left' }))).toBe('left');
    const pinned = layoutMap({ ...base, nodes: [n('r', null), n('x', 'r', 0, { side: 'left' }), n('y', 'r', 1, { side: 'right' })] }, size);
    expect(pinned.boxes.get('x')!.x + 120).toBe(pinned.boxes.get('r')!.x - LEVEL_GAP);
    expect(pinned.boxes.get('y')!.x).toBe(pinned.boxes.get('r')!.x + 120 + LEVEL_GAP);
  });

  it('round-trips Markdown', () => {
    const map: ResolvedMap = { scope: 'p', root: 'r', layout: 'right', links: [], nodes: [n('r', null, 0, { title: 'Site' }), n('t', 'r', 1, { kind: 'task', status: 'done', title: 'Ship' }), n('c', 't', 1, { title: 'Child' }), n('u', 'r', 2, { kind: 'task', title: 'Later' })] };
    const md = toMarkdown(map);
    expect(md).toBe('# Site\n\n- [x] Ship\n  - Child\n- [ ] Later\n');
    expect(fromMarkdown(md)).toEqual([{ text: 'Ship', status: 'done', children: [{ text: 'Child', children: [] }] }, { text: 'Later', status: 'todo', children: [] }]);
    expect(toOutline(map)).toContain('[x] Ship (task) [t]');
  });

  it('reads a graph’s picture from its file', () => {
    const g = graphPreview(blankGraph('Picture'))!;
    expect(g.shapes.map((x) => x.kind)).toEqual(['start', 'task', 'end']);
    expect(g.shapes[1]).toMatchObject({ id: 'Task_1', name: 'Describe this step', w: 100, h: 80 });
    expect(g.edges).toHaveLength(2);
    expect(g.edges[0].id).toBe('Flow_1');
    expect(g.bounds.w).toBeGreaterThan(200);
    expect(graphPreview('<bpmn:definitions><bpmn:process id="P" /></bpmn:definitions>')).toBeNull();
  });
});
