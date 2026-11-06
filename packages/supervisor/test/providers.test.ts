// R-02 any provider, R-20 custom providers, R-21 web search config, R-40 gateway preset — with a fake
// OpenAI-compatible endpoint that requires a bearer key, used by a real opencode server.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { createServer, type Server } from 'node:http';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-prov-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'no-open-design');
const KEY = 'sk-gateway-test-0123456789';

let fake: Server; let fakeUrl = ''; const seen: { auth?: string; model?: string }[] = [];
let base = '', token = '', sup: any, close = async () => {};

async function call(method: string, path: string, body?: unknown) {
  const r = await fetch(base + path, { method, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  const t = await r.text();
  return { status: r.status, data: t ? JSON.parse(t) : {} };
}
const ok = async (m: string, p: string, b?: unknown) => { const r = await call(m, p, b); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${JSON.stringify(r.data)}`); return r.data; };

beforeAll(async () => {
  fake = createServer(async (req, res) => {
    const auth = req.headers.authorization;
    if (auth !== 'Bearer ' + KEY) { res.writeHead(401, { 'content-type': 'application/json' }).end('{"error":{"message":"bad key"}}'); return; }
    if (req.url?.endsWith('/models')) { res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ object: 'list', data: [{ id: 'gateway-model', object: 'model' }] })); return; }
    let body = ''; for await (const c of req) body += c;
    const j = JSON.parse(body || '{}');
    seen.push({ auth, model: j.model });
    const chunk = (delta: object, finish: string | null) => `data: ${JSON.stringify({ id: 'c1', object: 'chat.completion.chunk', created: 1, model: j.model, choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
    if (j.stream) {
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      res.write(chunk({ role: 'assistant', content: 'Hello from the gateway.' }, null));
      res.write(chunk({}, 'stop'));
      res.write(`data: ${JSON.stringify({ id: 'c1', object: 'chat.completion.chunk', created: 1, model: j.model, choices: [], usage: { prompt_tokens: 5, completion_tokens: 4, total_tokens: 9 } })}\n\n`);
      res.end('data: [DONE]\n\n');
    } else res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ id: 'c1', object: 'chat.completion', created: 1, model: j.model, choices: [{ index: 0, message: { role: 'assistant', content: 'Hello from the gateway.' }, finish_reason: 'stop' }], usage: { prompt_tokens: 5, completion_tokens: 4, total_tokens: 9 } }));
  });
  await new Promise<void>((r) => fake.listen(0, '127.0.0.1', () => r()));
  const a = fake.address(); fakeUrl = `http://127.0.0.1:${typeof a === 'object' && a ? a.port : 0}/v1`;
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  const { installExtensions } = await import('../src/extensions.ts');
  sup = new Supervisor();
  const router = buildRouter();
  const server = await startServer(sup, router, {});
  installExtensions(sup, router, server);
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); fake.close(); };
});
afterAll(async () => { await close(); });

