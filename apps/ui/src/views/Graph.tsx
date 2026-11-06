// BPMN graph editor on bpmn-js (R-24, R-25, R-26, R-37): standard BPMN palette plus the harness element set,
// a harness inspector, model check, dry run and run, BPMN XML import/export. The bpmn.io logo stays visible.
import { useEffect, useRef, useState } from 'react';
import BpmnModeler from 'bpmn-js/lib/Modeler';
import 'bpmn-js/dist/assets/diagram-js.css';
import 'bpmn-js/dist/assets/bpmn-js.css';
import 'bpmn-js/dist/assets/bpmn-font/css/bpmn-embedded.css';
import { useStore, act, refreshSnap, wsId, toast, setState } from '../store.ts';
import { get, post, put, del } from '../api.ts';
import { HARNESS_MODDLE, PATTERNS, HARNESS_TYPES } from '../../../../packages/orchestrator/src/model.ts';
import { flatProjects } from '../../../../packages/shared/src/util.ts';
import type { GraphMeta, GraphRun } from '../../../../packages/shared/src/types.ts';
import { harnessPaletteModule } from './graphPalette.ts';
import { paintRun } from './runPaint.ts';
import { Approvals } from './Approvals.tsx';
import { openChat } from '../shell/Shell.tsx';
import { Icon } from '../ui/Icon.tsx';

type Any = any;
interface Sel { id: string; type: string; name: string; attrs: Record<string, string> }

const TYPE_HELP: Record<string, string> = {
  agent: 'One opencode prompt in this run’s session; earlier step results are passed along.',
  browser: 'Opens the URL with Playwright MCP and returns what it read.',
  script: 'Runs the command with sh in the project folder.',
  service: 'Runs as an agent turn that calls the named MCP tool.',
  rule: 'Judges the work so far and returns {verdict, score, reason}.',
};

