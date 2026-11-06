// R-23: Graphify runs on the mapped folders with only this workspace's allowed model, its graph.json is read back,
// ambiguous links go to the review queue, and watch mode re-runs `graphify update` on change.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, writeFileSync, readFileSync, existsSync, chmodSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-gfy-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'none');
process.env.LEAKY_SECRET = 'should-not-pass';

// Stub Graphify: records its args and env, writes a small graph with one ambiguous link.
const bin = join(home, 'graphify');
const calls = join(home, 'calls.jsonl');
writeFileSync(bin, `#!/usr/bin/env node
const fs = require('fs'), path = require('path');
const [mode, folder] = process.argv.slice(2);
fs.appendFileSync(${JSON.stringify(calls)}, JSON.stringify({ mode, folder, cwd: process.cwd(), env: process.env }) + '\\n');
fs.mkdirSync(path.join(folder, 'graphify-out'), { recursive: true });
fs.writeFileSync(path.join(folder, 'graphify-out', 'graph.json'), JSON.stringify({
  nodes: [{ id: 'a', label: 'a.ts', type: 'file', community: 'core', source_file: 'a.ts' }, { id: 'b', label: 'b.ts', type: 'file', community: 'core', source_file: '../outside.txt' }, { id: 'c', label: 'Retry policy', type: 'concept', community: 'docs' }],
  links: [{ source: 'a', target: 'b', relation: 'imports' }, { source: 'b', target: 'c', relation: 'mentions', ambiguous: true, reason: 'Inferred from a comment' }],
}));
`);
chmodSync(bin, 0o755);
process.env.HARNESS_GRAPHIFY_BIN = bin;

