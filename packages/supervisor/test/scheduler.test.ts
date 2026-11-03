// R-27: routines fire on their own — schedules, one-offs, file changes, webhooks — with approval and sleep rules.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, writeFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-sched-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'none');

let base = '', token = '', sup: any, scheduler: any, close = async () => {};
const call = async (m: string, p: string, b?: unknown, auth = true) => {
  const r = await fetch(base + p, { method: m, headers: { ...(auth ? { 'x-harness-token': token } : {}), 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); return { status: r.status, data: t ? JSON.parse(t) : {} };
};
const ok = async (m: string, p: string, b?: unknown) => { const r = await call(m, p, b); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${JSON.stringify(r.data)}`); return r.data; };
const until = async (fn: () => Promise<boolean>, ms: number) => { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return true; await new Promise((r) => setTimeout(r, 250)); } return false; };

const XML = `<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI" xmlns:dc="http://www.omg.org/spec/DD/20100524/DC" xmlns:di="http://www.omg.org/spec/DD/20100524/DI" xmlns:harness="http://harness.local/schema/bpmn" id="D" targetNamespace="http://bpmn.io/schema/bpmn">
<bpmn:process id="P" name="Tick" isExecutable="true"><bpmn:startEvent id="S" name="Start" /><bpmn:serviceTask id="T" name="Tick" implementation="\${environment.services.agent}" harness:type="script" harness:command="echo tick &gt;&gt; ticks.txt" /><bpmn:endEvent id="E" name="End" />
<bpmn:sequenceFlow id="F1" sourceRef="S" targetRef="T" /><bpmn:sequenceFlow id="F2" sourceRef="T" targetRef="E" /></bpmn:process>
<bpmndi:BPMNDiagram id="DI"><bpmndi:BPMNPlane id="PL" bpmnElement="P"><bpmndi:BPMNShape id="S_di" bpmnElement="S"><dc:Bounds x="60" y="100" width="36" height="36" /></bpmndi:BPMNShape><bpmndi:BPMNShape id="T_di" bpmnElement="T"><dc:Bounds x="140" y="78" width="100" height="80" /></bpmndi:BPMNShape><bpmndi:BPMNShape id="E_di" bpmnElement="E"><dc:Bounds x="300" y="100" width="36" height="36" /></bpmndi:BPMNShape>
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

describe('routine scheduler (R-27)', () => {
  let W = '', P = '', G = '';
  const runsOf = async (id: string) => (await ok('GET', `/api/ws/${W}/snapshot`)).routines.find((r: any) => r.id === id).runs;
  const mk = async (b: Record<string, unknown>) => ok('POST', `/api/ws/${W}/routines`, { project: P, what: { kind: 'graph', graph: G }, approval: 'pre-approved', ...b });

  it('fires a due schedule once per slot, then waits for the next', async () => {
    W = (await ok('POST', '/api/workspaces', { name: 'S', kind: 'personal' })).id;
    const proj = await ok('POST', `/api/ws/${W}/projects`, { name: 'Sched', parent: null }); P = proj.id;
    G = (await ok('POST', `/api/ws/${W}/graphs`, { project: P, name: 'Tick' })).id;
    await ok('PUT', `/api/ws/${W}/graphs/${G}`, { xml: XML });
    const r = await mk({ phrase: 'every 15 minutes', name: 'Quarter' });
    // Anchor on real slots: just after the first slot, a minute later (same slot), then just after the next slot.
    const { nextRuns } = await import('../../shared/src/schedule.ts');
    const [slot1, slot2] = nextRuns(r.trigger.cron, r.trigger.tz, 2, new Date(r.createdAt));
    await scheduler.tick(new Date(slot1 + 5000));
    expect((await runsOf(r.id)).length).toBe(1);
    await scheduler.tick(new Date(slot1 + 65000));
    expect((await runsOf(r.id)).length).toBe(1);
    await scheduler.tick(new Date(slot2 + 5000));
    expect((await runsOf(r.id)).length).toBe(2);
  });

  it('a one-off fires once and is then done', async () => {
    const r = await mk({ phrase: 'in 3 hours', name: 'Once' });
    expect(r.trigger.once).toBeGreaterThan(Date.now());
    await scheduler.tick(new Date(Date.now() + 60 * 60000));
    expect((await runsOf(r.id)).length).toBe(0);
    await scheduler.tick(new Date(Date.now() + 4 * 3600e3));
    expect((await runsOf(r.id)).length).toBe(1);
    expect((await ok('GET', `/api/ws/${W}/snapshot`)).routines.find((x: any) => x.id === r.id).paused).toBe(true);
  });

  it('ask-first routines wait for approval', async () => {
    const r = await mk({ phrase: 'every 15 minutes', name: 'Careful', approval: 'ask' });
    await scheduler.tick(new Date(r.createdAt + 16 * 60000));
    expect((await runsOf(r.id))[0]).toMatchObject({ status: 'waiting' });
    await ok('POST', `/api/ws/${W}/routines/${r.id}/approve`);
    expect((await runsOf(r.id))[0].status).toBe('running');
  });

  it('a file change in the project fires its routine', async () => {
    mkdirSync(join(sup.rt(W).project(P).folder, 'src'), { recursive: true });
    const r = await mk({ phrase: 'when files in src change', name: 'On change' });
    expect(r.trigger).toMatchObject({ kind: 'event', event: 'file.changed', detail: 'src' });
    await scheduler.tick();
    writeFileSync(join(sup.rt(W).project(P).folder, 'src', 'a.ts'), 'export {}\n');
    expect(await until(async () => (await runsOf(r.id)).length > 0, 10000)).toBe(true);
    expect((await runsOf(r.id))[0].note).toMatch(/file changed/);
  });

  it('a webhook needs its routine secret', async () => {
    const r = await mk({ phrase: 'on webhook', name: 'Hook' });
    const { path } = await ok('GET', `/api/ws/${W}/routines/${r.id}/hook`);
    expect((await call('POST', `/api/hooks/${W}/${r.id}?key=wrong`, {}, false)).status).toBe(404);
    expect((await call('POST', path, { ref: 'main' }, false)).status).toBe(200);
    expect((await runsOf(r.id))[0].note).toMatch(/webhook/);
  });

  it('skips when the computer it runs on is asleep and the routine says skip', async () => {
    sup.computers.push({ id: 'pc1', name: 'Studio', os: 'linux', status: 'asleep', lastSeen: 0, capabilities: { files: true, commands: true, browser: true, computerUse: false, wake: false }, allowedApps: [], askFirst: {}, access: { [W]: { allowed: true, folders: [] } }, activity: [] });
    const r = await mk({ phrase: 'every 15 minutes', name: 'On PC', runsOn: 'computer', computer: 'pc1', ifAsleep: 'Skip' });
    await scheduler.tick(new Date(r.createdAt + 16 * 60000));
    expect((await runsOf(r.id))[0]).toMatchObject({ status: 'skipped' });
  });
});
