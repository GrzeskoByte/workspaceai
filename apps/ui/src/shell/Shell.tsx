// The main window: workspace rail, project tree sidebar, main view, artifact panel, overlays (R-01).
import { lazy, Suspense, useEffect, useMemo, useState } from 'react';
import { useStore, setState, go, switchWorkspace, act, wsId, refreshSnap, getState, type View } from '../store.ts';
import { post, patch } from '../api.ts';
import { projectPath, childProjects, fuzzyScore } from '../../../../packages/shared/src/util.ts';
import type { Artifact, Chat, GraphMeta, Project } from '../../../../packages/shared/src/types.ts';
import { Home } from '../views/Home.tsx';
import { ChatView } from '../views/Chat.tsx';
import { ArtifactPanel, ART_ICON } from '../views/ArtifactPanel.tsx';
import { BrowserPanel } from '../views/BrowserPanel.tsx';
import { Icon } from '../ui/Icon.tsx';
import { Library } from '../views/Library.tsx';
import { ProjectView } from '../views/Project.tsx';
import { SettingsView } from '../views/Settings.tsx';
import { Palette } from './Palette.tsx';
import { NewWorkspace } from './NewWorkspace.tsx';
import { CustomProvider } from '../views/CustomProvider.tsx';
import { Confirm, Toasts } from './Overlays.tsx';
// The graph editor (bpmn-js and its CSS) is most of the bundle; it loads when the Graphs screen first opens.
const MapView = lazy(() => import('../views/MapView.tsx').then((m) => ({ default: m.MapView })));
const RunPanel = lazy(() => import('../views/RunPanel.tsx').then((m) => ({ default: m.RunPanel })));
const GraphView = lazy(() => import('../views/Graph.tsx').then((m) => ({ default: m.GraphView })));
import { RoutinesView } from '../views/Routines.tsx';
import { ComputersView } from '../views/Computers.tsx';
import { MemoryView } from '../views/Memory.tsx';
import { BriefView, GenerateView } from '../views/Create.tsx';

export function openArtifact(a: Artifact): void {
  const s = getState();
  const from = a.chat && s.snap?.chats.find((c) => c.id === a.chat);
  if (from) go('chat', { chatId: from.id, projId: a.project, artId: a.id });
  else go('artifacts', { projId: a.project, artId: a.id });
}

export function openChat(c: Chat): void { go('chat', { chatId: c.id, projId: c.project, artId: null }); }

