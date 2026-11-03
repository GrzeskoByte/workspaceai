// R-21 live: a scripted model (OpenAI-compatible stand-in) asks opencode to read a page with webfetch; opencode fetches
// it, the answer uses it, and the chat lists the page as a source. With "ask before searching" on, the fetch waits
// for the user's OK in the chat.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { createServer, type Server } from 'node:http';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-web-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'none');

let site: Server, model: Server, pageUrl = '', modelUrl = '';
const toolResults: string[] = [];
let base = '', token = '', sup: any, close = async () => {};
const ok = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${t}`); return t ? JSON.parse(t) : {};
};
const until = async (fn: () => Promise<boolean> | boolean, ms: number) => { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return true; await new Promise((r) => setTimeout(r, 200)); } return false; };

beforeAll(async () => {
  site = createServer((_req, res) => res.writeHead(200, { 'content-type': 'text/html' }).end('<html><head><title>Castform beta</title></head><body><h1>Castform beta</h1><p>Minimum mold wall: 8 mm.</p></body></html>'));
  pageUrl = 'http://127.0.0.1:' + await new Promise<number>((r) => site.listen(0, '127.0.0.1', () => r((site.address() as any).port))) + '/beta';
  // Turn 1: call webfetch. Turn 2 (a tool result is in the history): answer from it.
  model = createServer(async (req, res) => {
    let b = ''; for await (const c of req) b += c;
    const j = JSON.parse(b || '{}');
    const tool = (j.messages || []).filter((m: any) => m.role === 'tool').pop();
    const chunk = (delta: object, finish: string | null) => `data: ${JSON.stringify({ id: 'c', object: 'chat.completion.chunk', created: 1, model: j.model, choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    if (!tool) {
      res.write(chunk({ role: 'assistant', tool_calls: [{ index: 0, id: 'call_1', type: 'function', function: { name: 'webfetch', arguments: JSON.stringify({ url: pageUrl, format: 'text' }) } }] }, null));
      res.write(chunk({}, 'tool_calls'));
    } else {
      const text = typeof tool.content === 'string' ? tool.content : JSON.stringify(tool.content);
      toolResults.push(text);
      res.write(chunk({ role: 'assistant', content: /8 mm/.test(text) ? 'The page says the minimum wall is 8 mm.' : 'I could not read the page.' }, null));
      res.write(chunk({}, 'stop'));
    }
    res.end('data: [DONE]\n\n');
  });
  modelUrl = 'http://127.0.0.1:' + await new Promise<number>((r) => model.listen(0, '127.0.0.1', () => r((model.address() as any).port))) + '/v1';
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  sup = new Supervisor();
  const server = await startServer(sup, buildRouter(), {});
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); site.close(); model.close(); };
});
afterAll(async () => { await close(); });

describe('reading the web in a chat (R-21)', () => {
  let W = '';
  it('fetches the page through opencode and lists it as a source', async () => {
    W = (await ok('POST', '/api/workspaces', { name: 'Web', kind: 'personal' })).id;
    const rt = sup.rt(W);
    rt.ws.providers = [{ id: 'scripted', name: 'Scripted', note: '', npm: '@ai-sdk/openai-compatible', baseURL: modelUrl, models: [{ id: 'm' }] }];
    await ok('PATCH', `/api/ws/${W}`, { web: { ...rt.ws.web, mode: 'off', fetch: true, ask: false, showSources: true } });
    rt.writeConfig(); await rt.restartEngine();
    const chat = await ok('POST', `/api/ws/${W}/chats`, { title: 'Beta', model: 'scripted/m', text: 'What is the minimum wall on the beta page?' });
    await (await rt.client()).waitIdle(chat.sid, 60000).catch(() => {});
    expect(await until(async () => !!(await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`)).messages.find((m: any) => m.role === 'assistant' && /8 mm/.test(m.text)), 30000)).toBe(true);
    const reply = (await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`)).messages.filter((m: any) => m.role === 'assistant').pop();
    expect(reply.text).toContain('8 mm');
    expect(toolResults.some((t) => /Minimum mold wall: 8 mm/.test(t))).toBe(true);
    expect((reply.sources || []).map((s: any) => s.url)).toContain(pageUrl);
  }, 120000);

  it('with ask-before-searching on, the fetch waits for the user in the chat', async () => {
    const rt = sup.rt(W);
    await ok('PATCH', `/api/ws/${W}`, { web: { ...rt.ws.web, ask: true } });
    const events: any[] = [];
    const off = sup.on((e: any) => events.push(e));
    const before = toolResults.length;
    const chat = await ok('POST', `/api/ws/${W}/chats`, { title: 'Ask', model: 'scripted/m', text: 'Read the beta page.' });
    expect(await until(() => events.some((e) => e.type === 'chat.permission' && e.chat === chat.id), 30000)).toBe(true);
    expect(toolResults.length).toBe(before);
    const req = events.find((e) => e.type === 'chat.permission' && e.chat === chat.id).request;
    await ok('POST', `/api/ws/${W}/chats/${chat.id}/permission/${req.id}`, { decision: 'once' });
    expect(await until(() => toolResults.length > before, 30000)).toBe(true);
    off();
  }, 120000);
});
