// R-02: not tied to one vendor — one workspace answers real chats through opencode with Anthropic (its native
// Messages API, here a local stand-in reached through the provider's baseURL) and Ollama (its OpenAI-compatible /v1).
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { createServer, type Server } from 'node:http';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-vendors-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'none');
const ANTHROPIC_KEY = 'sk-ant-test-0123456789';

let anth: Server, ollama: Server, anthUrl = '', ollamaUrl = '';
const hits = { anthropic: [] as { key?: string; path?: string; model?: string }[], ollama: [] as { path?: string; model?: string }[] };
let base = '', token = '', sup: any, close = async () => {};
const ok = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${t}`); return t ? JSON.parse(t) : {};
};
const listen = (s: Server) => new Promise<string>((r) => s.listen(0, '127.0.0.1', () => r(`http://127.0.0.1:${(s.address() as any).port}`)));
const sse = (res: any, events: [string, object][]) => { res.writeHead(200, { 'content-type': 'text/event-stream' }); for (const [e, d] of events) res.write(`event: ${e}\ndata: ${JSON.stringify({ type: e, ...d })}\n\n`); res.end(); };

beforeAll(async () => {
  // Anthropic Messages API: x-api-key auth, streamed message/content-block events.
  anth = createServer(async (req, res) => {
    let body = ''; for await (const c of req) body += c;
    const j = JSON.parse(body || '{}');
    hits.anthropic.push({ key: String(req.headers['x-api-key'] || ''), path: req.url, model: j.model });
    if (req.headers['x-api-key'] !== ANTHROPIC_KEY) { res.writeHead(401, { 'content-type': 'application/json' }).end('{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}'); return; }
    sse(res, [
      ['message_start', { message: { id: 'msg_1', type: 'message', role: 'assistant', model: j.model, content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 7, output_tokens: 1 } } }],
      ['content_block_start', { index: 0, content_block: { type: 'text', text: '' } }],
      ['content_block_delta', { index: 0, delta: { type: 'text_delta', text: 'Hello from Anthropic.' } }],
      ['content_block_stop', { index: 0 }],
      ['message_delta', { delta: { stop_reason: 'end_turn', stop_sequence: null }, usage: { output_tokens: 5 } }],
      ['message_stop', {}],
    ]);
  });
  // Ollama's OpenAI-compatible endpoint: no key.
  ollama = createServer(async (req, res) => {
    let body = ''; for await (const c of req) body += c;
    const j = JSON.parse(body || '{}');
    hits.ollama.push({ path: req.url, model: j.model });
    const chunk = (delta: object, finish: string | null) => `data: ${JSON.stringify({ id: 'o1', object: 'chat.completion.chunk', created: 1, model: j.model, choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write(chunk({ role: 'assistant', content: 'Hello from Ollama.' }, null));
    res.write(chunk({}, 'stop'));
    res.end('data: [DONE]\n\n');
  });
  anthUrl = (await listen(anth)) + '/v1'; ollamaUrl = (await listen(ollama)) + '/v1';
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  sup = new Supervisor();
  const server = await startServer(sup, buildRouter(), {});
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); anth.close(); ollama.close(); };
});
afterAll(async () => { await close(); });

describe('vendor-neutral (R-02)', () => {
  it('one workspace chats through Anthropic and through Ollama', async () => {
    const { PROVIDER_PRESETS } = await import('../../shared/src/defaults.ts') as any;
    const { keychain } = await import('../src/keychain.ts');
    const W = (await ok('POST', '/api/workspaces', { name: 'Vendors', kind: 'personal' })).id;
    const rt = sup.rt(W);
    const presets = PROVIDER_PRESETS || {};
    const anthropic = { ...(presets.anthropic || { id: 'anthropic', name: 'Anthropic', note: '', preset: 'anthropic', keyEnv: 'ANTHROPIC_API_KEY', models: [{ id: 'claude-haiku-4-5' }] }), baseURL: anthUrl };
    const local = { ...(presets.ollama || { id: 'ollama', name: 'Ollama', note: '', preset: 'ollama', npm: '@ai-sdk/openai-compatible', models: [{ id: 'qwen3-coder' }] }), baseURL: ollamaUrl };
    rt.ws.providers = [anthropic, local];
    keychain.set(W, 'ANTHROPIC_API_KEY', ANTHROPIC_KEY);
    rt.writeConfig();
    const json = JSON.parse(readFileSync(rt.paths.opencodeJson, 'utf8'));
    expect(json.provider.anthropic).toEqual({ options: { baseURL: anthUrl, apiKey: '{env:ANTHROPIC_API_KEY}' } });
    expect(json.provider.ollama.npm).toBe('@ai-sdk/openai-compatible');

    for (const [model, want] of [[`anthropic/${anthropic.models[0].id}`, 'Hello from Anthropic.'], [`ollama/${local.models[0].id}`, 'Hello from Ollama.']]) {
      const chat = await ok('POST', `/api/ws/${W}/chats`, { title: model, model, text: 'Say hello.' });
      await (await rt.client()).waitIdle(chat.sid, 60000).catch(() => {});
      const reply = (await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`)).messages.find((m: any) => m.role === 'assistant');
      expect(reply?.text, JSON.stringify(reply)).toContain(want);
      expect(reply?.model).toBe(model);
    }
    expect(hits.anthropic.some((h) => h.key === ANTHROPIC_KEY && h.path?.startsWith('/v1/messages') && h.model === anthropic.models[0].id)).toBe(true);
    expect(hits.ollama.some((h) => h.path === '/v1/chat/completions' && h.model === local.models[0].id)).toBe(true);
  });
});
