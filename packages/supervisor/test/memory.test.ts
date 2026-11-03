// R-22: memory per workspace and per project; the user approves what is remembered and can add or forget facts.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

const home = mkdtempSync(join(tmpdir(), 'harness-mem-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'none');

let base = '', token = '', port = 0, sup: any, close = async () => {};
const ok = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${t}`); return t ? JSON.parse(t) : {};
};
const text = (r: any) => (r.content || []).map((c: any) => c.text).join('\n');

beforeAll(async () => {
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  const { installExtensions } = await import('../src/extensions.ts');
  sup = new Supervisor();
  const router = buildRouter();
  const server = await startServer(sup, router, {});
  installExtensions(sup, router, server);
  port = server.port; base = `http://127.0.0.1:${port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); };
});
afterAll(async () => { await close(); });

describe('memory (R-22)', () => {
  let W = '', B = '', P = '', S = '';
  it('adds, approves and forgets facts in the right scope; nothing crosses workspaces', async () => {
    W = (await ok('POST', '/api/workspaces', { name: 'Personal', kind: 'personal' })).id;
    B = (await ok('POST', '/api/workspaces', { name: 'Other', kind: 'team' })).id;
    P = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Castform', parent: null })).id;
    S = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Sim', parent: P })).id;
    const ws = await ok('POST', `/api/ws/${W}/facts`, { text: 'Prefers short answers.', scope: 'ws' });
    const proj = await ok('POST', `/api/ws/${W}/facts`, { text: 'Default mold wall is 8 mm.', scope: S, source: 'chat: Mold math' });
    const sug = await ok('POST', `/api/ws/${W}/facts`, { text: 'Uses Mold Star 30.', scope: P, status: 'suggested' });
    let facts = (await ok('GET', `/api/ws/${W}/snapshot`)).facts;
    expect(facts.find((f: any) => f.id === ws.id).scope).toBe('ws');
    expect(facts.find((f: any) => f.id === proj.id).scope).toBe(S);
    expect(facts.find((f: any) => f.id === sug.id).status).toBe('suggested');
    // Stored as Markdown in the project folder.
    const { readFileSync } = await import('node:fs');
    expect(readFileSync(join(sup.rt(W).project(S).folder, '.harness/memory/facts.md'), 'utf8')).toContain('Default mold wall is 8 mm.');
    await ok('PATCH', `/api/ws/${W}/facts/${sug.id}`, { status: 'approved' });
    await ok('DELETE', `/api/ws/${W}/facts/${ws.id}`);
    facts = (await ok('GET', `/api/ws/${W}/snapshot`)).facts;
    expect(facts.find((f: any) => f.id === sug.id).status).toBe('approved');
    expect(facts.some((f: any) => f.id === ws.id)).toBe(false);
    expect((await ok('GET', `/api/ws/${B}/snapshot`)).facts).toEqual([]);
  });

  it('the agent’s memory MCP server reads approved facts and writes only suggestions', async () => {
    const rt = sup.rt(W);
    const client = new Client({ name: 'test', version: '0' });
    await client.connect(new StdioClientTransport({ command: process.execPath, args: [join(__dirname, '../../memory-mcp/src/server.ts')], env: { ...process.env, HARNESS_MEMORY_DIR: rt.paths.memory, HARNESS_WS: W, HARNESS_SUPERVISOR: base, HARNESS_SUPERVISOR_TOKEN: token, HARNESS_MEMORY_BACKEND: 'facts' } }));
    try {
      const tools = (await client.listTools()).tools.map((t) => t.name);
      expect(tools).toEqual(expect.arrayContaining(['memory_search', 'memory_read', 'memory_write']));
      // A sub-project's read widens to its parents and the workspace.
      const read = text(await client.callTool({ name: 'memory_read', arguments: { project: 'Sim' } }));
      expect(read).toContain('Default mold wall is 8 mm.');
      expect(read).toContain('Uses Mold Star 30.');
      const w = text(await client.callTool({ name: 'memory_write', arguments: { text: 'Pours at 22 °C.', project: 'Sim' } }));
      expect(w).toMatch(/approves/);
      // Not usable until approved.
      expect(text(await client.callTool({ name: 'memory_search', arguments: { query: 'pours', project: 'Sim' } }))).toBe('No matching facts.');
      const facts = (await ok('GET', `/api/ws/${W}/snapshot`)).facts;
      const f = facts.find((x: any) => x.text === 'Pours at 22 °C.');
      expect(f).toMatchObject({ status: 'suggested', scope: S });
      await ok('PATCH', `/api/ws/${W}/facts/${f.id}`, { status: 'approved' });
      expect(text(await client.callTool({ name: 'memory_search', arguments: { query: 'pours', project: 'Sim' } }))).toContain('Pours at 22 °C.');
    } finally { await client.close(); }
  });
});