export function GraphView() {
  const snap = useStore((s) => s.snap)!;
  const runs = useStore((s) => s.runs);
  const wanted = useStore((s) => s.graphId);
  const [gid, setGid] = useState<string | null>((wanted && snap.graphs.some((g) => g.id === wanted) ? wanted : null) || snap.graphs[0]?.id || null);
  useEffect(() => { if (wanted && wanted !== gid && snap.graphs.some((g) => g.id === wanted)) setGid(wanted); }, [wanted]);
  // The sidebar highlights the graph open here.
  useEffect(() => { if (gid && gid !== wanted) setState({ graphId: gid }); }, [gid]);
  const [issues, setIssues] = useState<{ message: string; category: string; id?: string }[]>([]);
  const [name, setName] = useState('');
  const [sel, setSel] = useState<Sel | null>(null);
  const [dirty, setDirty] = useState(false);
  const [imported, setImported] = useState(0);
  const host = useRef<HTMLDivElement>(null);
  const modeler = useRef<Any>(null);
  const file = useRef<HTMLInputElement>(null);
  const meta = snap.graphs.find((g) => g.id === gid) || null;
  const graphRuns = runs.filter((r) => r.graph === gid).slice().sort((a, b) => b.startedAt - a.startedAt);
  const canvasOpen = !!meta;

  useEffect(() => {
    if (!canvasOpen || !host.current) return;
    const m: Any = new (BpmnModeler as Any)({ container: host.current, moddleExtensions: { harness: HARNESS_MODDLE }, additionalModules: [harnessPaletteModule] });
    modeler.current = m;
    const read = (el: Any): Sel | null => {
      if (!el || el.type === 'bpmn:Process' || el.type === 'label') return null;
      const bo = el.businessObject;
      const attrs: Record<string, string> = {};
      for (const [k, v] of Object.entries(bo.$attrs || {})) if (k.startsWith('harness:')) attrs[k.slice(8)] = String(v);
      for (const k of ['type', 'agent', 'model', 'prompt', 'url', 'command', 'askFirst', 'maxLoops', 'tools', 'runner', 'mcp']) { const v = bo.get?.('harness:' + k); if (v != null) attrs[k] = String(v); }
      return { id: el.id, type: el.type, name: bo.name || '', attrs };
    };
    m.on('selection.changed', (e: Any) => setSel(read(e.newSelection?.[0])));
    m.on('element.changed', (e: Any) => { setDirty(true); setSel((cur) => (cur && cur.id === e.element.id ? read(e.element) : cur)); });
    return () => { m.destroy(); modeler.current = null; };
  }, [canvasOpen]);

  useEffect(() => {
    if (!gid || !modeler.current) return;
    let stop = false;
    get<{ meta: GraphMeta; xml: string }>(`/api/ws/${wsId()}/graphs/${gid}`).then(async (r) => {
      if (stop || !modeler.current) return;
      setName(r.meta.name);
      await modeler.current.importXML(r.xml);
      modeler.current.get('canvas').zoom('fit-viewport', 'auto');
      setDirty(false); setSel(null); setIssues([]); setImported((n) => n + 1);
    }).catch((e) => toast(e.message, 'error'));
    get<{ runs: GraphRun[] }>(`/api/ws/${wsId()}/runs`).then((r) => { if (!stop) setState({ runs: r.runs }); }).catch(() => {});
    return () => { stop = true; };
  }, [gid, canvasOpen]);

  // The latest run, drawn on the canvas while it runs and after; editing the graph clears it.
  const latest = graphRuns[0];
  useEffect(() => { if (imported) paintRun(modeler.current, dirty ? undefined : latest); }, [imported, latest, dirty]);

  const xmlOf = async (): Promise<string> => (await modeler.current?.saveXML({ format: true }))?.xml || '';
  const save = async (quiet = false) => {
    if (!gid) return false;
    const xml = await xmlOf();
    // `act` gives undefined on failure, so the save itself must answer true (refreshSnap answers nothing).
    const ok = await act(() => put(`/api/ws/${wsId()}/graphs/${gid}`, { name, xml }).then(() => refreshSnap()).then(() => true), quiet ? undefined : 'Graph saved');
    if (ok) setDirty(false);
    return !!ok;
  };
  const check = async (quiet = false, action = 'save or run this graph') => {
    if (!gid) return false;
    const xml = await xmlOf();
    const r = await post<{ ok: boolean; issues: { message: string; category: string; id?: string }[] }>(`/api/ws/${wsId()}/graphs/${gid}/lint`, { xml });
    setIssues(r.issues);
    const errors = r.issues.filter((i) => i.category === 'error');
    if (!quiet || !r.ok) toast(r.ok ? 'Model check passed' : `Can’t ${action}: ${errors[0]?.message || 'the model check found a problem'}${errors.length > 1 ? ` (+${errors.length - 1} more, listed below)` : ''}`, r.ok ? undefined : 'error');
    return r.ok;
  };
  const create = async (pattern?: string) => {
    const g = await act(() => post<GraphMeta>(`/api/ws/${wsId()}/graphs`, { project: snap.workspace.defaultProject, pattern, name: PATTERNS.find((p) => p.id === pattern)?.name }).then(async (x) => { await refreshSnap(); return x; }), 'Graph created');
    if (g) setGid(g.id);
  };
  // A run always uses the model as checked and saved (R-25: the model check blocks running).
  const run = async (dryRun: boolean) => {
    if (!gid) return;
    const what = dryRun ? 'dry-run this graph' : 'run this graph';
    if (!(await check(true, what))) return;
    if (!(await save(true))) { toast(`Can’t ${what}: saving it failed`, 'error'); return; }
    const r = await act(() => post<GraphRun>(`/api/ws/${wsId()}/graphs/${gid}/run`, { dryRun }), dryRun ? 'Dry run started — nothing is called' : 'Run started');
    if (r) setState((s) => ({ runs: [r, ...s.runs.filter((x) => x.id !== r.id)] }));
  };
  const exportXml = async () => {
    const xml = await xmlOf();
    const url = URL.createObjectURL(new Blob([xml], { type: 'application/xml' }));
    const a = document.createElement('a'); a.href = url; a.download = (name || 'graph').replace(/[^\w-]+/g, '-').toLowerCase() + '.bpmn'; a.click(); URL.revokeObjectURL(url);
    toast('Exported BPMN 2.0 XML — it opens in Camunda Modeler or bpmn.io');
  };
  const importXml = async (f: File) => {
    const xml = await f.text();
    try {
      const r = await modeler.current.importXML(xml);
      modeler.current.get('canvas').zoom('fit-viewport', 'auto');
      setDirty(true);
      toast(r.warnings?.length ? `Imported with ${r.warnings.length} warning(s); unsupported elements block running until replaced` : 'Imported — save to keep it');
    } catch (e) { toast('Not a BPMN 2.0 file: ' + (e as Error).message, 'error'); }
  };
  const setAttr = (k: string, v: string) => {
    if (!sel || !modeler.current) return;
    const el = modeler.current.get('elementRegistry').get(sel.id);
    modeler.current.get('modeling').updateProperties(el, k === 'name' ? { name: v } : { ['harness:' + k]: v || undefined });
  };
  const isTask = !!sel && /Task$/.test(sel.type);

  return (
    <div className="split" style={{ flex: '1 1 auto', display: 'flex', minHeight: 0, minWidth: 0 }}>
      <aside aria-label="Graphs" className="side-col" style={{ flex: '0 0 220px', borderRight: '1px solid var(--bg2)', padding: 12, display: 'flex', flexDirection: 'column', gap: 6, overflowY: 'auto' }}>
        <button type="button" className="btn btn-p" onClick={() => void create()}>New graph</button>
        <span className="lbl" style={{ paddingTop: 6 }}>Agent patterns</span>
        {PATTERNS.map((p) => <button key={p.id} type="button" className="navrow" onClick={() => void create(p.id)} title={p.blurb}>{p.name}</button>)}
        <span className="lbl" style={{ paddingTop: 6 }}>In this workspace</span>
        {snap.graphs.map((g) => <button key={g.id} type="button" className="navrow" aria-current={g.id === gid ? 'page' : 'false'} onClick={() => setGid(g.id)}>{g.name}</button>)}
        {!snap.graphs.length && <span className="muted" style={{ fontSize: 12 }}>No graphs yet.</span>}
      </aside>
      <div style={{ flex: '1 1 auto', display: 'flex', flexDirection: 'column', minWidth: 0, padding: 12, gap: 8 }}>
        {!meta && <div className="card" style={{ padding: 30, textAlign: 'center' }}><p className="muted" style={{ margin: 0 }}>Create a graph or load a pattern. Drag tasks, gateways and events from the palette — no XML to type.</p></div>}
        {meta && <>
          <div className="row" style={{ flexWrap: 'wrap' }}>
            <input className="inp" value={name} onChange={(e) => { setName(e.target.value); setDirty(true); }} aria-label="Graph name" style={{ width: 220, maxWidth: '100%' }} />
            <select className="inp" value={meta.project} aria-label="Project" onChange={(e) => void act(() => put(`/api/ws/${wsId()}/graphs/${meta.id}`, { project: e.target.value }).then(() => refreshSnap()), 'Moved to another project')}>{flatProjects(snap.projects).map(({ project, depth }) => <option key={project.id} value={project.id}>{' '.repeat(depth * 2)}{project.name}</option>)}</select>
            <button type="button" className="btn" onClick={() => void save()}>{dirty ? 'Save •' : 'Save'}</button>
            <button type="button" className="btn" onClick={() => void check()}>Check model</button>
            <button type="button" className="btn" onClick={() => void run(true)}>Dry run</button>
            <button type="button" className="btn btn-p" onClick={() => void run(false)}>Run</button>
            <span className="grow" />
            <button type="button" className="btn" onClick={() => file.current?.click()}>Import .bpmn</button>
            <input ref={file} type="file" accept=".bpmn,.xml" hidden onChange={(e) => { const f = e.target.files?.[0]; if (f) void importXml(f); e.target.value = ''; }} />
            <button type="button" className="btn" onClick={() => void exportXml()}>Export .bpmn</button>
            <button type="button" className="btn btn-d" onClick={() => setState({ confirm: { title: 'Delete ' + meta.name + '?', body: 'The BPMN file is removed from this project.', ok: 'Delete', run: async () => { await del(`/api/ws/${wsId()}/graphs/${meta.id}`); setGid(null); await refreshSnap(); } } })}>Delete</button>
          </div>
          <div className="row graph-body" style={{ alignItems: 'stretch', flex: '1 1 auto', minHeight: 0 }}>
            <div ref={host} className="graph-canvas" style={{ flex: '1 1 auto', minWidth: 0 }} />
            <aside className="card graph-insp" aria-label="Inspector" style={{ flex: '0 0 270px', padding: 12, display: 'flex', flexDirection: 'column', gap: 8, overflowY: 'auto' }}>
              <span className="lbl">Inspector</span>
              {!sel && <span className="muted" style={{ fontSize: 12 }}>Select an element. The WorkspaceAI group in the palette adds agent tasks, Browser steps, scripts and business rules.</span>}
              {sel && <>
                <span className="mono muted" style={{ fontSize: 11 }}>{sel.type.replace('bpmn:', '')} · {sel.id}</span>
                <label className="field">Name<input className="inp" value={sel.name} onChange={(e) => setAttr('name', e.target.value)} /></label>
                {isTask && sel.type !== 'bpmn:UserTask' && <>
                  <label className="field">WorkspaceAI step<select className="inp" value={sel.attrs.type || 'agent'} onChange={(e) => setAttr('type', e.target.value)}>{HARNESS_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}</select></label>
                  <span className="muted" style={{ fontSize: 11.5 }}>{TYPE_HELP[sel.attrs.type || 'agent']}</span>
                  {(sel.attrs.type || 'agent') !== 'script' && (sel.attrs.type || 'agent') !== 'browser' && <label className="field">Prompt<textarea className="inp" rows={4} value={sel.attrs.prompt || ''} onChange={(e) => setAttr('prompt', e.target.value)} placeholder={sel.name} /></label>}
                  {(sel.attrs.type || 'agent') === 'browser' && <label className="field">Page URL<input className="inp mono" value={sel.attrs.url || ''} onChange={(e) => setAttr('url', e.target.value)} /></label>}
                  {(sel.attrs.type || 'agent') === 'script' && <label className="field">Command<input className="inp mono" value={sel.attrs.command || ''} onChange={(e) => setAttr('command', e.target.value)} placeholder="pnpm test" /></label>}
                  {['agent', 'rule', 'service'].includes(sel.attrs.type || 'agent') && <>
                    <label className="field">Model<input className="inp mono" value={sel.attrs.model || ''} onChange={(e) => setAttr('model', e.target.value)} placeholder="inherit from project" /></label>
                    <label className="field">Agent<select className="inp" value={sel.attrs.agent || 'opencode'} onChange={(e) => setAttr('agent', e.target.value)}>{snap.workspace.agents.filter((a) => a.on).map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}</select></label>
                  </>}
                  {(sel.attrs.type || 'agent') === 'service' && <label className="field">MCP tool<input className="inp mono" value={sel.attrs.mcp || ''} onChange={(e) => setAttr('mcp', e.target.value)} placeholder="server.tool" /></label>}
                  <label className="row" style={{ justifyContent: 'space-between', fontSize: 13 }}>Ask first<input type="checkbox" checked={sel.attrs.askFirst === 'true'} onChange={(e) => setAttr('askFirst', e.target.checked ? 'true' : '')} /></label>
                  <label className="field">Loop maximum<input className="inp" type="number" min={1} value={sel.attrs.maxLoops || ''} onChange={(e) => setAttr('maxLoops', e.target.value)} placeholder="required on a cycle" /></label>
                </>}
                {sel.type === 'bpmn:UserTask' && <span className="muted" style={{ fontSize: 12 }}>Waits for a person: the run pauses here until someone continues it in Routines or the graph view.</span>}
                {/Gateway$/.test(sel.type) && <span className="muted" style={{ fontSize: 12 }}>Put a condition on each outgoing flow, or mark one as default. The model check enforces it.</span>}
              </>}
              {!!issues.length && <><span className="lbl" style={{ paddingTop: 6 }}>Model check</span>{issues.map((i, n) => <button key={n} type="button" className="link" style={{ textAlign: 'left', fontSize: 12, color: i.category === 'warn' ? 'var(--warning-text)' : 'var(--error-text)' }} onClick={() => { if (i.id && modeler.current) { const el = modeler.current.get('elementRegistry').get(i.id); if (el) modeler.current.get('selection').select(el); } }}><Icon name={i.category === 'warn' ? 'alert' : 'x'} size={13} style={{ verticalAlign: -2, color: i.category === 'warn' ? 'var(--warning-text)' : 'var(--error-text)' }} /> {i.message}</button>)}</>}
            </aside>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, maxHeight: 260, overflowY: 'auto' }}>{graphRuns.slice(0, 5).map((r) => <RunRow key={r.id} run={r} />)}</div>
        </>}
      </div>
    </div>
  );
}