export function Shell() {
  const view = useStore((s) => s.view);
  const snap = useStore((s) => s.snap);
  const artId = useStore((s) => s.artId);
  const artWide = useStore((s) => s.artWide);
  const browserOn = useStore((s) => !!s.browser);
  const browserWide = useStore((s) => s.browserWide);
  const runId = useStore((s) => s.runId);
  const runWide = useStore((s) => s.runWide);
  const mapOn = useStore((s) => s.mapOpen);
  const mapWide = useStore((s) => s.mapWide);
  const chatId = useStore((s) => s.chatId);
  const active = useStore((s) => s.app?.settings.activeWorkspace);
  const navOpen = useStore((s) => s.navOpen);
  useEffect(() => {
    if (!navOpen) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setState({ navOpen: false }); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [navOpen]);
  // Never show one workspace's data while another is active (R-06).
  if (!snap || snap.workspace.id !== active) return <div style={{ margin: 'auto' }} className="muted mono pulse">loading workspace…</div>;
  const artOpen = !!artId && snap.artifacts.some((a) => a.id === artId) && (view === 'chat' || view === 'artifacts');
  // An open artifact covers the browser; closing it shows the browser again.
  const runOpen = !!runId && !artOpen && (view === 'chat' || view === 'project');
  // Side panels, by priority: artifact, run, map, browser. Closing one shows the next.
  const chatProject = view === 'chat' ? snap.chats.find((c) => c.id === chatId)?.project : undefined;
  const mapOpen = mapOn && !artOpen && !runOpen && !!chatProject;
  const browserOpen = browserOn && !artOpen && !runOpen && !mapOpen && (view === 'chat' || view === 'project');
  return (
    <>
      <div style={{ height: 2, background: 'var(--ws-accent)', transition: 'background-color .4s ease', flex: '0 0 auto' }} />
      <Header />
      <div style={{ flex: '1 1 auto', display: 'flex', minHeight: 0 }}>
        {navOpen && <div className="shell-scrim" onClick={() => setState({ navOpen: false })} />}
        <div className={'shell-nav' + (navOpen ? ' open' : '')}>
          <Rail />
          <Sidebar />
        </div>
        <main className={artOpen && artWide ? 'art-focus' : runOpen && runWide ? 'run-focus' : mapOpen && mapWide ? 'map-focus' : browserOpen && browserWide ? 'browser-focus' : undefined} style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', minHeight: 0, position: 'relative' }}>
          {view === 'home' && <Home />}
          {view === 'chat' && <ChatView />}
          {view === 'artifacts' && <Library />}
          {view === 'project' && <ProjectView />}
          {view === 'settings' && <SettingsView />}
          {view === 'map' && <Suspense fallback={<div style={{ margin: 'auto' }} className="muted pulse">Loading the map…</div>}><MapView scope="ws" /></Suspense>}
          {view === 'graph' && <Suspense fallback={<div style={{ margin: 'auto' }} className="muted pulse">Loading the graph editor…</div>}><GraphView /></Suspense>}
          {view === 'routines' && <RoutinesView />}
          {view === 'computers' && <ComputersView />}
          {view === 'memory' && <MemoryView />}
          {view === 'brief' && <BriefView />}
          {view === 'generate' && <GenerateView />}
          {artOpen && <ArtifactPanel />}
          {runOpen && <Suspense fallback={<section className="art-panel run-panel" style={{ flex: '0 0 560px' }}><span className="muted pulse" style={{ margin: 'auto' }}>Loading the run…</span></section>}><RunPanel /></Suspense>}
          {mapOpen && <section aria-label="Project map" className="rise art-panel map-panel" style={{ flex: mapWide ? '1 1 auto' : '0 0 620px' }}><Suspense fallback={<span className="muted pulse" style={{ margin: 'auto' }}>Loading the map…</span>}><MapView scope={chatProject!} compact focus={'chat:' + chatId} wide={mapWide} onWide={() => setState({ mapWide: !mapWide })} onClose={() => setState({ mapOpen: false, mapWide: false })} /></Suspense></section>}
          {browserOpen && <BrowserPanel />}
        </main>
      </div>
      <Palette />
      <NewWorkspace />
      <CustomProvider />
      <Confirm />
      <Toasts />
    </>
  );
}

const VIEW_NAMES: Partial<Record<View, string>> = { home: 'Home', artifacts: 'Artifacts', graph: 'Graphs', routines: 'Routines', computers: 'Computers', memory: 'Memory', settings: 'Settings', brief: 'New artifact', generate: 'Generating' };

function Header() {
  const snap = useStore((s) => s.snap)!;
  const view = useStore((s) => s.view);
  const projId = useStore((s) => s.projId);
  const chatId = useStore((s) => s.chatId);
  const theme = useStore((s) => s.app?.settings.theme || 'claude');
  const ws = snap.workspace;
  const chat = snap.chats.find((c) => c.id === chatId);
  const crumbs: { label: string; go: () => void }[] = [{ label: ws.name, go: () => go('home') }];
  if (view === 'project' || (view === 'chat' && chat)) for (const p of projectPath(snap.projects, view === 'chat' && chat ? chat.project : projId)) crumbs.push({ label: p.name, go: () => go('project', { projId: p.id, ptab: 'chats' }) });
  if (view === 'chat' && chat) crumbs.push({ label: chat.title, go: () => {} });
  else if (VIEW_NAMES[view]) crumbs.push({ label: VIEW_NAMES[view] as string, go: () => go(view) });
  const e = snap.engine;
  const engLabel = e.state === 'running' ? 'opencode :' + e.port : e.state === 'connected' ? 'opencode · ' + e.url.replace(/^https?:\/\//, '') : 'opencode · ' + e.state;
  const engDot = e.state === 'running' || e.state === 'connected' ? 'var(--success)' : e.state === 'error' ? 'var(--error)' : e.state === 'stopped' ? 'var(--dim)' : 'var(--warning)';
  const toggleTheme = () => act(() => patch('/api/settings', { theme: theme === 'claude' ? 'opencode' : 'claude', followSystem: false }).then((app) => setState({ app })));
  const navOpen = useStore((s) => s.navOpen);
  return (
    <header style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '7px 14px', borderBottom: '1px solid var(--bg2)', flex: '0 0 auto' }}>
      <button type="button" className="chip shell-menu" aria-label="Projects and workspaces" aria-expanded={navOpen} onClick={() => setState({ navOpen: !navOpen })}><Icon name="menu" /></button>
      <span className="mono hdr-brand" style={{ fontWeight: 600 }}>harness</span>
      <nav aria-label="Breadcrumb" className="row grow" style={{ gap: 6, fontSize: 13, minWidth: 0, overflow: 'hidden' }}>
        {crumbs.map((c, i) => <span key={i} className="row" style={{ gap: 6, minWidth: 0 }}>{i > 0 && <span style={{ color: 'var(--dim)' }}>/</span>}<button type="button" className="link ellipsis" onClick={c.go} style={{ color: i === crumbs.length - 1 ? 'var(--text)' : 'var(--muted)', maxWidth: 260 }}>{c.label}</button></span>)}
      </nav>
      <button type="button" className="btn hdr-search" aria-label="Search or jump to" onClick={() => setState({ palette: true })} style={{ height: 28, minWidth: 240, justifyContent: 'space-between', color: 'var(--muted)' }}><span className="hdr-text">Search or jump to…</span><span className="hdr-icon"><Icon name="search" size={14} /></span> <span className="mono hdr-text" style={{ fontSize: 11 }}>⌘K</span></button>
      <button type="button" className="chip mono" onClick={() => go('settings', { setPage: 'engine' })} aria-label={engLabel} title={e.error || engLabel} style={{ fontSize: 11 }}><span className={'dot ' + (e.state === 'starting' || e.state === 'restarting' ? 'pulse' : '')} style={{ width: 6, height: 6, background: engDot }} /><span className="hdr-text">{engLabel}</span></button>
      <button type="button" className="chip" onClick={toggleTheme} aria-label="Switch theme"><Icon name="contrast" size={14} /> <span className="hdr-text">{theme === 'claude' ? 'Claude' : 'opencode'}</span></button>
    </header>
  );
}

