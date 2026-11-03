// Live BPMN runs for every workspace. State is written after each update so a quit can resume (R-38).
import type { BpmnEngineExecutionState } from 'bpmn-engine';
import type { GraphRun } from '../../shared/src/types.ts';
import { uid } from '../../shared/src/util.ts';
import { launchGraph, type LiveGraph, type RunUpdate } from '../../orchestrator/src/runner.ts';
import { lintBpmn } from '../../orchestrator/src/lint.ts';
import type { Supervisor } from './supervisor.ts';
import type { WorkspaceRuntime } from './runtime.ts';
import { spawn } from 'node:child_process';
import type { StepRequest } from '../../orchestrator/src/runner.ts';
import { mapOcMessages } from '../../shared/src/messages.ts';
import { expandHome } from './paths.ts';
import { browseOnce } from './browser.ts';
import { baseEnv } from './opencode.ts';

interface Live { rt: WorkspaceRuntime; handle: LiveGraph; run: GraphRun; engineState: unknown }

const live = new Map<string, Live>();

function apply(entry: Live, u: RunUpdate): void {
  const run = entry.run;
  if (u.status) run.status = u.status;
  if (u.error) run.log = [...run.log, { at: Date.now(), element: '', text: u.error }].slice(-200);
  if (u.log) run.log = [...run.log, u.log].slice(-200);
  if (u.started) run.active = [...new Set([...(run.active || []), u.started])];
  if (u.taken) run.taken = [...new Set([...(run.taken || []), u.taken])];
  if (u.failedAt) { run.failedAt = u.failedAt; run.active = (run.active || []).filter((x) => x !== u.failedAt); }
  if (u.completed) { run.completed = [...new Set([...run.completed, u.completed])]; run.active = (run.active || []).filter((x) => x !== u.completed); }
  if (u.status && u.status !== 'running' && u.status !== 'waiting' && u.status !== 'paused') run.active = [];
  if (u.waiting !== undefined) run.waiting = u.waiting;
  if (u.state !== undefined) entry.engineState = u.state;
  if (u.outputs) run.outputs = { ...(run.outputs || {}), ...u.outputs };
  run.updatedAt = Date.now();
  entry.rt.saveStoredRun(run, entry.engineState);
  entry.rt.emit({ type: 'graph.run', workspace: entry.rt.id, run: { ...run, log: run.log.slice(-30), completed: [...run.completed], active: [...(run.active || [])], taken: [...(run.taken || [])], chats: [...(run.chats || [])] } });
  const settled = run.status === 'completed' || run.status === 'failed' || run.status === 'stopped';
  if (run.from && (settled || (u.status === 'waiting' && run.waiting))) entry.rt.graphRunSettled(run);
  if (settled) live.delete(run.id);
}

/** What a run produced, for the chat it was started from and its agent. */
export function runSummary(run: GraphRun, graphName: string): string {
  const head = run.status === 'completed' ? `Graph “${graphName}” ${run.dryRun ? 'dry run ' : ''}completed`
    : run.status === 'waiting' && run.waiting ? `Graph “${graphName}” is waiting at “${run.waiting.name || run.waiting.element}”${run.waiting.prompt ? ': ' + run.waiting.prompt : ''}`
    : `Graph “${graphName}” ${run.status}${run.log.length ? ': ' + run.log[run.log.length - 1].text : ''}`;
  const outs = Object.entries(run.outputs || {}).map(([id, o]) => {
    const t = typeof o === 'object' && o ? String((o as { text?: string; stdout?: string }).text ?? (o as { stdout?: string }).stdout ?? JSON.stringify(o)) : String(o);
    return `- ${id}: ${t.trim().slice(0, 700)}`;
  });
  return (head + (outs.length ? '\n\nStep results:\n' + outs.join('\n') : '')).slice(0, 4000);
}

function context(outputs: Record<string, unknown>): string {
  const parts = Object.entries(outputs).map(([id, o]) => `- ${id}: ${typeof o === 'object' && o ? String((o as { text?: string }).text ?? JSON.stringify(o)).slice(0, 1500) : String(o)}`);
  return parts.length ? '\n\nResults of earlier steps in this graph:\n' + parts.join('\n') : '';
}

