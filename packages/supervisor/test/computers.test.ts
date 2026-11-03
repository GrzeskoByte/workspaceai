// R-28: pair, rename, wake, unlink; capabilities, apps, per-workspace access, ask-first; activity and a live view
// that can be stopped or taken over. Each control changes the computer's state.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { magicPacket } from '../src/computers.ts';

const home = mkdtempSync(join(tmpdir(), 'harness-pc-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'none');

let base = '', token = '', sup: any, scheduler: any, close = async () => {};
const call = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); return { status: r.status, data: t ? JSON.parse(t) : {} };
};
const ok = async (m: string, p: string, b?: unknown) => { const r = await call(m, p, b); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${JSON.stringify(r.data)}`); return r.data; };
const pc = async (id: string) => (await ok('GET', '/api/state')).computers.find((c: any) => c.id === id);
const until = async (fn: () => Promise<boolean>, ms: number) => { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return true; await new Promise((r) => setTimeout(r, 250)); } return false; };

const XML = `<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI" xmlns:dc="http://www.omg.org/spec/DD/20100524/DC" xmlns:di="http://www.omg.org/spec/DD/20100524/DI" xmlns:harness="http://harness.local/schema/bpmn" id="D" targetNamespace="http://bpmn.io/schema/bpmn">
<bpmn:process id="P" name="Render" isExecutable="true"><bpmn:startEvent id="S" name="Start" /><bpmn:userTask id="Check" name="Check the render" /><bpmn:endEvent id="E" name="End" />
<bpmn:sequenceFlow id="F1" sourceRef="S" targetRef="Check" /><bpmn:sequenceFlow id="F2" sourceRef="Check" targetRef="E" /></bpmn:process>
<bpmndi:BPMNDiagram id="DI"><bpmndi:BPMNPlane id="PL" bpmnElement="P"><bpmndi:BPMNShape id="S_di" bpmnElement="S"><dc:Bounds x="60" y="100" width="36" height="36" /></bpmndi:BPMNShape><bpmndi:BPMNShape id="Check_di" bpmnElement="Check"><dc:Bounds x="140" y="78" width="100" height="80" /></bpmndi:BPMNShape><bpmndi:BPMNShape id="E_di" bpmnElement="E"><dc:Bounds x="300" y="100" width="36" height="36" /></bpmndi:BPMNShape>
<bpmndi:BPMNEdge id="F1_di" bpmnElement="F1"><di:waypoint x="96" y="118" /><di:waypoint x="140" y="118" /></bpmndi:BPMNEdge><bpmndi:BPMNEdge id="F2_di" bpmnElement="F2"><di:waypoint x="240" y="118" /><di:waypoint x="300" y="118" /></bpmndi:BPMNEdge></bpmndi:BPMNPlane></bpmndi:BPMNDiagram></bpmn:definitions>`;

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

describe('computers (R-28)', () => {
  let W = '', id = '';
  it('pairs with a single-use code and starts with no workspace access', async () => {
    W = (await ok('POST', '/api/workspaces', { name: 'P', kind: 'personal' })).id;
    const { code } = await ok('POST', '/api/computers/pairing-code');
    expect(code).toMatch(/^[A-Z2-9]{3}-[A-Z2-9]{3}$/);
    const c = await ok('POST', '/api/computers/pair', { code, name: 'Studio desktop', os: 'linux', mac: '00:11:22:33:44:55' });
    id = c.id;
    expect(c.access).toEqual({});
    expect((await call('POST', '/api/computers/pair', { code, name: 'Again' })).status).toBe(403);
  });

  it('every setting changes the computer’s state', async () => {
    await ok('PATCH', `/api/computers/${id}`, { name: 'Render box' });
    await ok('PATCH', `/api/computers/${id}`, { capabilities: { computerUse: true, wake: true } });
    await ok('PATCH', `/api/computers/${id}`, { allowedApps: ['Blender', 'Terminal'] });
    await ok('PATCH', `/api/computers/${id}`, { askFirst: { purchases: false } });
    await ok('PATCH', `/api/computers/${id}`, { access: { [W]: { allowed: true, folders: [{ path: '~/renders', mode: 'ro' }] } } });
    const c = await pc(id);
    expect(c.name).toBe('Render box');
    expect(c.capabilities).toMatchObject({ computerUse: true, wake: true });
    expect(c.allowedApps).toEqual(['Blender', 'Terminal']);
    expect(c.askFirst.purchases).toBe(false);
    expect(c.access[W].folders[0]).toEqual({ path: '~/renders', mode: 'ro' });
    expect(c.activity.length).toBeGreaterThan(3);
  });

  it('builds a correct Wake-on-LAN packet and refuses to wake without permission', async () => {
    const pkt = magicPacket('00:11:22:33:44:55');
    expect(pkt.length).toBe(102);
    expect(pkt.subarray(0, 6).every((b) => b === 0xff)).toBe(true);
    expect(pkt.subarray(6, 12).toString('hex')).toBe('001122334455');
    await ok('PATCH', `/api/computers/${id}`, { status: 'asleep', capabilities: { wake: false } });
    expect((await call('POST', `/api/computers/${id}/wake`)).data.error).toMatch(/does not allow waking/);
    await ok('PATCH', `/api/computers/${id}`, { capabilities: { wake: true } });
    const r = await call('POST', `/api/computers/${id}/wake`);
    // Sending a broadcast may be refused in a sandbox; either way the attempt is recorded or explained.
    expect(r.status === 200 ? r.data.activity[0].text : r.data.error).toMatch(/Wake packet sent|EACCES|EPERM|ENETUNREACH|network/i);
    await ok('POST', `/api/computers/${id}/check-in`);
    expect((await pc(id)).status).toBe('online');
  });

  it('live view: a routine on the computer can be taken over, handed back and stopped', async () => {
    const P = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Renders', parent: null })).id;
    const G = (await ok('POST', `/api/ws/${W}/graphs`, { project: P, name: 'Render' })).id;
    await ok('PUT', `/api/ws/${W}/graphs/${G}`, { xml: XML });
    const r = await ok('POST', `/api/ws/${W}/routines`, { project: P, phrase: 'every 15 minutes', name: 'Nightly render', what: { kind: 'graph', graph: G }, approval: 'pre-approved', runsOn: 'computer', computer: id });
    await ok('POST', `/api/ws/${W}/routines/${r.id}/run`);
    expect(await until(async () => (await pc(id)).live?.state === 'running', 5000)).toBe(true);
    expect((await pc(id)).live.title).toBe('Nightly render');
    await until(async () => /waiting/.test((await pc(id)).live?.action || ''), 5000);
    await ok('POST', `/api/computers/${id}/live/takeover`);
    expect((await pc(id)).live).toMatchObject({ state: 'paused', controlledBy: 'you' });
    await ok('POST', `/api/computers/${id}/live/handback`);
    expect((await pc(id)).live).toMatchObject({ state: 'running', controlledBy: 'agent' });
    await ok('POST', `/api/computers/${id}/live/stop`);
    expect((await pc(id)).live.state).toBe('stopped');
    expect((await call('POST', `/api/computers/${id}/live/stop`)).status).toBe(400);
  });

  it('unlinks', async () => {
    await ok('DELETE', `/api/computers/${id}`);
    expect(await pc(id)).toBeUndefined();
  });
});