function Rail() {
  const app = useStore((s) => s.app)!;
  const view = useStore((s) => s.view);
  const active = app.settings.activeWorkspace;
  return (
    <nav aria-label="Workspaces" style={{ flex: '0 0 60px', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10, padding: '12px 0', borderRight: '1px solid var(--bg2)' }}>
      {app.workspaces.map((w, i) => (
        <button key={w.id} type="button" aria-label={w.name + ' workspace'} title={`${w.name} · ⌘${i + 1}`} onClick={() => w.id !== active && switchWorkspace(w.id)} style={{ width: 40, height: 40, borderRadius: 10, border: 0, background: 'var(--bg2)', color: w.kind === 'org' ? 'var(--accent-o-text)' : w.kind === 'team' ? 'var(--cyan-text)' : 'var(--accent-p-text)', fontFamily: 'var(--font-mono)', fontWeight: 700, fontSize: 12, boxShadow: `0 0 0 2px ${w.id === active ? (w.kind === 'org' ? 'var(--accent-o)' : 'var(--accent-p)') : 'transparent'}` }}>{w.initials}</button>
      ))}
      <button type="button" aria-label="Add workspace" title="New workspace or join an organization" onClick={() => setState({ newWs: { type: 'team', name: '' } })} style={{ width: 40, height: 40, borderRadius: 10, border: '1px dashed var(--line3)', background: 'transparent', color: 'var(--muted)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}><Icon name="plus" size={18} /></button>
      <span style={{ flex: '1 1 auto' }} />
      {([['routines', 'clock', 'Routines'], ['computers', 'monitor', 'Computers'], ['memory', 'memory', 'Memory'], ['settings', 'settings', 'Settings']] as [View, string, string][]).map((n) => (
        <button key={n[0]} type="button" aria-label={n[2]} title={n[2]} onClick={() => go(n[0])} style={{ width: 40, height: 36, borderRadius: 9, border: 0, background: view === n[0] ? 'var(--bg3)' : 'transparent', color: view === n[0] ? 'var(--text)' : 'var(--muted)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}><Icon name={n[1]} size={18} /></button>
      ))}
    </nav>
  );
}

interface TreeRow { key: string; kind: 'project' | 'chat' | 'artifact' | 'graph' | 'input'; label: string; depth: number; icon: string; iconColor: string; meta: string; cur: boolean; caret: string; open?: boolean; onOpen?: () => void; project?: Project; chat?: Chat; parentFor?: string | null; placeholder?: string }

function Sidebar() {
  const snap = useStore((s) => s.snap)!;
  const view = useStore((s) => s.view);
  const projId = useStore((s) => s.projId);
  const chatId = useStore((s) => s.chatId);
  const artId = useStore((s) => s.artId);
  const graphId = useStore((s) => s.graphId);
  const open = useStore((s) => s.open);
  // Only which chats are streaming matters here, not each token (a string keeps the selector stable).
  const streamingKey = useStore((s) => Object.keys(s.live).sort().join(','));
  const live = useMemo(() => new Set(streamingKey ? streamingKey.split(',') : []), [streamingKey]);
  const [q, setQ] = useState('');
  const [addFor, setAddFor] = useState<string | null>(null);
  const [draft, setDraft] = useState('');
  const [dropOn, setDropOn] = useState<string | null>(null);
  const ws = snap.workspace;
  const proj = snap.projects.find((p) => p.id === projId) || snap.projects[0];
  const routinesWaiting = snap.routines.filter((r) => r.runs[0]?.status === 'waiting').length;

  const rows = useMemo(() => {
    const out: TreeRow[] = [];
    const query = q.trim();
    if (addFor === 'root') out.push({ key: 'in-root', kind: 'input', label: '', depth: 0, icon: '', iconColor: '', meta: '', cur: false, caret: '', parentFor: null, placeholder: 'New project name' });
    if (query) {
      for (const p of snap.projects) if (fuzzyScore(query, p.name) >= 2) out.push({ key: p.id, kind: 'project', label: p.name, depth: 0, icon: 'folder', iconColor: 'var(--muted)', meta: projectPath(snap.projects, p.id).slice(-2, -1)[0] ? 'in ' + projectPath(snap.projects, p.id).slice(-2, -1)[0].name : '', cur: false, caret: '', project: p, onOpen: () => go('project', { projId: p.id, ptab: 'chats' }) });
      for (const a of snap.artifacts) if (fuzzyScore(query, a.title) >= 2) out.push({ key: a.id, kind: 'artifact', label: a.title, depth: 0, icon: ART_ICON[a.type] || 'layers', iconColor: 'var(--accent-p-text)', meta: snap.projects.find((p) => p.id === a.project)?.name || '', cur: false, caret: '', onOpen: () => openArtifact(a) });
      for (const g of snap.graphs) if (fuzzyScore(query, g.name) >= 2) out.push({ key: g.id, kind: 'graph', label: g.name, depth: 0, icon: 'workflow', iconColor: 'var(--cyan-text, var(--accent-p-text))', meta: snap.projects.find((p) => p.id === g.project)?.name || '', cur: false, caret: '', onOpen: () => go('graph', { graphId: g.id }) });
      for (const c of snap.chats) if (fuzzyScore(query, c.title) >= 2) out.push({ key: c.id, kind: 'chat', label: c.title, depth: 0, icon: 'chat', iconColor: 'var(--muted)', meta: snap.projects.find((p) => p.id === c.project)?.name || '', cur: false, caret: '', chat: c, onOpen: () => openChat(c) });
      return out;
    }
    const graphRow = (g: GraphMeta, depth: number): TreeRow => ({ key: g.id, kind: 'graph', label: g.name, depth, icon: 'workflow', iconColor: 'var(--cyan-text, var(--accent-p-text))', meta: 'graph', cur: view === 'graph' && graphId === g.id, caret: '', onOpen: () => go('graph', { graphId: g.id }) });
    const walk = (pid: string | null, depth: number) => {
      const kids = childProjects(snap.projects, pid).sort((a, b) => Number(!!b.isGeneral) - Number(!!a.isGeneral) || a.createdAt - b.createdAt);
      for (const p of kids) {
        const isOpen = !!open[p.id];
        const chats = snap.chats.filter((c) => c.project === p.id).sort((a, b) => b.updatedAt - a.updatedAt);
        const arts = snap.artifacts.filter((a) => a.project === p.id);
        const graphs = snap.graphs.filter((g) => g.project === p.id);
        const n = childProjects(snap.projects, p.id).length + chats.length + arts.length + graphs.length;
        out.push({ key: p.id, kind: 'project', label: p.name, depth, icon: 'folder', iconColor: depth === 0 ? 'var(--ws-accent)' : 'var(--muted)', meta: !isOpen && n ? String(n) : p.isGeneral ? 'default' : '', cur: view === 'project' && proj?.id === p.id, caret: n ? '▶' : '', open: isOpen, project: p,
          onOpen: () => { setState((s) => ({ open: { ...s.open, [p.id]: !isOpen } })); go('project', { projId: p.id, ptab: 'chats' }); } });
        if (!isOpen) continue;
        if (addFor === p.id) out.push({ key: 'in-' + p.id, kind: 'input', label: '', depth: depth + 1, icon: '', iconColor: '', meta: '', cur: false, caret: '', parentFor: p.id, placeholder: 'Sub-project of ' + p.name });
        walk(p.id, depth + 1);
        for (const c of chats) {
          const made = arts.filter((a) => a.chat === c.id);
          const madeGraphs = graphs.filter((g) => g.chat === c.id);
          out.push({ key: c.id, kind: 'chat', label: c.title, depth: depth + 1, icon: 'chat', iconColor: 'var(--muted)', meta: live.has(c.id) ? '…' : made.length + madeGraphs.length ? String(made.length + madeGraphs.length) : c.imported ? 'imported' : '', cur: view === 'chat' && chatId === c.id, caret: '', chat: c, onOpen: () => openChat(c) });
          for (const a of made) out.push({ key: a.id, kind: 'artifact', label: a.title, depth: depth + 2, icon: ART_ICON[a.type] || 'layers', iconColor: 'var(--accent-p-text)', meta: a.type.toLowerCase() + ' v' + a.version, cur: artId === a.id, caret: '', onOpen: () => openArtifact(a) });
          for (const g of madeGraphs) out.push(graphRow(g, depth + 2));
        }
        // Graphs made in the Graphs screen or found as files sit at the project's root.
        for (const g of graphs.filter((x) => !x.chat || !chats.some((c) => c.id === x.chat))) out.push(graphRow(g, depth + 1));
        for (const a of arts.filter((x) => !x.chat || !chats.some((c) => c.id === x.chat))) out.push({ key: a.id, kind: 'artifact', label: a.title, depth: depth + 1, icon: ART_ICON[a.type] || 'layers', iconColor: 'var(--accent-p-text)', meta: a.type.toLowerCase() + ' v' + a.version, cur: artId === a.id, caret: '', onOpen: () => openArtifact(a) });
      }
    };
    walk(null, 0);
    return out;
  }, [snap, open, q, addFor, view, projId, chatId, artId, graphId, live]);

  const commit = async (parent: string | null) => {
    const name = draft.trim();
    if (!name) { setAddFor(null); return; }
    const p = await act(() => post<Project>(`/api/ws/${wsId()}/projects`, { name, parent }), (r) => (parent ? 'Sub-project created · ' : 'Project created · ') + r.name);
    setAddFor(null); setDraft('');
    if (p) { await refreshSnap(); setState((s) => ({ open: { ...s.open, [p.id]: true, ...(parent ? { [parent]: true } : {}) } })); go('project', { projId: p.id, ptab: 'chats' }); }
  };

  // Drag a chat onto a project to move it (its artifacts follow); drag a project to re-parent it.
  const onDrop = async (target: Project, data: string) => {
    setDropOn(null);
    const [kind, id] = data.split(':');
    if (kind === 'chat') {
      const r = await act(() => post<{ movedArtifacts: number }>(`/api/ws/${wsId()}/chats/${id}/move`, { project: target.id }), (x) => 'Moved to ' + target.name + (x.movedArtifacts ? ` with ${x.movedArtifacts === 1 ? 'its artifact' : 'its ' + x.movedArtifacts + ' artifacts'}` : ''));
      if (r) setState((s) => ({ open: { ...s.open, [target.id]: true } }));
    } else if (kind === 'project' && id !== target.id) {
      await act(() => patch(`/api/ws/${wsId()}/projects/${id}`, { parent: target.id }), 'Moved under ' + target.name);
    }
  };

  return (
    <aside aria-label="Projects" style={{ flex: '0 0 272px', display: 'flex', flexDirection: 'column', gap: 10, padding: 12, background: 'var(--bg1)', borderRight: '1px solid var(--bg2)', minHeight: 0 }}>
      <button type="button" className="navrow" onClick={() => setState({ palette: true })} style={{ padding: '8px 10px', background: 'var(--bg2)' }}>
        <span className="mono" style={{ width: 28, height: 28, borderRadius: 7, background: 'var(--bg3)', color: 'var(--ws-accent-text)', fontWeight: 700, fontSize: 11, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>{ws.initials}</span>
        <span className="grow" style={{ display: 'flex', flexDirection: 'column', textAlign: 'left' }}><b style={{ fontSize: 13 }}>{ws.name}</b><span className="muted" style={{ fontSize: 11 }}>{ws.kind === 'org' ? 'Organization · policy-managed' : ws.kind === 'team' ? 'Team workspace' : 'Personal workspace'}</span></span>
        <span className="muted"><Icon name="updown" size={14} /></span>
      </button>
      <button type="button" className="btn btn-p" onClick={() => { setState({ composeProj: proj?.id || null, draft: '' }); go('home'); }} style={{ justifyContent: 'center' }}>+ New chat in {proj?.name || 'General'}</button>
      <label className="row" style={{ padding: '0 10px', height: 32, borderRadius: 8, border: '1px solid var(--line)', background: 'var(--bg0)' }}>
        <span className="muted" style={{ display: 'flex' }}><Icon name="search" size={14} /></span><span className="sr">Filter projects and chats</span>
        <input type="search" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter projects and chats" style={{ flex: '1 1 auto', minWidth: 0, border: 0, outline: 'none', background: 'transparent', color: 'var(--text)', font: 'inherit', fontSize: 13 }} />
      </label>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
        {([['home', 'home', 'Home', ''], ['artifacts', 'layers', 'Artifacts', String(snap.artifacts.length)], ['graph', 'workflow', 'Graphs', String(snap.graphs.length || '')], ['map', 'map', 'Map', ''], ['routines', 'clock', 'Routines', routinesWaiting ? routinesWaiting + ' waiting' : ''], ['memory', 'memory', 'Memory', String(snap.facts.filter((f) => f.status === 'suggested').length || '') && snap.facts.filter((f) => f.status === 'suggested').length + ' to review']] as [View, string, string, string][]).map((n) => (
          <button key={n[0]} type="button" className="navrow" aria-current={view === n[0] ? 'page' : 'false'} onClick={() => go(n[0])}><span style={{ width: 18, display: 'flex', justifyContent: 'center', color: view === n[0] ? 'var(--text)' : 'var(--muted)' }}><Icon name={n[1]} size={16} /></span>{n[2]}<span className="muted" style={{ marginLeft: 'auto', fontSize: 11 }}>{n[3] === '0' ? '' : n[3]}</span></button>
        ))}
      </div>
      <span className="lbl row" style={{ padding: '4px 4px 0' }}>Projects<button type="button" className="link" onClick={() => { setAddFor('root'); setDraft(''); }} style={{ marginLeft: 'auto', textTransform: 'none', letterSpacing: 0, fontFamily: 'var(--font-ui)', fontSize: 12 }}>+ project</button></span>
      <div style={{ flex: '1 1 auto', overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 1, minHeight: 0 }}>
        {rows.map((t) => t.kind === 'input' ? (
          <form key={t.key} className="rise row" onSubmit={(e) => { e.preventDefault(); commit(t.parentFor ?? null); }} style={{ gap: 6, padding: `4px 4px 4px ${6 + t.depth * 14}px` }}>
            <input className="inp grow" autoFocus value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={t.placeholder} style={{ height: 30 }} aria-label="New project name" onKeyDown={(e) => e.key === 'Escape' && setAddFor(null)} />
            <button type="submit" className="btn btn-p" style={{ height: 30, padding: '0 10px' }}>Add</button>
            <button type="button" className="btn" onClick={() => setAddFor(null)} style={{ height: 30, padding: '0 8px' }} aria-label="Cancel"><Icon name="x" size={14} /></button>
          </form>
        ) : (
          <div key={t.key} className={'treerow' + (dropOn === t.key ? ' drop' : '')} aria-current={t.cur ? 'page' : undefined} style={{ paddingLeft: 4 + t.depth * 14 }}
            draggable={t.kind === 'chat' || (t.kind === 'project' && !t.project?.isGeneral)}
            onDragStart={(e) => { e.dataTransfer.setData('text/plain', `${t.kind}:${t.key}`); e.dataTransfer.effectAllowed = 'move'; }}
            onDragOver={(e) => { if (t.kind === 'project') { e.preventDefault(); setDropOn(t.key); } }}
            onDragLeave={() => setDropOn((d) => (d === t.key ? null : d))}
            onDrop={(e) => { e.preventDefault(); if (t.project) onDrop(t.project, e.dataTransfer.getData('text/plain')); }}>
            <button type="button" className="treemain" onClick={t.onOpen}>
              <span style={{ width: 12, color: 'var(--muted)', display: 'inline-flex', transform: t.open ? 'rotate(90deg)' : 'none', transition: 'transform .2s ease' }}>{t.caret && <Icon name="right" size={12} stroke={2} />}</span>
              <span style={{ color: t.iconColor, minWidth: 14, display: 'inline-flex', justifyContent: 'center' }}><Icon name={t.icon} size={14} /></span>
              <span className="ellipsis grow" style={{ textAlign: 'left' }}>{t.label}</span>
              <span className="muted mono" style={{ fontSize: 10.5 }}>{t.meta}</span>
            </button>
            {t.kind === 'project' && t.project && <>
              <button type="button" className="treeact" aria-label={'New chat in ' + t.label} title="New chat here" onClick={() => { setState({ composeProj: t.project!.id, projId: t.project!.id }); go('home'); }}><Icon name="compose" size={13} /></button>
              <button type="button" className="treeact" aria-label={'New sub-project in ' + t.label} title="New sub-project" onClick={() => { setAddFor(t.project!.id); setDraft(''); setState((s) => ({ open: { ...s.open, [t.project!.id]: true } })); }}><Icon name="plus" size={13} /></button>
            </>}
          </div>
        ))}
        {q && !rows.length && <span className="muted" style={{ fontSize: 12, padding: 8 }}>Nothing matches “{q}”.</span>}
      </div>
    </aside>
  );
}
