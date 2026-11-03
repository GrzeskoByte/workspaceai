// Routes for the screens Claude had not written yet: routines, graphs, computers.
import type { Router } from './server.ts';
import type { Routine } from '../../shared/src/types.ts';
import { uid } from '../../shared/src/util.ts';
import { parseSchedule } from '../../shared/src/schedule.ts';
import { blankGraph, PATTERNS } from '../../orchestrator/src/model.ts';
import { lintBpmn } from '../../orchestrator/src/lint.ts';
import { pauseGraphRun, resumeGraphRun, signalGraphRun, startGraphRun } from './graphs.ts';
import type { WorkspaceRuntime } from './runtime.ts';
import { perform, trigger, webhookKey } from './scheduler.ts';
import { newPairingCode, redeemPairingCode, controlLive } from './computers.ts';

function err(message: string, status = 400): never { throw Object.assign(new Error(message), { status }); }

function routineOf(rt: WorkspaceRuntime, id: string): Routine {
  return rt.state.routines.find((r) => r.id === id) || err('Routine not found', 404);
}

export function registerScreens(r: Router): void {
  r.post('/api/ws/:id/routines', ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    const phrase = String(body.phrase || '').trim();
    const parsed = parseSchedule(phrase, { tz: body.tz });
    if (!parsed.ok) err(parsed.error || 'Could not read that schedule');
    const project = String(body.project || rt.ws.defaultProject || '');
    const what = body.what?.kind === 'graph'
      ? { kind: 'graph' as const, graph: String(body.what.graph || '') }
      : body.what?.kind === 'artifact'
        ? { kind: 'artifact' as const, artifact: String(body.what.artifact || '') }
        : { kind: 'prompt' as const, prompt: String(body.what?.prompt || body.prompt || phrase) };
    if (what.kind === 'graph' && !what.graph) err('Pick a graph');
    if (what.kind === 'prompt' && !what.prompt.trim()) err('Write what the routine should do');
    return rt.createRoutine({
      name: String(body.name || phrase).slice(0, 80),
      phrase,
      trigger: parsed.kind === 'event' && parsed.event
        ? { kind: 'event', event: parsed.event.event, detail: parsed.event.detail }
        : { kind: 'schedule', cron: parsed.cron, tz: parsed.tz, once: parsed.once },
      what,
      project,
      runsOn: body.runsOn === 'computer' || body.runsOn === 'org-runner' ? body.runsOn : 'cloud',
      computer: body.computer,
      approval: body.approval === 'pre-approved' ? 'pre-approved' : 'ask',
      preApproved: Array.isArray(body.preApproved) ? body.preApproved.map(String) : [],
      ifAsleep: body.ifAsleep || rt.ws.routineIfAsleep,
      notify: Array.isArray(body.notify) ? body.notify : ['chat'],
    });
  });
  r.patch('/api/ws/:id/routines/:rid', ({ sup, params, body }) => sup.rt(params.id).updateRoutine(params.rid, body));
  r.del('/api/ws/:id/routines/:rid', ({ sup, params }) => { sup.rt(params.id).deleteRoutine(params.rid); return { ok: true }; });
  r.post('/api/ws/:id/routines/:rid/run', async ({ sup, params }) => {
    const rt = sup.rt(params.id);
    const routine = routineOf(rt, params.rid);
    if (routine.paused) err('This routine is paused');
    await trigger(sup, rt, routine, 'run now');
    return routine;
  });
  r.post('/api/hooks/:id/:rid', async ({ sup, params, query, body }) => {
    const rt = sup.rt(params.id);
    const routine = routineOf(rt, params.rid);
    if (!routine.hookKey || query.get('key') !== routine.hookKey) err('Unknown webhook', 404);
    if (routine.paused) return { ok: false, paused: true };
    rt.audit('routine.webhook', routine.name, 'webhook');
    await trigger(sup, rt, routine, 'webhook' + (body && Object.keys(body).length ? ' · ' + JSON.stringify(body).slice(0, 80) : ''));
    return { ok: true };
  });
  r.get('/api/ws/:id/routines/:rid/hook', ({ sup, params }) => {
    const rt = sup.rt(params.id);
    const routine = routineOf(rt, params.rid);
    const key = webhookKey(routine); rt.save();
    return { path: `/api/hooks/${rt.id}/${routine.id}?key=${key}` };
  });
  r.post('/api/ws/:id/routines/:rid/approve', async ({ sup, params }) => {
    const rt = sup.rt(params.id);
    const routine = routineOf(rt, params.rid);
    const waiting = routine.runs.find((x) => x.status === 'waiting');
    if (waiting) { waiting.status = 'ok'; waiting.note = 'Approved'; }
    return perform(rt, routine, 'approved', sup);
  });

  r.get('/api/ws/:id/graphs/:gid', ({ sup, params }) => sup.rt(params.id).readGraph(params.gid));
  r.post('/api/ws/:id/graphs', ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    const pattern = PATTERNS.find((p) => p.id === body.pattern);
    const xml = typeof body.xml === 'string' && body.xml.trim() ? body.xml : pattern?.xml || blankGraph(body.name);
    return rt.createGraph({ name: body.name || pattern?.name || 'New graph', project: body.project || rt.ws.defaultProject, xml, pattern: pattern?.id });
  });
  r.put('/api/ws/:id/graphs/:gid', ({ sup, params, body }) => sup.rt(params.id).saveGraph(params.gid, { name: body.name, project: body.project, xml: body.xml }));
  r.del('/api/ws/:id/graphs/:gid', ({ sup, params }) => { sup.rt(params.id).deleteGraph(params.gid); return { ok: true }; });
  r.post('/api/ws/:id/graphs/:gid/lint', async ({ sup, params, body }) => {
    const xml = typeof body.xml === 'string' ? body.xml : sup.rt(params.id).readGraph(params.gid).xml;
    return lintBpmn(xml);
  });
  // From a chat (`/graph`): the run shows in that chat and its agent gets the results with the next message.
  r.post('/api/ws/:id/graphs/:gid/run', ({ sup, params, body }) => startGraphRun(sup.rt(params.id), params.gid, !!body.dryRun, body.from ? { from: String(body.from), tellAgent: true } : {}));
  // Pick up .bpmn files added to project folders.
  r.post('/api/ws/:id/graphs/scan', ({ sup, params }) => { const rt = sup.rt(params.id); return { changed: rt.scanGraphFiles(), graphs: rt.state.graphs }; });
  r.get('/api/ws/:id/runs', ({ sup, params }) => ({ runs: sup.rt(params.id).listStoredRuns().map((r) => r.run) }));
  r.post('/api/ws/:id/runs/:rid/signal', ({ sup, params, body }) => signalGraphRun(sup.rt(params.id), params.rid, String(body.element || body.id || '')));
  r.post('/api/ws/:id/runs/:rid/pause', ({ sup, params }) => pauseGraphRun(sup.rt(params.id), params.rid));
  r.post('/api/ws/:id/runs/:rid/resume', ({ sup, params }) => resumeGraphRun(sup.rt(params.id), params.rid));

  r.post('/api/computers', ({ sup, body }) => sup.pairComputer(String(body.name || '')));
  r.patch('/api/computers/:id', ({ sup, params, body }) => sup.updateComputer(params.id, body));
  r.post('/api/computers/:id/wake', ({ sup, params }) => sup.wakeComputer(params.id));
  r.post('/api/computers/pairing-code', () => newPairingCode());
  // Called by the desktop app on the other computer with the code the user reads off this screen.
  r.post('/api/computers/pair', ({ sup, body }) => redeemPairingCode(sup, String(body.code || ''), { name: String(body.name || ''), os: body.os, mac: body.mac }));
  r.post('/api/computers/:id/check-in', ({ sup, params }) => sup.checkIn(params.id));
  r.post('/api/computers/:id/live/:op', ({ sup, params }) => {
    if (!['stop', 'takeover', 'handback'].includes(params.op)) err('Unknown action');
    return controlLive(sup, params.id, params.op as 'stop' | 'takeover' | 'handback');
  });
  r.del('/api/computers/:id', ({ sup, params }) => { sup.unlinkComputer(params.id); return sup.appState(); });
}
