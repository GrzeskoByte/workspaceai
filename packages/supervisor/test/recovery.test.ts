// R-38: quitting the app mid-run and reopening it resumes from the last completed step.
import { describe, it, expect } from 'vitest';
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtempSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

const home = mkdtempSync(join(tmpdir(), 'harness-recover-'));
const env = { ...process.env, HARNESS_HOME: home, HARNESS_KEYCHAIN: 'memory', HARNESS_OD_DIR: join(home, 'none') };

async function boot(): Promise<{ proc: ChildProcess; call: (m: string, p: string, b?: unknown) => Promise<any> }> {
  const proc = spawn(process.execPath, [join(__dirname, '../src/main.ts')], { env, stdio: ['ignore', 'pipe', 'inherit'], detached: true });
  const ready: { port: number; token: string } = await new Promise((resolve) => createInterface({ input: proc.stdout! }).on('line', (l) => { if (l.startsWith('HARNESS_READY ')) resolve(JSON.parse(l.slice(14))); }));
  const call = async (m: string, p: string, b?: unknown) => {
    const r = await fetch(`http://127.0.0.1:${ready.port}${p}`, { method: m, headers: { 'x-harness-token': ready.token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
    const t = await r.text(); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${t}`); return t ? JSON.parse(t) : {};
  };
  return { proc, call };
}
const until = async (fn: () => Promise<boolean>, ms: number) => { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return true; await new Promise((r) => setTimeout(r, 300)); } return false; };

const shape = (id: string, x: number, w = 100, h = 80) => `<bpmndi:BPMNShape id="${id}_di" bpmnElement="${id}"><dc:Bounds x="${x}" y="100" width="${w}" height="${h}" /></bpmndi:BPMNShape>`;
const XML = `<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI" xmlns:dc="http://www.omg.org/spec/DD/20100524/DC" xmlns:di="http://www.omg.org/spec/DD/20100524/DI" xmlns:harness="http://harness.local/schema/bpmn" id="D" targetNamespace="http://bpmn.io/schema/bpmn">
<bpmn:process id="P" name="Approve then finish" isExecutable="true">
<bpmn:startEvent id="Start" name="Start" />
<bpmn:serviceTask id="First" name="First" implementation="\${environment.services.agent}" harness:type="script" harness:command="echo first &gt;&gt; trail.txt" />
<bpmn:userTask id="Approve" name="Approve" />
<bpmn:serviceTask id="Second" name="Second" implementation="\${environment.services.agent}" harness:type="script" harness:command="echo second &gt;&gt; trail.txt" />
<bpmn:endEvent id="End" name="End" />
<bpmn:sequenceFlow id="F1" sourceRef="Start" targetRef="First" /><bpmn:sequenceFlow id="F2" sourceRef="First" targetRef="Approve" /><bpmn:sequenceFlow id="F3" sourceRef="Approve" targetRef="Second" /><bpmn:sequenceFlow id="F4" sourceRef="Second" targetRef="End" />
</bpmn:process>
<bpmndi:BPMNDiagram id="DI"><bpmndi:BPMNPlane id="PL" bpmnElement="P">${shape('Start', 60, 36, 36)}${shape('First', 140)}${shape('Approve', 280)}${shape('Second', 420)}${shape('End', 560, 36, 36)}
${['F1', 'F2', 'F3', 'F4'].map((f, i) => `<bpmndi:BPMNEdge id="${f}_di" bpmnElement="${f}"><di:waypoint x="${100 + i * 140}" y="140" /><di:waypoint x="${140 + i * 140}" y="140" /></bpmndi:BPMNEdge>`).join('')}
</bpmndi:BPMNPlane></bpmndi:BPMNDiagram></bpmn:definitions>`;

describe('crash recovery (R-38)', () => {
  it('a run waiting for a person survives the supervisor being killed and continues from the last completed step', async () => {
    let a = await boot();
    const W = (await a.call('POST', '/api/workspaces', { name: 'R', kind: 'personal' })).id;
    const P = await a.call('POST', `/api/ws/${W}/projects`, { name: 'Recover', parent: null });
    const G = (await a.call('POST', `/api/ws/${W}/graphs`, { project: P.id, name: 'Approve then finish' })).id;
    await a.call('PUT', `/api/ws/${W}/graphs/${G}`, { xml: XML });
    const run = await a.call('POST', `/api/ws/${W}/graphs/${G}/run`, { dryRun: false });
    expect(await until(async () => (await a.call('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id)?.status === 'waiting', 30000)).toBe(true);
    expect(readFileSync(join(P.folder, 'trail.txt'), 'utf8')).toBe('first\n');

    // Crash: no shutdown hooks run.
    process.kill(-a.proc.pid!, 'SIGKILL');
    await new Promise((r) => setTimeout(r, 500));

    const b = await boot();
    try {
      const recovered = (await b.call('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id);
      expect(recovered.status).toBe('waiting');
      expect(recovered.waiting.element).toBe('Approve');
      await b.call('POST', `/api/ws/${W}/runs/${run.id}/signal`, { element: 'Approve' });
      expect(await until(async () => (await b.call('GET', `/api/ws/${W}/runs`)).runs.find((r: any) => r.id === run.id)?.status === 'completed', 30000)).toBe(true);
      expect(readFileSync(join(P.folder, 'trail.txt'), 'utf8')).toBe('first\nsecond\n');
    } finally { process.kill(-b.proc.pid!, 'SIGTERM'); }
    expect(existsSync(join(P.folder, 'trail.txt'))).toBe(true);
  });
});
