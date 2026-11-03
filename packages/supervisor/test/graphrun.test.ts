// R-38 graphs run on bpmn-engine with real work behind each step; R-41 Browser step via Playwright MCP.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { createServer, type Server } from 'node:http';
import { mkdtempSync, existsSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-graph-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'no-open-design');

let page: Server; let pageUrl = '';
// The agent step's model: a local OpenAI-compatible stand-in that answers from the prompt (the Browser step's text is
// passed in as context). Set HARNESS_TEST_MODEL to run the step on a real model instead.
let model: Server; let modelUrl = '';
let base = '', token = '', sup: any, close = async () => {};
const ok = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${t}`); return t ? JSON.parse(t) : {};
};
const until = async (fn: () => Promise<boolean>, ms: number) => { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return true; await new Promise((r) => setTimeout(r, 500)); } return false; };

function graphXml(url: string): string {
  return `<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI" xmlns:dc="http://www.omg.org/spec/DD/20100524/DC" xmlns:di="http://www.omg.org/spec/DD/20100524/DI" xmlns:harness="http://harness.local/schema/bpmn" id="D" targetNamespace="http://bpmn.io/schema/bpmn">
<bpmn:process id="P" name="Read and summarise" isExecutable="true">
<bpmn:startEvent id="Start" name="Start" />
<bpmn:serviceTask id="Read" name="Read the beta page" implementation="\${environment.services.agent}" harness:type="browser" harness:url="${url}" />
<bpmn:serviceTask id="Ask" name="Minimum wall" implementation="\${environment.services.agent}" harness:type="agent" harness:prompt="From the page the Browser step read, what is the minimum mold wall in mm? Reply with the number only." />
<bpmn:serviceTask id="Mark" name="Write result" implementation="\${environment.services.agent}" harness:type="script" harness:command="echo done &gt; graph-result.txt" />
<bpmn:endEvent id="End" name="End" />
<bpmn:sequenceFlow id="F1" sourceRef="Start" targetRef="Read" /><bpmn:sequenceFlow id="F2" sourceRef="Read" targetRef="Ask" /><bpmn:sequenceFlow id="F3" sourceRef="Ask" targetRef="Mark" /><bpmn:sequenceFlow id="F4" sourceRef="Mark" targetRef="End" />
</bpmn:process>
<bpmndi:BPMNDiagram id="DI"><bpmndi:BPMNPlane id="PL" bpmnElement="P">
<bpmndi:BPMNShape id="Start_di" bpmnElement="Start"><dc:Bounds x="100" y="100" width="36" height="36" /></bpmndi:BPMNShape>
<bpmndi:BPMNShape id="Read_di" bpmnElement="Read"><dc:Bounds x="180" y="78" width="100" height="80" /></bpmndi:BPMNShape>
<bpmndi:BPMNShape id="Ask_di" bpmnElement="Ask"><dc:Bounds x="320" y="78" width="100" height="80" /></bpmndi:BPMNShape>
<bpmndi:BPMNShape id="Mark_di" bpmnElement="Mark"><dc:Bounds x="460" y="78" width="100" height="80" /></bpmndi:BPMNShape>
<bpmndi:BPMNShape id="End_di" bpmnElement="End"><dc:Bounds x="600" y="100" width="36" height="36" /></bpmndi:BPMNShape>
<bpmndi:BPMNEdge id="F1_di" bpmnElement="F1"><di:waypoint x="136" y="118" /><di:waypoint x="180" y="118" /></bpmndi:BPMNEdge>
<bpmndi:BPMNEdge id="F2_di" bpmnElement="F2"><di:waypoint x="280" y="118" /><di:waypoint x="320" y="118" /></bpmndi:BPMNEdge>
<bpmndi:BPMNEdge id="F3_di" bpmnElement="F3"><di:waypoint x="420" y="118" /><di:waypoint x="460" y="118" /></bpmndi:BPMNEdge>
<bpmndi:BPMNEdge id="F4_di" bpmnElement="F4"><di:waypoint x="560" y="118" /><di:waypoint x="600" y="118" /></bpmndi:BPMNEdge>
</bpmndi:BPMNPlane></bpmndi:BPMNDiagram>
</bpmn:definitions>`;
}

beforeAll(async () => {
  page = createServer((_q, r) => r.writeHead(200, { 'content-type': 'text/html' }).end('<html><head><title>Castform beta</title></head><body><h1>Castform beta</h1><p>Mold wall minimum is 8 mm.</p></body></html>'));
  await new Promise<void>((r) => page.listen(0, '127.0.0.1', () => r()));
  const a = page.address(); pageUrl = `http://127.0.0.1:${typeof a === 'object' && a ? a.port : 0}/`;
  model = createServer(async (req, res) => {
    let b = ''; for await (const c of req) b += c;
    const j = JSON.parse(b || '{}');
    const last = (j.messages || []).filter((m: any) => m.role === 'user').pop();
    const text = typeof last?.content === 'string' ? last.content : JSON.stringify(last?.content || '');
    const answer = /(\d+)\s*mm/.exec(text)?.[1] || 'unknown';
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write(`data: ${JSON.stringify({ id: 'c', object: 'chat.completion.chunk', created: 1, model: j.model, choices: [{ index: 0, delta: { role: 'assistant', content: answer }, finish_reason: 'stop' }] })}\n\n`);
    res.end('data: [DONE]\n\n');
  });
  await new Promise<void>((r) => model.listen(0, '127.0.0.1', () => r()));
  modelUrl = `http://127.0.0.1:${(model.address() as any).port}/v1`;
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  const { installExtensions } = await import('../src/extensions.ts');
  sup = new Supervisor();
  const router = buildRouter();
  const server = await startServer(sup, router, {});
  installExtensions(sup, router, server);
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); page.close(); model.close(); };
});
afterAll(async () => { await close(); });

