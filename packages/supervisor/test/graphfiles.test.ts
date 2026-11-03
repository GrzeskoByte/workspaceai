// Graphs as project files, run from a chat or by an agent; and the browser beside the chat (dev servers, frames).
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-gfiles-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'none');

// An ACP agent that answers every prompt with what it was sent, so the test can see what reached it.
const agent = join(home, 'echo-acp.cjs');
const record = join(home, 'echo-record.json');
writeFileSync(agent, `
const rl = require('readline').createInterface({ input: process.stdin });
const send = (m) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...m }) + '\\n');
const prompts = [];
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.method === 'initialize') send({ id: m.id, result: { protocolVersion: 1, agentCapabilities: {} } });
  else if (m.method === 'session/new') send({ id: m.id, result: { sessionId: 'E1' } });
  else if (m.method === 'session/prompt') {
    prompts.push(m.params.prompt[0].text); require('fs').writeFileSync(${JSON.stringify(record)}, JSON.stringify(prompts));
    send({ method: 'session/update', params: { sessionId: 'E1', update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'ok' } } } });
    send({ id: m.id, result: { stopReason: 'end_turn' } });
  } else if (m.id !== undefined) send({ id: m.id, result: {} });
});
`);

const HELLO = `<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" xmlns:harness="http://harness.local/schema/bpmn" id="D" targetNamespace="http://harness.local">
<bpmn:process id="P" name="Say hello" isExecutable="true">
<bpmn:startEvent id="S" name="Start" /><bpmn:sequenceFlow id="F1" sourceRef="S" targetRef="Greet" />
<bpmn:scriptTask id="Greet" name="Greet" harness:type="script" harness:command="echo hello-from-graph" /><bpmn:sequenceFlow id="F2" sourceRef="Greet" targetRef="E" />
<bpmn:endEvent id="E" name="End" /></bpmn:process></bpmn:definitions>`;

