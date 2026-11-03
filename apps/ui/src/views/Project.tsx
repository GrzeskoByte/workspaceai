// Project page: chats, rolled-up artifacts, and inherited instructions (R-09, R-12, R-13).
import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { useStore, setState, go, act, refreshSnap, wsId, toast, openRun } from '../store.ts';
import { descendants } from '../../../../packages/shared/src/util.ts';
import type { GraphRun } from '../../../../packages/shared/src/types.ts';
import { get, put, post, patch } from '../api.ts';
import { chatsWithin, projectPathName, rolledUpArtifacts } from '../../../../packages/shared/src/util.ts';
import { openArtifact, openChat } from '../shell/Shell.tsx';
import { Switch } from '../ui/Switch.tsx';

const TABS = ['chats', 'artifacts', 'graphs', 'map', 'instructions'] as const;
const MapView = lazy(() => import('./MapView.tsx').then((m) => ({ default: m.MapView })));

export function ProjectView() {
  const snap = useStore((s) => s.snap)!;
  const projId = useStore((s) => s.projId);
  const ptab = useStore((s) => s.ptab);
  const proj = snap.projects.find((p) => p.id === projId) || snap.projects[0];
  const [layers, setLayers] = useState<{ name: string; file: string; text: string; kind: string }[]>([]);
  const [own, setOwn] = useState('');
  useEffect(() => {
    if (!proj || ptab !== 'instructions') return;
    get<{ layers: typeof layers }>(`/api/ws/${wsId()}/projects/${proj.id}/layers`).then((r) => setLayers(r.layers)).catch(() => {});
    get<{ text: string }>(`/api/ws/${wsId()}/projects/${proj.id}/agents`).then((r) => setOwn(r.text)).catch(() => {});
  }, [proj?.id, ptab]);
  const runs = useStore((s) => s.runs);
  const fileIn = useRef<HTMLInputElement>(null);
  // .bpmn files added to the folder show up when the tab opens.
  useEffect(() => {
    if (!proj || ptab !== 'graphs') return;
    void post(`/api/ws/${wsId()}/graphs/scan`).then(() => refreshSnap()).catch(() => {});
    void get<{ runs: GraphRun[] }>(`/api/ws/${wsId()}/runs`).then((r) => setState({ runs: r.runs })).catch(() => {});
  }, [proj?.id, ptab]);
  if (!proj) return null;
  const inProj = new Set([proj.id, ...descendants(snap.projects, proj.id)]);
  const graphs = snap.graphs.filter((g) => inProj.has(g.project));
  const addFile = async (f: File) => {
    const xml = await f.text();
    if (!/<(bpmn:)?definitions[\s>]/.test(xml)) { toast(f.name + ' is not a BPMN file', 'error'); return; }
    const name = /<(?:bpmn:)?process\b[^>]*\bname="([^"]+)"/.exec(xml)?.[1] || f.name.replace(/\.bpmn$/i, '').replace(/[-_]+/g, ' ');
    await act(() => post(`/api/ws/${wsId()}/graphs`, { project: proj.id, name, xml }).then(() => refreshSnap()), `Added ${name} to ${proj.name}`);
  };
  const run = (gid: string, dryRun: boolean) => void act(() => post<GraphRun>(`/api/ws/${wsId()}/graphs/${gid}/run`, { dryRun }).then((r) => { setState((s) => ({ runs: [r, ...s.runs.filter((x) => x.id !== r.id)] })); openRun(r.id); }), dryRun ? 'Dry run started' : 'Run started');
  const chats = chatsWithin(snap.projects, snap.chats, proj.id);
  const arts = rolledUpArtifacts(snap.projects, snap.artifacts, proj.id);
  return (
    <div className="page-pad" style={{ flex: '1 1 auto', overflowY: 'auto', padding: 28, display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div>
        <span className="muted" style={{ fontSize: 12 }}>{projectPathName(snap.projects, proj.id)}</span>
        <h1 className="disp" style={{ margin: '4px 0 0', fontSize: 32 }}>{proj.name}</h1>
      </div>
      <div className="row" role="tablist">
        {TABS.map((t) => <button key={t} type="button" className="tab" role="tab" aria-selected={ptab === t} onClick={() => setState({ ptab: t })}>{t[0].toUpperCase() + t.slice(1)}</button>)}
        <span className="grow" />
        {!proj.isGeneral && <button type="button" className="btn btn-d" onClick={() => setState({ confirm: { title: 'Archive ' + proj.name + '?', body: 'Chats in this project must be moved or they are archived with it.', ok: 'Archive', run: async () => { await post(`/api/ws/${wsId()}/projects/${proj.id}/archive`); await refreshSnap(); go('home'); } } })}>Archive</button>}
      </div>
      {ptab === 'chats' && chats.map((c) => <button key={c.id} type="button" className="navrow" onClick={() => openChat(c)}><span className="ellipsis grow">{c.title}</span><span className="muted mono" style={{ fontSize: 11 }}>{c.sid}</span></button>)}
      {ptab === 'chats' && !chats.length && <p className="muted">No chats in this project yet.</p>}
      {ptab === 'artifacts' && arts.map((a) => <button key={a.id} type="button" className="navrow" onClick={() => openArtifact(a)}><span className="ellipsis grow">{a.title}</span><span className="muted">{a.type} v{a.version} · {projectPathName(snap.projects, a.project)}</span></button>)}
      {ptab === 'artifacts' && !arts.length && <p className="muted">Artifacts made in this project, and in its sub-projects, show up here.</p>}
      {ptab === 'map' && <div className="card" style={{ flex: '1 1 auto', minHeight: 420, display: 'flex', overflow: 'hidden', padding: 0 }}><Suspense fallback={<span className="muted pulse" style={{ margin: 'auto' }}>Loading the map…</span>}><MapView scope={proj.id} /></Suspense></div>}
      {ptab === 'graphs' && <>
        <div className="row" style={{ flexWrap: 'wrap', gap: 8 }}>
          <span className="muted grow" style={{ fontSize: 12.5 }}>Graphs are .bpmn files in this project’s folder — add one here or drop it into the folder. Run them here, from a chat with <span className="mono">/graph</span>, or ask the agent.</span>
          <input ref={fileIn} type="file" accept=".bpmn,application/xml,text/xml" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) void addFile(f); }} />
          <button type="button" className="btn" onClick={() => fileIn.current?.click()}>Add graph file…</button>
          <button type="button" className="btn btn-p" onClick={() => void act(() => post<{ id: string }>(`/api/ws/${wsId()}/graphs`, { project: proj.id, name: 'New graph' }).then(async (g) => { await refreshSnap(); go('graph', { graphId: g.id }); }))}>New graph</button>
        </div>
        {graphs.map((g) => { const last = runs.filter((r) => r.graph === g.id).sort((a, b) => b.startedAt - a.startedAt)[0]; return (
          <div key={g.id} className="card row" style={{ padding: '10px 14px', gap: 10, flexWrap: 'wrap' }}>
            <span className="grow" style={{ display: 'flex', flexDirection: 'column', minWidth: 160 }}>
              <b className="ellipsis" style={{ fontSize: 13.5 }}>{g.name}</b>
              <span className="muted mono ellipsis" style={{ fontSize: 11 }} title={g.file}>{g.file.startsWith(proj.folder) ? g.file.slice(proj.folder.length + 1) : g.file}{g.project !== proj.id ? ' · ' + projectPathName(snap.projects, g.project) : ''}</span>
            </span>
 {last && <button type="button" className="chip mono" onClick={() => openRun(last.id)} title="Show this run" style={{ fontSize: 11, color: last.status === 'failed' ? 'var(--error-text)' : last.status === 'completed' ? 'var(--success-text)' : 'var(--muted)' }}>{last.dryRun ? 'dry · ' : ''}{last.status}</button>}
            <button type="button" className="btn" onClick={() => go('graph', { graphId: g.id })}>Edit</button>
            <button type="button" className="btn" onClick={() => run(g.id, true)}>Dry run</button>
            <button type="button" className="btn btn-p" onClick={() => run(g.id, false)}>Run</button>
          </div>
        ); })}
        {!graphs.length && <p className="muted">No graphs in this project yet.</p>}
      </>}
      {ptab === 'instructions' && <>
        <label className="row">Inherit from parents <Switch on={proj.inherit !== false} label="Inherit instructions" onChange={() => void act(() => patch(`/api/ws/${wsId()}/projects/${proj.id}`, { inherit: proj.inherit === false }).then(() => refreshSnap()))} /></label>
        {layers.filter((l) => l.kind !== 'project').map((l) => <div key={l.file} className="card" style={{ padding: 12 }}><span className="lbl">{l.name}</span><pre className="pre" style={{ marginTop: 8 }}>{l.text || 'Empty'}</pre></div>)}
        <label className="field">This project's AGENTS.md<span className="muted" style={{ fontWeight: 400 }}>The narrowest layer. It wins when it disagrees with the ones above.</span>
          <textarea className="inp" style={{ height: 180, padding: 10 }} value={own} onChange={(e) => setOwn(e.target.value)} />
        </label>
        <button type="button" className="btn btn-p" onClick={() => void act(() => put(`/api/ws/${wsId()}/projects/${proj.id}/agents`, { text: own }), 'Instructions saved')}>Save AGENTS.md</button>
      </>}
    </div>
  );
}