function RunRow({ run }: { run: GraphRun }) {
  const [note, setNote] = useState('');
  const snap = useStore((s) => s.snap)!;
  const base = `/api/ws/${wsId()}/runs/${run.id}`;
  const chat = run.chat ? snap.chats.find((c) => c.id === run.chat) : undefined;
  const color = run.status === 'completed' ? 'var(--success)' : run.status === 'failed' ? 'var(--error)' : run.status === 'waiting' || run.status === 'paused' ? 'var(--warning)' : 'var(--info)';
  return (
    <div className="card rise" style={{ padding: 10, display: 'flex', flexDirection: 'column', gap: 6 }}>
      <div className="row"><span className={'dot ' + (run.status === 'running' ? 'pulse' : '')} style={{ background: color }} /><b>{run.status}</b><span className="muted" style={{ fontSize: 12 }}>{run.dryRun ? 'dry run · nothing called' : 'live'} · {new Date(run.startedAt).toLocaleTimeString()}</span><span className="grow" />
        {chat && <button type="button" className="link" style={{ fontSize: 12 }} onClick={() => openChat(chat)}><Icon name="chat" size={12} style={{ verticalAlign: -2 }} /> Transcript</button>}
        {(run.status === 'running' || run.status === 'waiting') && <button type="button" className="btn" onClick={() => void act(() => post(`${base}/pause`), 'Paused · state saved')}>Pause</button>}
        {run.status === 'paused' && <button type="button" className="btn" onClick={() => void act(() => post(`${base}/resume`), 'Resumed')}>Resume</button>}
      </div>
      {!!run.chats?.length && (run.status === 'running' || run.status === 'paused') && <Approvals chatIds={run.chats} />}
      {run.waiting && <form className="row" onSubmit={(e) => { e.preventDefault(); void act(() => post(`${base}/signal`, { element: run.waiting?.element, note }), 'Continued'); }}>
        <span className="grow">Waiting for you · {run.waiting.prompt}</span>
        <input className="inp" value={note} onChange={(e) => setNote(e.target.value)} placeholder="Note" aria-label="Reply to the waiting step" />
        <button type="submit" className="btn btn-p">Continue</button>
      </form>}
      <pre className="pre" style={{ maxHeight: 120 }}>{run.log.slice(-8).map((l) => l.text).join('\n')}</pre>
    </div>
  );
}