let base = '', token = '', sup: any, close = async () => {};
const call = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); return { status: r.status, body: t ? JSON.parse(t) : {} };
};
const ok = async (m: string, p: string, b?: unknown) => { const r = await call(m, p, b); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${JSON.stringify(r.body)}`); return r.body; };
const until = async (fn: () => Promise<boolean> | boolean, ms: number) => { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return true; await new Promise((r) => setTimeout(r, 150)); } return false; };

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

describe('graphs as project files', () => {
  let W = '', P = '';
  it('finds a .bpmn file dropped into the project folder and lays it out', async () => {
    W = (await ok('POST', '/api/workspaces', { name: 'G', kind: 'personal' })).id;
    P = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Site', parent: null })).id;
    const folder = sup.rt(W).project(P).folder;
    mkdirSync(join(folder, 'flows'), { recursive: true });
    writeFileSync(join(folder, 'flows', 'hello.bpmn'), HELLO);
    writeFileSync(join(folder, 'flows', 'notes.bpmn'), 'not bpmn');
    const { graphs } = await ok('POST', `/api/ws/${W}/graphs/scan`);
    const g = graphs.find((x: any) => x.file.endsWith('/flows/hello.bpmn'));
    expect(g).toMatchObject({ name: 'Say hello', project: P, source: 'file' });
    expect(graphs.some((x: any) => x.file.endsWith('notes.bpmn'))).toBe(false);
    // No diagram in the file: one is made, so it opens in the editor and passes the model check.
    const read = await ok('GET', `/api/ws/${W}/graphs/${g.id}`);
    expect(read.xml).toContain('BPMNShape');
    expect((await ok('POST', `/api/ws/${W}/graphs/${g.id}/lint`, {})).ok).toBe(true);
    // New graphs are visible files too.
    const made = await ok('POST', `/api/ws/${W}/graphs`, { project: P, name: 'Release check' });
    expect(made.file).toBe(join(folder, 'graphs', 'release-check.bpmn'));
    expect(existsSync(made.file)).toBe(true);
  });

  it('runs from a chat: the run shows there and its results reach the agent with the next message', async () => {
    const rt = sup.rt(W);
    await ok('PATCH', `/api/ws/${W}`, { acpAskFirst: false, agents: [...rt.ws.agents, { id: 'echo', name: 'Echo', via: 'ACP', command: `${process.execPath} ${agent}`, on: true }] });
    const chat = await ok('POST', `/api/ws/${W}/chats`, { project: P, title: 'Work', agent: 'echo' });
    const g = rt.findGraph('say hello');
    const run = await ok('POST', `/api/ws/${W}/graphs/${g.id}/run`, { from: chat.id });
    expect(await until(async () => (await ok('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id)?.status === 'completed', 15000)).toBe(true);
    // Where the run went, for the diagram: every path taken, every step finished, nothing left running.
    const finished = (await ok('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id);
    expect(finished).toMatchObject({ from: chat.id, active: [], taken: ['F1', 'F2'] });
    expect(finished.completed).toEqual(expect.arrayContaining(['S', 'Greet', 'E']));
    const { messages } = await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`);
    const card = messages.find((m: any) => m.run === run.id);
    expect(card.text).toContain('Graph “Say hello” completed');
    expect(card.text).toContain('hello-from-graph');
    await ok('POST', `/api/ws/${W}/chats/${chat.id}/send`, { text: 'what did it say?' });
    expect(await until(() => existsSync(record), 10000)).toBe(true);
    const sent = JSON.parse(require('node:fs').readFileSync(record, 'utf8')).at(-1);
    expect(sent).toContain('<harness-graph>');
    expect(sent).toContain('hello-from-graph');
    // The transcript keeps only what the user typed.
    expect(await until(async () => !(await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`)).streaming, 10000)).toBe(true);
    const after = (await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`)).messages.filter((m: any) => m.role === 'user');
    expect(after.at(-1).text).toBe('what did it say?');
  });

  it('agents list, save and run graphs with the harness tools', async () => {
    const list = await ok('GET', `/api/ws/${W}/agent/graphs`);
    expect(list.graphs.find((g: any) => g.name === 'Say hello').steps).toEqual(['Greet (script)']);
    const saved = await ok('POST', `/api/ws/${W}/agent/graphs`, { xml: HELLO.replace('Say hello', 'Say hi').replace('hello-from-graph', 'hi-from-agent') });
    expect(saved).toMatchObject({ name: 'Say hi', created: true });
    // Made outside a chat: no chat, so it sits at the project's root in the sidebar.
    expect(sup.rt(W).state.graphs.find((g: any) => g.id === saved.id).chat).toBeUndefined();
    const bad = await call('POST', `/api/ws/${W}/agent/graphs`, { xml: '<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"><bpmn:process id="P"><bpmn:task id="T" /></bpmn:process></bpmn:definitions>' });
    expect(bad.status).toBe(400);
    const done = await ok('POST', `/api/ws/${W}/agent/graphs/${encodeURIComponent('Say hi')}/run`, {});
    expect(done.status).toBe('completed');
    expect(done.summary).toContain('hi-from-agent');
  });

  it('marks the step that failed', async () => {
    const saved = await ok('POST', `/api/ws/${W}/agent/graphs`, { xml: HELLO.replace('Say hello', 'Breaks').replace('echo hello-from-graph', 'exit 3') });
    const run = await ok('POST', `/api/ws/${W}/graphs/${saved.id}/run`, {});
    expect(await until(async () => (await ok('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id)?.status === 'failed', 15000)).toBe(true);
    const failed = (await ok('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id);
    expect(failed).toMatchObject({ failedAt: 'Greet', active: [], taken: ['F1'] });
  });
});

describe('browser beside the chat', () => {
  it('runs the project dev server and finds the address it prints', async () => {
    const { detectDevCommand, devUrlIn } = await import('../src/devserver.ts');
    const dir = mkdtempSync(join(tmpdir(), 'devproj-'));
    writeFileSync(join(dir, 'package.json'), JSON.stringify({ scripts: { dev: 'vite' } }));
    writeFileSync(join(dir, 'pnpm-lock.yaml'), '');
    expect(detectDevCommand(dir)).toBe('pnpm run dev');
    expect(devUrlIn('  ➜  Local:   \x1b[36mhttp://0.0.0.0:5173/\x1b[39m')).toBe('http://localhost:5173/');
    const W = (await ok('POST', '/api/workspaces', { name: 'D', kind: 'personal' })).id;
    const P = (await ok('POST', `/api/ws/${W}/projects`, { name: 'App', parent: null })).id;
    const started = await ok('POST', `/api/ws/${W}/projects/${P}/dev`, { action: 'start', command: 'echo "Local: http://localhost:4321/"; sleep 30' });
    expect(started.running).toBe(true);
    expect(await until(async () => (await ok('GET', `/api/ws/${W}/projects/${P}/dev`)).url === 'http://localhost:4321/', 5000)).toBe(true);
    expect(sup.rt(W).project(P).devCommand).toBe('echo "Local: http://localhost:4321/"; sleep 30');
    await ok('POST', `/api/ws/${W}/projects/${P}/dev`, { action: 'stop' });
    expect(await until(async () => !(await ok('GET', `/api/ws/${W}/projects/${P}/dev`)).running, 6000)).toBe(true);
  });

  it('says when a page refuses to be framed', async () => {
    const srv = createServer((req, res) => { if (req.url === '/deny') res.setHeader('x-frame-options', 'DENY'); res.end('hi'); });
    await new Promise<void>((r) => srv.listen(0, '127.0.0.1', r));
    const port = (srv.address() as any).port;
    expect(await ok('GET', `/api/preview/check?url=${encodeURIComponent(`http://127.0.0.1:${port}/`)}`)).toMatchObject({ ok: true, embeddable: true });
    expect(await ok('GET', `/api/preview/check?url=${encodeURIComponent(`http://127.0.0.1:${port}/deny`)}`)).toMatchObject({ embeddable: false });
    srv.close();
  });
});