describe('graph runs do real work (R-38, R-41)', () => {
  let W = '', P = '', G = '';
  it('a dry run follows the path without calling anything', async () => {
    W = (await ok('POST', '/api/workspaces', { name: 'Graphs', kind: 'personal' })).id;
    P = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Castform', parent: null })).id;
    if (!process.env.HARNESS_TEST_MODEL) {
      sup.rt(W).ws.providers.push({ id: 'scripted', name: 'Scripted', note: '', npm: '@ai-sdk/openai-compatible', baseURL: modelUrl, models: [{ id: 'm' }] });
      await ok('PATCH', `/api/ws/${W}`, { defaultModel: 'scripted/m' });
    } else await ok('PATCH', `/api/ws/${W}`, { defaultModel: process.env.HARNESS_TEST_MODEL });
    const g = await ok('POST', `/api/ws/${W}/graphs`, { project: P, name: 'Read and summarise' });
    G = g.id;
    await ok('PUT', `/api/ws/${W}/graphs/${G}`, { xml: graphXml(pageUrl) });
    // Stored in the project folder, so it moves with the project.
    const file = (await ok('GET', `/api/ws/${W}/graphs/${G}`)).meta.file;
    expect(file.startsWith(sup.rt(W).project(P).folder + '/graphs/')).toBe(true);
    expect(readFileSync(file, 'utf8')).toContain('harness:type="browser"');
    const Q = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Other', parent: null })).id;
    await ok('PUT', `/api/ws/${W}/graphs/${G}`, { project: Q });
    const moved = (await ok('GET', `/api/ws/${W}/graphs/${G}`)).meta.file;
    expect(moved.startsWith(sup.rt(W).project(Q).folder + '/graphs/')).toBe(true);
    expect(existsSync(file)).toBe(false);
    await ok('PUT', `/api/ws/${W}/graphs/${G}`, { project: P });
    const run = await ok('POST', `/api/ws/${W}/graphs/${G}/run`, { dryRun: true });
    expect(await until(async () => (await ok('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id)?.status === 'completed', 20000)).toBe(true);
    const done = (await ok('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id);
    expect(done.log.map((l: any) => l.text).join('\n')).toMatch(/dry run · browser/);
    const folder = sup.rt(W).project(P).folder;
    expect(existsSync(join(folder, 'graph-result.txt'))).toBe(false);
    expect(done.chat ?? null).toBeNull();
  });

  it('a live run reads the page, asks the agent, runs the script, and keeps one transcript', async () => {
    const run = await ok('POST', `/api/ws/${W}/graphs/${G}/run`, { dryRun: false });
    const finished = await until(async () => ['completed', 'failed'].includes((await ok('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id)?.status), 180000);
    const done = (await ok('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id);
    expect(finished, JSON.stringify(done.log)).toBe(true);
    expect(done.status, JSON.stringify(done.log)).toBe('completed');
    expect(done.outputs.Read.title).toBe('Castform beta');
    expect(done.outputs.Read.text).toContain('8 mm');
    expect(String(done.outputs.Ask.text)).toMatch(/8/);
    const folder = sup.rt(W).project(P).folder;
    expect(readFileSync(join(folder, 'graph-result.txt'), 'utf8').trim()).toBe('done');
    const chat = sup.rt(W).chat(done.chat);
    expect(chat.title).toBe('Run · Read and summarise');
    expect(chat.project).toBe(P);
  });
});
