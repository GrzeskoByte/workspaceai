// Graphs inside sessions: agents list, read, save and run the project's BPMN graphs (files in the project folder)
// through the harness tools, and a run started from a chat shows in that chat and reports back to it.
import type { Supervisor } from './supervisor.ts';
import type { Router } from './server.ts';
import type { WorkspaceRuntime } from './runtime.ts';
import type { GraphMeta, GraphRun } from '../../shared/src/types.ts';
import { projectPathName } from '../../shared/src/util.ts';
import { lintBpmn } from '../../orchestrator/src/lint.ts';
import { withHarnessSteps, withLayout } from '../../orchestrator/src/layout.ts';
import { runSummary, signalGraphRun, startGraphRun } from './graphs.ts';

/** The steps a graph has, by name, so an agent can tell graphs apart without reading the XML. */
export function graphSteps(xml: string): string[] {
  const out: string[] = [];
  const re = /<(?:\w+:)?(\w*[Tt]ask|callActivity)\b([^>]*)>/g;
  for (let m = re.exec(xml); m; m = re.exec(xml)) {
    const name = /\bname="([^"]*)"/.exec(m[2])?.[1];
    if (!name) continue;
    const kind = /\bharness:type="([^"]+)"/.exec(m[2])?.[1] || (m[1] === 'userTask' ? 'waits for the user' : m[1].replace(/Task$/, '').toLowerCase() || 'task');
    out.push(`${name} (${kind})`);
  }
  return out.slice(0, 30);
}

function find(rt: WorkspaceRuntime, ref: string): GraphMeta {
  rt.scanGraphFiles();
  const g = rt.findGraph(ref);
  if (!g) throw Object.assign(new Error(`No graph “${ref}” — call graph_list to see this workspace's graphs`), { status: 404 });
  return g;
}

const storedRun = (rt: WorkspaceRuntime, id: string): GraphRun | undefined => rt.listStoredRuns().find((r) => r.run.id === id)?.run;

/** Wait until a run stops running (done, failed, or waiting for someone), up to `ms`. */
async function settle(rt: WorkspaceRuntime, id: string, ms: number): Promise<GraphRun | undefined> {
  const end = Date.now() + ms;
  for (;;) {
    const r = storedRun(rt, id);
    if (!r || r.status !== 'running' || Date.now() > end) return r;
    await new Promise((res) => setTimeout(res, 400));
  }
}

function report(rt: WorkspaceRuntime, run: GraphRun | undefined): { status: string; run: string; summary: string } {
  if (!run) return { status: 'unknown', run: '', summary: 'The run is gone.' };
  const name = rt.state.graphs.find((g) => g.id === run.graph)?.name || 'graph';
  return { status: run.status, run: run.id, summary: run.status === 'running' ? `Graph “${name}” is still running (${run.completed.length} steps done). Call graph_status with run ${run.id} later.` : runSummary(run, name) };
}

export function installGraphRoutes(sup: Supervisor, router: Router, callingChat: (rt: WorkspaceRuntime) => string | null): void {
  router.get('/api/ws/:id/agent/graphs', ({ params }) => {
    const rt = sup.rt(params.id);
    rt.scanGraphFiles();
    const chat = callingChat(rt);
    const pid = chat ? rt.chat(chat)?.project : undefined;
    const graphs = rt.state.graphs.map((g) => {
      let steps: string[] = [];
      try { steps = graphSteps(rt.readGraph(g.id).xml); } catch { /* unreadable file */ }
      return { id: g.id, name: g.name, project: projectPathName(rt.state.projects, g.project), file: g.file, here: g.project === pid, steps };
    });
    return { graphs: graphs.sort((a, b) => Number(b.here) - Number(a.here)) };
  });
  router.get('/api/ws/:id/agent/graphs/:ref', ({ params }) => {
    const rt = sup.rt(params.id);
    const g = find(rt, decodeURIComponent(params.ref));
    return { id: g.id, name: g.name, file: g.file, xml: rt.readGraph(g.id).xml };
  });
  // Create a graph in the calling chat's project, or save a new version of one; the model check must pass.
  router.post('/api/ws/:id/agent/graphs', async ({ params, body }) => {
    const rt = sup.rt(params.id);
    const xml = withHarnessSteps(withLayout(String(body.xml || '')));
    const checked = await lintBpmn(xml);
    const errors = checked.issues.filter((i) => i.category === 'error');
    if (!checked.ok || errors.length) throw Object.assign(new Error('The graph breaks model rules: ' + (errors.map((i) => `${i.id ? i.id + ': ' : ''}${i.message}`).join('; ') || 'invalid BPMN')), { status: 400 });
    if (body.graph) {
      const g = find(rt, String(body.graph));
      const meta = rt.saveGraph(g.id, { xml, ...(body.name ? { name: String(body.name) } : {}) });
      return { id: meta.id, name: meta.name, file: meta.file, created: false };
    }
    const chat = callingChat(rt);
    const project = (chat && rt.chat(chat)?.project) || rt.ws.defaultProject || rt.general().id;
    const name = String(body.name || /<(?:bpmn:)?process\b[^>]*\bname="([^"]+)"/.exec(xml)?.[1] || 'New graph');
    const meta = rt.createGraph({ name, project, xml, chat });
    return { id: meta.id, name: meta.name, file: meta.file, created: true };
  });
  // Run a graph for the calling chat; by default wait for it, so the agent gets the results as the tool's answer.
  router.post('/api/ws/:id/agent/graphs/:ref/run', async ({ params, body }) => {
    const rt = sup.rt(params.id);
    const g = find(rt, decodeURIComponent(params.ref));
    const run = await startGraphRun(rt, g.id, !!body.dryRun, { from: callingChat(rt), tellAgent: false });
    if (body.wait === false) return report(rt, run);
    return report(rt, await settle(rt, run.id, Math.min(Number(body.timeout || 280), 1800) * 1000));
  });
  router.get('/api/ws/:id/agent/runs/:rid', async ({ params, query }) => {
    const rt = sup.rt(params.id);
    return report(rt, await settle(rt, params.rid, Math.min(Number(query.get('wait') || 0), 280) * 1000));
  });
  router.post('/api/ws/:id/agent/runs/:rid/continue', ({ params, body }) => {
    const rt = sup.rt(params.id);
    const run = storedRun(rt, params.rid);
    if (!run?.waiting) throw Object.assign(new Error('That run is not waiting'), { status: 409 });
    return report(rt, signalGraphRun(rt, params.rid, String(body.element || run.waiting.element)));
  });
}