let base = '', token = '', sup: any, scheduler: any, close = async () => {};
const ok = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${t}`); return t ? JSON.parse(t) : {};
};
const lastCall = () => { const l = readFileSync(calls, 'utf8').trim().split('\n'); return { n: l.length, ...JSON.parse(l[l.length - 1]) }; };
const until = async (fn: () => boolean, ms: number) => { const end = Date.now() + ms; while (Date.now() < end) { if (fn()) return true; await new Promise((r) => setTimeout(r, 200)); } return false; };

beforeAll(async () => {
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  const { installExtensions } = await import('../src/extensions.ts');
  const { Scheduler } = await import('../src/scheduler.ts');
  sup = new Supervisor();
  const router = buildRouter();
  const server = await startServer(sup, router, {});
  installExtensions(sup, router, server);
  scheduler = new Scheduler(sup);
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { scheduler.stop(); await sup.shutdown(); await server.close(); };
});
afterAll(async () => { await close(); });

describe('Graphify (R-23)', () => {
  let W = '';
  const folder = join(home, 'notes');
  it('runs extract with a code-only backend when no allowed provider can do the docs pass', async () => {
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, 'a.ts'), 'export {}\n');
    W = (await ok('POST', '/api/workspaces', { name: 'G', kind: 'personal' })).id;
    await ok('PATCH', `/api/ws/${W}`, { graphifyFolders: [folder], graphifyWatch: false });
    expect((await ok('GET', `/api/ws/${W}/graphify/status`)).installed).toBe(true);
    const g = await ok('POST', `/api/ws/${W}/graphify/build`);
    const c = lastCall();
    expect(c).toMatchObject({ mode: 'extract', folder, cwd: folder });
    expect(c.env.GRAPHIFY_BACKEND).toBe('none');
    expect(c.env.LEAKY_SECRET).toBeUndefined();
    expect(g.source).toBe('graphify');
    expect(g.nodes.map((n: any) => n.label).sort()).toEqual(['Retry policy', 'a.ts', 'b.ts']);
    expect(g.ambiguous).toHaveLength(1);
    expect((await ok('GET', `/api/ws/${W}/graphify/status`)).pendingLinks).toBe(1);
  });

  it('uses only this workspace’s own provider key for the docs pass', async () => {
    await ok('POST', `/api/ws/${W}/providers`, { id: 'gateway', url: 'http://127.0.0.1:9/v1', key: 'sk-workspace-only', model: 'm1' });
    const other = (await ok('POST', '/api/workspaces', { name: 'Other', kind: 'personal' })).id;
    await ok('POST', `/api/ws/${other}/providers`, { id: 'elsewhere', url: 'http://127.0.0.1:8/v1', key: 'sk-other', model: 'm2' });
    await ok('POST', `/api/ws/${W}/graphify/build`);
    const c = lastCall();
    expect(c.env).toMatchObject({ GRAPHIFY_BACKEND: 'openai', OPENAI_API_KEY: 'sk-workspace-only', OPENAI_BASE_URL: 'http://127.0.0.1:9/v1' });
    expect(JSON.stringify(c.env)).not.toContain('sk-other');
  });

  it('watch mode runs graphify update when a mapped file changes', async () => {
    await ok('PATCH', `/api/ws/${W}`, { graphifyWatch: true });
    const before = lastCall().n;
    await scheduler.tick();
    writeFileSync(join(folder, 'c.md'), '# Retry policy\n');
    expect(await until(() => lastCall().n > before, 10000)).toBe(true);
    expect(lastCall().mode).toBe('update');
    expect(existsSync(join(folder, 'graphify-out', 'graph.json'))).toBe(true);
  });

  it('a chat on the map carries the Graphify nodes it is about, folded under it', async () => {
    const rt = sup.rt(W);
    const P = rt.state.projects.find((p: any) => p.isGeneral).id;
    const chat = (id: string, title: string) => ({ id, project: P, title, sid: 'ses_' + id, agent: 'opencode', model: '', web: null, instructions: '', imported: false, archived: false, createdAt: 1, updatedAt: 1 });
    rt.state.chats.push(chat('c_retry', 'Tune the retry policy'), chat('c_other', 'Weekly standup'));
    const nodes = async () => (await ok('GET', `/api/ws/${W}/projects/${P}/map`)).nodes as any[];

    await ok('PATCH', `/api/ws/${W}`, { memory: 'facts' });
    expect((await nodes()).filter((n) => n.kind === 'memory')).toEqual([]);

    await ok('PATCH', `/api/ws/${W}`, { memory: 'graph' });
    const on = await nodes();
    const group = on.find((n) => n.id === 'group:chat-memory:c_retry');
    expect(group).toMatchObject({ parent: 'chat:c_retry', folded: true });
    expect(on.filter((n) => n.parent === group.id).map((n) => [n.kind, n.title])).toEqual([['memory', 'Retry policy']]);
    expect(on.some((n) => n.id === 'group:chat-memory:c_other')).toBe(false);
    // Its node opens onto what it connects to.
    const node = on.find((n) => n.parent === group.id);
    expect(on.filter((n) => n.parent === node.id).map((n) => [n.ref, n.meta])).toEqual([['b', 'mentions · file']]);
  });

  it('previews a node’s file on the map, only from inside the mapped folders', async () => {
    const detail = (id: string) => ok('GET', `/api/ws/${W}/graphify/node?id=${encodeURIComponent(id)}`);
    await ok('GET', `/api/ws/${W}/graphify/graph?rebuild=1`);
    const a = await detail('a');
    expect(a.file.text).toContain('export');
    expect(a.links.map((l: any) => l.id)).toEqual(['b']);
    writeFileSync(join(home, 'outside.txt'), 'private');
    const b = await detail('b');
    expect(b.file.text).toBeUndefined();
    expect(b.file.note).toContain('outside');
    await expect(detail('nope')).rejects.toThrow('404');
  });


  it('a rebuild tells open maps, and a file that can’t be read still shows its node', async () => {
    const rt = sup.rt(W);
    const emit = rt.hooks.emit;
    const seen: any[] = [];
    rt.hooks.emit = (e: any) => { seen.push(e); emit(e); };
    try { await ok('GET', `/api/ws/${W}/graphify/graph?rebuild=1`); } finally { rt.hooks.emit = emit; }
    expect(seen.some((e) => e.type === 'map' && e.workspace === W)).toBe(true);
    chmodSync(join(folder, 'a.ts'), 0o000);
    try {
      const a = await ok('GET', `/api/ws/${W}/graphify/node?id=a`);
      expect(a.node.label).toBe('a.ts');
      if (process.getuid?.() !== 0) expect(a.file.note).toContain('can’t be read');
    } finally { chmodSync(join(folder, 'a.ts'), 0o644); }
  });

  it('offers Pi as an ACP agent that starts switched off', () => {
    expect(sup.rt(W).ws.agents.find((a: any) => a.id === 'pi')).toMatchObject({ on: false, command: 'npx -y pi-acp@0.0.34' });
  });
});