/**
 * The work behind each live step (R-38): agent and rule steps prompt this run's opencode session,
 * a Browser step reads a page through Playwright MCP (R-41), a script step runs in the project folder.
 */
function executor(rt: WorkspaceRuntime, run: GraphRun, projectId: string, graphName: string, touch: () => void) {
  let chatId: string | null = run.chat || null;
  // The run lists its chats at once, so an approval one of them asks for shows on the run.
  const own = (id: string) => { if (!(run.chats || []).includes(id)) { run.chats = [...(run.chats || []), id]; touch(); } };
  // Agent tasks with harness:runner="acp:<id>" run in that ACP agent, one chat per agent per run.
  const acpChats = new Map<string, string>();
  const session = async (runner?: string) => {
    const acp = runner?.startsWith('acp:') ? runner.slice(4) : '';
    if (acp) {
      if (!rt.ws.agents.some((a) => a.id === acp && a.on)) throw new Error(`ACP agent “${acp}” is not enabled in this workspace`);
      const existing = acpChats.get(acp);
      if (existing && rt.chat(existing)) return rt.chat(existing)!;
      const chat = await rt.createChat({ project: projectId, title: `Run · ${graphName} · ${acp}`, agent: acp });
      acpChats.set(acp, chat.id);
      own(chat.id);
      return chat;
    }
    if (!chatId || !rt.chat(chatId)) {
      const chat = await rt.createChat({ project: projectId, title: `Run · ${graphName}`, agent: 'opencode' });
      chatId = chat.id; run.chat = chat.id;
      own(chat.id);
    }
    return rt.chat(chatId)!;
  };
  return async (step: StepRequest): Promise<unknown> => {
    const project = rt.project(projectId);
    const folder = expandHome(project?.folder || rt.general().folder);
    if (step.type === 'browser') {
      const url = step.attrs.url || /(https?:\/\/\S+)/.exec(step.attrs.prompt || step.name)?.[1];
      if (!url) throw new Error('Set the page URL on this Browser step');
      return browseOnce(url, { profileDir: rt.paths.browserProfile });
    }
    if (step.type === 'script') {
      const cmd = step.attrs.command || step.attrs.prompt;
      if (!cmd) throw new Error('Set the command on this script step');
      return new Promise((resolve, reject) => {
        const p = spawn('sh', ['-c', cmd], { cwd: folder, env: { ...baseEnv(), HARNESS_RUN: run.id }, stdio: ['ignore', 'pipe', 'pipe'] });
        let out = ''; let err = '';
        const t = setTimeout(() => p.kill('SIGKILL'), Number(step.attrs.timeout || 120) * 1000);
        p.stdout.on('data', (b) => { out += b; }); p.stderr.on('data', (b) => { err += b; });
        p.on('close', (code) => { clearTimeout(t); code === 0 ? resolve({ exitCode: 0, stdout: out.slice(-20000) }) : reject(new Error(`exit ${code}: ${(err || out).slice(-400)}`)); });
      });
    }
    // agent, rule and service steps run as a turn of this run's opencode session.
    const chat = await session(step.attrs.runner || (step.attrs.agent && step.attrs.agent !== 'opencode' ? 'acp:' + step.attrs.agent : ''));
    const base = step.attrs.prompt || step.name;
    const ask = step.type === 'rule'
      ? `${base}\n\nJudge the work so far. Answer with JSON only: {"verdict":"pass"|"fail","score":0..1,"reason":"…"}.`
      : step.type === 'service' && step.attrs.mcp
        ? `Call the MCP tool ${step.attrs.mcp} exactly once for this: ${base}\nReply with what it returned.`
        : base;
    // A step runs without asking unless it is marked "Ask first"; then its approvals show on the run.
    const yolo = step.attrs.askFirst !== 'true';
    if (!!chat.yolo !== yolo) await rt.setChatYolo(chat.id, yolo).catch(() => { chat.yolo = yolo; });
    if (step.attrs.model && chat.agent === 'opencode') await (await rt.client()).switchModel(chat.sid, step.attrs.model).catch(() => {});
    const text = await rt.askAndWait(chat.id, `[Graph step ${step.element} of “${graphName}”] ${ask}${context(step.outputs)}`, Number(step.attrs.timeout || 600) * 1000);
    if (step.type === 'rule') { try { return { ...JSON.parse(/\{[\s\S]*\}/.exec(text)?.[0] || '{}'), text }; } catch { return { verdict: 'unknown', text }; } }
    return { text, chat: chat.id };
  };
}

