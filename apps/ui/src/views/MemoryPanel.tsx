// Shared memory panel: facts plus the Graphify graph, used by Memory and by Settings › Memory (R-22, R-23).
import { useEffect, useState } from 'react';
import { useStore, act, refreshSnap, wsId, toast } from '../store.ts';
import { get, post, patch, del } from '../api.ts';
import { flatProjects } from '../../../../packages/shared/src/util.ts';
import type { GraphifyStatus, MemoryBackend, MemoryGraph, MemoryGraphNode } from '../../../../packages/shared/src/types.ts';
import { Switch } from '../ui/Switch.tsx';
import { MemoryGraphView } from './MemoryGraphView.tsx';

const BACKENDS: [MemoryBackend, string][] = [['facts', 'Facts'], ['graph', 'Graphify'], ['both', 'Graphify + facts'], ['mcp', 'Custom memory MCP']];

export function MemoryPanel() {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const [text, setText] = useState('');
  const [scope, setScope] = useState('ws');
  const [folder, setFolder] = useState('');
  const [status, setStatus] = useState<GraphifyStatus | null>(null);
  const [graph, setGraph] = useState<MemoryGraph | null>(null);
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState('');
  const [matches, setMatches] = useState<MemoryGraphNode[]>([]);
  const graphOn = ws.memory === 'graph' || ws.memory === 'both';
  const folderKey = ws.graphifyFolders.join('\n');
  const suggested = snap.facts.filter((f) => f.status === 'suggested');
  const approved = snap.facts.filter((f) => f.status === 'approved');

  const saveBackend = (memory: MemoryBackend) => void act(() => patch(`/api/ws/${wsId()}`, { memory }).then(() => refreshSnap()), 'Memory backend saved');

  const loadStatus = () => get<GraphifyStatus>(`/api/ws/${wsId()}/graphify/status`).then(setStatus).catch(() => setStatus(null));

  const loadGraph = () => {
    if (!graphOn) { setGraph(null); return; }
    setBusy(true);
    get<MemoryGraph>(`/api/ws/${wsId()}/graphify/graph`)
      .then((g) => { setGraph(g); return loadStatus(); })
      .catch((e) => toast((e as Error).message || String(e), 'error'))
      .finally(() => setBusy(false));
  };

  useEffect(() => { void loadGraph(); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [graphOn, folderKey, ws.graphifyWatch]);

  const rebuild = () => void act(async () => {
    setBusy(true);
    try {
      const g = await post<MemoryGraph>(`/api/ws/${wsId()}/graphify/build`);
      setGraph(g);
      await loadStatus();
      return g;
    } finally { setBusy(false); }
  }, (g) => `Graph built · ${g.nodes.length} nodes · ${g.edges.length} edges`);

  const install = () => void act(async () => {
    setBusy(true);
    try {
      const r = await post<{ ok: boolean; output: string }>('/api/graphify/install');
      await loadStatus();
      if (!r.ok) throw new Error(r.output);
      return r;
    } finally { setBusy(false); }
  }, 'Graphify installed');

  const runQuery = async () => {
    if (!query.trim()) { setMatches([]); return; }
    const r = await act(() => get<{ matches: MemoryGraphNode[] }>(`/api/ws/${wsId()}/graphify/query?q=${encodeURIComponent(query.trim())}`));
    if (r) setMatches(r.matches);
  };

  const review = (id: string, decision: 'kept' | 'dropped') => void act(async () => {
    await post(`/api/ws/${wsId()}/graphify/review/${encodeURIComponent(id)}`, { status: decision });
    const g = await post<MemoryGraph>(`/api/ws/${wsId()}/graphify/build`);
    setGraph(g);
    await loadStatus();
  }, decision === 'kept' ? 'Link kept' : 'Link dropped');

  const addFolder = (f: string) => { if (!f.trim()) return; void act(() => patch(`/api/ws/${wsId()}`, { graphifyFolders: [...ws.graphifyFolders, f.trim()] }).then(() => refreshSnap())); };
  const removeFolder = (f: string) => void act(() => patch(`/api/ws/${wsId()}`, { graphifyFolders: ws.graphifyFolders.filter((x) => x !== f) }).then(() => refreshSnap()));

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <div className="row" style={{ flexWrap: 'wrap' }}>
        {BACKENDS.map(([id, label]) => <button key={id} type="button" className="chip" aria-pressed={ws.memory === id} disabled={ws.kind === 'org' && id !== 'facts' && !!snap.policy} onClick={() => saveBackend(id)}>{label}</button>)}
      </div>
      <div className="card row" style={{ padding: '12px 16px', gap: 14 }}>
        <span className="grow" style={{ display: 'flex', flexDirection: 'column' }}>
          <span style={{ fontSize: 13, fontWeight: 500 }}>Use memory in every chat</span>
          <span className="muted" style={{ fontSize: 12 }}>Every session, whatever the agent, starts with what memory holds ({ws.memory === 'facts' ? 'approved facts' : ws.memory === 'graph' ? 'the memory graph' : ws.memory === 'mcp' ? 'your memory server' : 'the memory graph and approved facts'}), is told to check it before exploring or answering, and gets the memory tools. Off: agents don’t see memory unless you share it.</span>
        </span>
        <Switch on={ws.useMemory !== false} label="Use memory in every chat" onChange={() => void act(() => patch(`/api/ws/${wsId()}`, { useMemory: ws.useMemory === false }).then(() => refreshSnap()), ws.useMemory === false ? 'Every chat uses memory' : 'Memory is off for chats')} />
      </div>
      {ws.memory !== 'mcp' && ws.useMemory !== false && (
        <div className="card row" style={{ padding: '12px 16px', gap: 14 }}>
          <span className="grow" style={{ display: 'flex', flexDirection: 'column' }}>
            <span style={{ fontSize: 13, fontWeight: 500 }}>Recall on every message</span>
            <span className="muted" style={{ fontSize: 12 }}>Each message is also looked up in {ws.memory === 'facts' ? 'approved facts' : ws.memory === 'graph' ? 'the memory graph' : 'the memory graph and approved facts'} and the matches go to the agent with it.</span>
          </span>
          <Switch on={ws.memoryRecall !== false} label="Recall on every message" onChange={() => void act(() => patch(`/api/ws/${wsId()}`, { memoryRecall: ws.memoryRecall === false }).then(() => refreshSnap()), ws.memoryRecall === false ? 'Memory is recalled on every message' : 'Agents use memory through their tools only')} />
        </div>
      )}
      {ws.memory === 'mcp' && <label className="field">Memory MCP command<input className="inp" defaultValue={ws.memoryMcpCommand || ''} onBlur={(e) => void act(() => patch(`/api/ws/${wsId()}`, { memoryMcpCommand: e.target.value }).then(() => refreshSnap()))} /></label>}

      {graphOn && <div className="card" style={{ padding: 14, display: 'flex', flexDirection: 'column', gap: 12 }}>
        <div className="row" style={{ flexWrap: 'wrap' }}>
          <span className="lbl grow">Graphify memory</span>
          {status && <>
            <span className="chip" title={status.graphFile || ''} style={{ height: 24, fontSize: 11 }}>{status.installed ? 'Graphify' + (status.version ? ' ' + status.version : '') : 'Graphify not installed'}</span>
            <span className="chip" style={{ height: 24, fontSize: 11 }}>{status.nodes} nodes · {status.edges} edges</span>
            <span className="chip" style={{ height: 24, fontSize: 11 }}>{status.pendingLinks} to review</span>
          </>}
          {status && !status.installed && <button type="button" className="btn" onClick={install} disabled={busy}>Install Graphify</button>}
          <button type="button" className="btn" onClick={rebuild} disabled={busy}>{busy ? 'Building…' : graph ? 'Rebuild' : 'Build'}</button>
        </div>
        <p className="muted" style={{ margin: 0, fontSize: 13 }}>
          {status?.installed
            ? 'Graphify is installed. Build runs graphify extract on each folder with only this workspace’s allowed model (code-only when none fits); watching runs graphify update on change.'
            : 'Graphify is not installed on this computer, so harness builds its own folder and file index. Install Graphify to get its code-and-docs graph on this same screen.'}
          {' '}Nothing from another workspace is included.
        </p>
        {status?.graphFile && <span className="mono muted ellipsis" style={{ fontSize: 11 }}>{status.graphFile}</span>}

        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <span className="lbl">Folders to map</span>
          {ws.graphifyFolders.map((f) => <div key={f} className="row"><span className="mono grow" style={{ fontSize: 12 }}>{f}</span><button type="button" className="link" onClick={() => removeFolder(f)}>Remove</button></div>)}
          {!ws.graphifyFolders.length && <span className="muted" style={{ fontSize: 13 }}>No folders yet. Add a project folder, notes folder, or the workspace folder.</span>}
          <form className="row" onSubmit={(e) => { e.preventDefault(); addFolder(folder); setFolder(''); }}>
            <input className="inp grow" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="~/code/castform" aria-label="Folder to map" />
            <button type="submit" className="btn">Add folder</button>
          </form>
          <label className="row">Watch for changes <Switch on={ws.graphifyWatch} label="Watch Graphify folders" onChange={() => void act(() => patch(`/api/ws/${wsId()}`, { graphifyWatch: !ws.graphifyWatch }).then(() => refreshSnap()))} /></label>
        </div>

        {graph && <MemoryGraphView graph={graph} highlight={matches.map((m) => m.id)} />}

        <form className="row" onSubmit={(e) => { e.preventDefault(); void runQuery(); }}>
          <input className="inp grow" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Ask the graph: solver, build script, auth…" aria-label="Query the memory graph" />
          <button type="submit" className="btn">Query</button>
          {!!matches.length && <button type="button" className="link" onClick={() => { setMatches([]); setQuery(''); }}>Clear</button>}
        </form>
        {!!matches.length && <div className="row" style={{ flexWrap: 'wrap' }}>{matches.map((m) => <span key={m.id} className="chip" style={{ height: 24, fontSize: 11 }}>{m.label}</span>)}</div>}
        {query.trim() && !matches.length && <span className="muted" style={{ fontSize: 12 }}>No node matches that yet. Press Build if the folders changed.</span>}

        {!!graph?.ambiguous.filter((a) => a.status === 'pending').length && <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <span className="lbl">Review queue · ambiguous links</span>
          {graph.ambiguous.filter((a) => a.status === 'pending').map((a) => (
            <div key={a.id} className="row card" style={{ padding: '8px 10px' }}>
              <span className="grow" style={{ fontSize: 13 }}>{a.reason}</span>
              <button type="button" className="btn" onClick={() => review(a.id, 'kept')}>Keep</button>
              <button type="button" className="btn btn-d" onClick={() => review(a.id, 'dropped')}>Drop</button>
            </div>
          ))}
        </div>}
        {graph?.ambiguous.length ? <span className="muted" style={{ fontSize: 12 }}>{graph.ambiguous.filter((a) => a.status !== 'pending').length} reviewed links stay out of the queue.</span> : null}
      </div>}

      <form className="row" onSubmit={(e) => { e.preventDefault(); const t = text.trim(); if (!t) return; void act(() => post(`/api/ws/${wsId()}/facts`, { text: t, scope, status: 'approved' }).then(() => refreshSnap()), 'Remembered'); setText(''); }}>
        <input className="inp grow" value={text} onChange={(e) => setText(e.target.value)} placeholder="Remember that…" aria-label="New fact" />
        <select className="inp" value={scope} onChange={(e) => setScope(e.target.value)} aria-label="Fact scope">
          <option value="ws">Workspace</option>
          {flatProjects(snap.projects).map(({ project, depth }) => <option key={project.id} value={project.id}>{'\u00a0'.repeat(depth * 2)}{project.name}</option>)}
        </select>
        <button type="submit" className="btn btn-p">Remember</button>
      </form>
      {!!suggested.length && <span className="lbl">To review</span>}
      {suggested.map((f) => (
        <div key={f.id} className="card" style={{ padding: 12, display: 'flex', gap: 8, alignItems: 'center' }}>
          <span className="grow">{f.text}</span>
          <button type="button" className="btn btn-p" onClick={() => void act(() => patch(`/api/ws/${wsId()}/facts/${f.id}`, { status: 'approved' }).then(() => refreshSnap()), 'Approved')}>Approve</button>
          <button type="button" className="btn" onClick={() => void act(() => del(`/api/ws/${wsId()}/facts/${f.id}`).then(() => refreshSnap()), 'Forgotten')}>Forget</button>
        </div>
      ))}
      <span className="lbl">Remembered</span>
      {approved.map((f) => (
        <div key={f.id} className="row card" style={{ padding: '8px 12px' }}>
          <span className="grow">{f.text}<span className="muted" style={{ marginLeft: 8, fontSize: 12 }}>{f.scope === 'ws' ? 'workspace' : snap.projects.find((p) => p.id === f.scope)?.name}</span></span>
          <button type="button" className="link" onClick={() => void act(() => del(`/api/ws/${wsId()}/facts/${f.id}`).then(() => refreshSnap()), 'Forgotten')}>Forget</button>
        </div>
      ))}
      {!approved.length && <p className="muted">Nothing is remembered in this workspace yet.</p>}
    </div>
  );
}