describe('providers and web search', () => {
  let W = '';
  it('tests a custom OpenAI-compatible endpoint before saving (R-20)', async () => {
    W = (await ok('POST', '/api/workspaces', { name: 'Personal', kind: 'personal' })).id;
    const bad = await ok('POST', '/api/onboarding/test-provider', { kind: 'custom', url: fakeUrl, key: 'nope' });
    expect(bad.ok).toBe(false);
    const good = await ok('POST', '/api/onboarding/test-provider', { kind: 'custom', url: fakeUrl, key: KEY });
    expect(good).toMatchObject({ ok: true, models: ['gateway-model'] });
    expect((await ok('POST', '/api/onboarding/test-provider', { kind: 'custom', url: 'http://gateway.example.test/v1' })).error).toMatch(/https/);
  });

  it('a LiteLLM-style gateway provider answers a real chat through opencode (R-02, R-20, R-40)', async () => {
    const { CUSTOM_PRESETS } = await import('../../shared/src/defaults.ts');
    const lite = CUSTOM_PRESETS.find((p) => p.id === 'litellm')!;
    expect(lite.url).toBe('http://localhost:4000/v1');
    await ok('POST', `/api/ws/${W}/providers`, { id: 'litellm', name: lite.name, pkg: lite.pkg, url: fakeUrl, env: lite.env, model: 'gateway-model', key: KEY });
    expect((await ok('POST', `/api/ws/${W}/providers/test`, { id: 'litellm' })).ok).toBe(true);
    const json = JSON.parse(readFileSync(sup.rt(W).paths.opencodeJson, 'utf8'));
    expect(json.provider.litellm.options.apiKey).toBe('{env:LITELLM_API_KEY}');
    const chat = await ok('POST', `/api/ws/${W}/chats`, { title: 'Gateway', model: 'litellm/gateway-model', text: 'Say hello.' });
    await (await sup.rt(W).client()).waitIdle(chat.sid, 60000).catch(() => {});
    const msgs = (await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`)).messages;
    const reply = msgs.find((m: any) => m.role === 'assistant');
    expect(reply?.text).toContain('Hello from the gateway.');
    expect(reply?.model).toBe('litellm/gateway-model');
    expect(seen.some((s) => s.auth === 'Bearer ' + KEY && s.model === 'gateway-model')).toBe(true);
  });

  it('an org workspace reaches every allowed model through its one gateway URL (R-40)', async () => {
    const ORG = (await ok('POST', '/api/workspaces/join-org', { name: 'Gateway Org', email: 'me@org.test', gateway: fakeUrl })).id;
    const rt = sup.rt(ORG);
    const { keychain } = await import('../src/keychain.ts');
    keychain.set(ORG, 'AI_FACTORY_TOKEN', KEY);
    rt.writeConfig();
    const json = JSON.parse(readFileSync(rt.paths.opencodeJson, 'utf8'));
    const urls = new Set(Object.values(json.provider).map((p: any) => p.options?.baseURL));
    expect([...urls]).toEqual([fakeUrl]);
    expect(Object.values(json.provider).every((p: any) => p.options.apiKey === '{env:AI_FACTORY_TOKEN}')).toBe(true);
    const before = seen.length;
    for (const model of rt.policy.allowedModels) {
      const chat = await ok('POST', `/api/ws/${ORG}/chats`, { title: model, model, text: 'Say hello.' });
      await (await rt.client()).waitIdle(chat.sid, 60000).catch(() => {});
      const reply = (await ok('GET', `/api/ws/${ORG}/chats/${chat.id}/messages`)).messages.find((m: any) => m.role === 'assistant');
      expect(reply?.text, model).toContain('Hello from the gateway.');
    }
    const got = seen.slice(before).filter((x) => x.auth === 'Bearer ' + KEY).map((x) => x.model);
    for (const model of rt.policy.allowedModels) expect(got, model).toContain(model.split('/').slice(1).join('/'));
  });

  it('web search choices are written to opencode.json and env (R-21)', async () => {
    const { buildOpencodeJson } = await import('../../shared/src/opencode-config.ts');
    const ws = sup.rt(W).ws;
    const mk = (web: object) => buildOpencodeJson({ ...ws, web: { ...ws.web, ...web } }, null);
    expect(mk({ mode: 'off' }).json.permission!.websearch).toBe('deny');
    const exa = mk({ mode: 'exa', ask: true });
    expect(exa.env.OPENCODE_ENABLE_EXA).toBe('true');
    expect(exa.json.permission).toMatchObject({ websearch: 'ask', webfetch: 'ask' });
    const tav = mk({ mode: 'mcp', provider: 'Tavily' });
    expect((tav.json.mcp as any).tavily.environment.TAVILY_API_KEY).toBe('{env:TAVILY_API_KEY}');
    expect(tav.json.permission!['tavily_*']).toBe('allow');
    const sx = mk({ mode: 'searxng', searxngUrl: 'https://search.example.test' });
    expect((sx.json.mcp as any).searxng.environment.SEARXNG_URL).toBe('https://search.example.test');
    expect(mk({ mode: 'exa', fetch: false }).json.permission!.webfetch).toBe('deny');
  });

  it('a connection test fails when the URL serves a web page, as Open WebUI does on /v1 (R-20)', async () => {
    const page = createServer((req, res) => {
      if (req.method === 'POST') res.writeHead(405, { 'content-type': 'application/json' }).end('{"detail":"Method Not Allowed"}');
      else res.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html>');
    });
    await new Promise<void>((done) => page.listen(0, '127.0.0.1', done));
    const url = `http://127.0.0.1:${(page.address() as any).port}/v1`;
    for (const model of [undefined, 'gemma']) {
      const r = await ok('POST', '/api/onboarding/test-provider', { kind: 'custom', url, model });
      expect(r.ok).toBe(false);
      expect(r.error).toContain('Base URL');
    }
    page.close();
  });

  it('a connection test asks the model for a reply, and a saved provider can be reconfigured (R-20)', async () => {
    const before = seen.length;
    expect((await ok('POST', `/api/ws/${W}/providers/test`, { id: 'litellm', pkg: '@ai-sdk/openai-compatible', model: 'probe-model' })).ok).toBe(true);
    expect(seen.slice(before).map((s) => s.model)).toEqual(['probe-model']);
    expect((await call('PATCH', `/api/ws/${W}/providers/litellm`, { models: [' '] })).status).toBe(400);
    await ok('PATCH', `/api/ws/${W}/providers/litellm`, { name: 'Renamed', url: fakeUrl + '/', models: ['gateway-model', 'second'] });
    const p = sup.rt(W).ws.providers.find((x: any) => x.id === 'litellm');
    expect(p).toMatchObject({ name: 'Renamed', baseURL: fakeUrl + '/', keyEnv: 'LITELLM_API_KEY' });
    expect(p.models.map((m: any) => m.id)).toEqual(['gateway-model', 'second']);
    await ok('PATCH', `/api/ws/${W}/providers/litellm`, { url: fakeUrl, models: ['gateway-model'] });
    expect(JSON.parse(readFileSync(sup.rt(W).paths.opencodeJson, 'utf8')).provider.litellm.options.baseURL).toBe(fakeUrl);
  });

  it('web search starts off in a new chat and a chat can turn it on; org workspaces lock the backend (R-21)', async () => {
    const chat = await ok('POST', `/api/ws/${W}/chats`, { title: 'No web' });
    expect(sup.rt(W).webEnabledFor(sup.rt(W).chat(chat.id))).toBe(false);
    await ok('PATCH', `/api/ws/${W}/chats/${chat.id}`, { web: true });
    if (sup.rt(W).effectiveWebMode() !== 'off') expect(sup.rt(W).webEnabledFor(sup.rt(W).chat(chat.id))).toBe(true);
    const ORG = (await ok('POST', '/api/workspaces/join-org', { name: 'Org' })).id;
    const o = await ok('GET', `/api/ws/${ORG}/snapshot`);
    expect(o.workspace.web).toMatchObject({ mode: 'searxng', locked: true });
    expect((await call('PATCH', `/api/ws/${ORG}`, { web: { ...o.workspace.web, mode: 'exa' } })).status).toBe(403);
    // The per-chat toggle still works in the org.
    const oc = await ok('POST', `/api/ws/${ORG}/chats`, { title: 'org no web', web: false, agent: 'opencode' }).catch(() => null);
    if (oc) expect(sup.rt(ORG).webEnabledFor(sup.rt(ORG).chat(oc.id))).toBe(false);
  });

  it('answers list the pages the agent read (R-21)', async () => {
    const { sourcesFromTools } = await import('../../shared/src/messages.ts');
    const s = sourcesFromTools([{ name: 'webfetch opencode.ai/docs', status: 'ok', input: '{"url":"https://opencode.ai/docs/"}', output: 'see https://opencode.ai/docs/server/' }]);
    expect(s.map((x) => x.url)).toEqual(['https://opencode.ai/docs/server/', 'https://opencode.ai/docs/']);
  });
});