export async function startGraphRun(rt: WorkspaceRuntime, graphId: string, dryRun: boolean, o: { from?: string | null; tellAgent?: boolean } = {}): Promise<GraphRun> {
  const { meta, xml } = rt.readGraph(graphId);
  const checked = await lintBpmn(xml);
  if (!checked.ok) throw new Error(checked.issues.filter((i) => i.category === 'error').map((i) => i.message).join('; ') || 'This graph breaks a model rule');
  const run: GraphRun = { id: uid('run'), graph: meta.id, status: 'running', dryRun, startedAt: Date.now(), updatedAt: Date.now(), log: [{ at: Date.now(), element: '', text: dryRun ? 'Dry run started' : 'Run started' }], waiting: null, completed: [], ...(o.from && rt.chat(o.from) ? { from: o.from, tellAgent: !!o.tellAgent } : {}) };
  const entry: Live = { rt, run, handle: null as unknown as LiveGraph, engineState: null };
  entry.handle = launchGraph({
    source: xml,
    dryRun,
    exec: executor(rt, run, meta.project, meta.name, () => apply(entry, {})),
    onUpdate: (u) => apply(entry, u),
  });
  live.set(run.id, entry);
  rt.saveStoredRun(run, null);
  if (run.from) rt.graphRunStarted(run, meta.name);
  rt.audit('graph.run', `${meta.name}${dryRun ? ' · dry' : ''}`);
  return run;
}

export function signalGraphRun(rt: WorkspaceRuntime, runId: string, elementId: string): GraphRun {
  const entry = live.get(runId);
  if (!entry || entry.rt.id !== rt.id) throw new Error('That run is not active. Open it again to resume.');
  entry.handle.signal(elementId);
  return entry.run;
}

export async function pauseGraphRun(rt: WorkspaceRuntime, runId: string): Promise<GraphRun> {
  const entry = live.get(runId);
  if (!entry || entry.rt.id !== rt.id) throw new Error('That run is not active');
  await entry.handle.pause();
  return entry.run;
}

export async function resumeGraphRun(rt: WorkspaceRuntime, runId: string): Promise<GraphRun> {
  const existing = live.get(runId);
  if (existing && existing.rt.id === rt.id) { await existing.handle.resume(); return existing.run; }
  const stored = rt.listStoredRuns().find((r) => r.run.id === runId);
  if (!stored) throw new Error('Run not found');
  return recoverOne(rt, stored.run, stored.engineState, false);
}

async function recoverOne(rt: WorkspaceRuntime, run: GraphRun, engineState: unknown, hold: boolean): Promise<GraphRun> {
  const { xml, meta } = rt.readGraph(run.graph);
  const entry: Live = { rt, run, handle: null as unknown as LiveGraph, engineState };
  entry.handle = launchGraph({
    source: xml,
    saved: (engineState as BpmnEngineExecutionState | null) ?? null,
    dryRun: run.dryRun,
    hold,
    exec: executor(rt, run, meta.project, meta.name, () => apply(entry, {})),
    outputs: run.outputs,
    onUpdate: (u) => apply(entry, u),
  });
  live.set(run.id, entry);
  return run;
}

export async function recoverGraphRuns(sup: Supervisor): Promise<void> {
  for (const rt of sup.runtimes.values()) {
    for (const row of rt.listStoredRuns()) {
      if (row.run.status !== 'running' && row.run.status !== 'waiting') continue;
      if (live.has(row.run.id)) continue;
      try { await recoverOne(rt, row.run, row.engineState, false); }
      catch (e) { row.run.status = 'failed'; row.run.log = [...row.run.log, { at: Date.now(), element: '', text: (e as Error).message }]; rt.saveStoredRun(row.run, row.engineState); }
    }
  }
}

export async function stopGraphRuns(): Promise<void> {
  await Promise.all(Array.from(live.values()).map(async (entry) => {
    const state = await entry.handle.state().catch(() => entry.engineState);
    entry.rt.saveStoredRun(entry.run, state);
  }));
  live.clear();
}
