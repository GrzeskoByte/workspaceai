// A chat is an opencode session, one to one (R-16). Header shows the session id; the menu maps to the session API.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useStore, setState, go, act, wsId, refreshSnap, loadMessages, loadPermissions, toast, getState, openBrowser } from '../store.ts';
import { post, patch, del, get } from '../api.ts';
import { flatProjects, followsBottom, projectPathName } from '../../../../packages/shared/src/util.ts';
import type { Chat, ChatMessage, GraphMeta } from '../../../../packages/shared/src/types.ts';
import { Markdown } from '../ui/Markdown.tsx';
import { ModelPicker, useOutside, useAnchored, pickModel, effortLabel } from './Composer.tsx';
import { openChat } from '../shell/Shell.tsx';
import { parseSlash, findHarnessCommand, type ChatCommand } from '../../../../packages/shared/src/commands.ts';
import { ActionSheet, type SheetContent, type SheetState } from './ActionSheet.tsx';
import { SignInCard } from './SignInCard.tsx';
import { RunCard } from './RunCard.tsx';
import { localUrlIn } from './BrowserPanel.tsx';
import { CommandMenu, NoteCard, matchCommands, type ChatNote, type ChatStatus } from './Commands.tsx';
import { FileMenu, useFileMention } from './FileMention.tsx';
import { Icon } from '../ui/Icon.tsx';

// Only messages that just arrived animate in; opening a chat with a long history shouldn't animate every row.
const fresh = (m: ChatMessage) => Date.now() - m.createdAt < 4000;

/** The reply being streamed: the only part of a chat that re-renders per token (batched to one render per frame). */
function LiveReply({ chatId, scroller, following }: { chatId: string; scroller: React.RefObject<HTMLDivElement | null>; following: React.RefObject<boolean> }) {
  const live = useStore((s) => s.live[chatId]);
  // Follow the reply while the reader is at the bottom; leave the scroll alone if they scrolled up to read.
  useEffect(() => { const el = scroller.current; if (el && following.current) el.scrollTop = el.scrollHeight; }, [live?.text.length, live?.tools.length]);
  if (!live) return null;
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {live.tools.map((t) => <span key={t.detail} className="mono rise" style={{ fontSize: 12, color: 'var(--muted)' }}><span style={{ color: t.status === 'running' ? 'var(--warning-text)' : t.status === 'error' ? 'var(--error-text)' : 'var(--success-text)', display: 'inline-flex', verticalAlign: '-2px' }}><Icon name={t.status === 'running' ? 'dot' : t.status === 'error' ? 'x' : 'check'} size={13} stroke={2.25} className={t.status === 'running' ? 'pulse' : undefined} /></span> {t.name}{t.status === 'running' ? '…' : ''}</span>)}
      <div><Markdown text={live.text} /><span className="pulse" style={{ display: 'inline-block', width: 8, height: 16, verticalAlign: -3, background: 'var(--accent-p)', marginLeft: 2 }} /></div>
    </div>
  );
}

export function ChatView() {
  const snap = useStore((s) => s.snap)!;
  const chatId = useStore((s) => s.chatId);
  const messages = useStore((s) => (chatId ? s.messages[chatId] : undefined));
  // Only whether a reply is streaming; the reply's text is rendered by LiveReply, so tokens don't re-render the chat.
  const live = useStore((s) => !!(chatId && s.live[chatId]));
  const perms = useStore((s) => (chatId ? s.permissions[chatId] : undefined));
  const artId = useStore((s) => s.artId);
  const browserOn = useStore((s) => !!s.browser);
  const mapOn = useStore((s) => s.mapOpen);
  const readsOn = useStore((s) => s.readsOpen);
  const runId = useStore((s) => s.runId);
  const models = useStore((s) => s.models);
  const agentModels = useStore((s) => s.agentModels);
  const modelGroups = useStore((s) => s.modelGroups);
  const chat = snap.chats.find((c) => c.id === chatId);
  const [draft, setDraft] = useState('');
  // Slash commands: what this chat's agent understands, the highlighted one in the menu, and cards they answered with.
  const [cmds, setCmds] = useState<ChatCommand[]>([]);
  const [cmdIdx, setCmdIdx] = useState(0);
  const [cmdHidden, setCmdHidden] = useState(false);
  const [notes, setNotes] = useState<ChatNote[]>([]);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [sheet, setSheet] = useState<SheetState | null>(null);
  const closeSheet = useCallback(() => setSheet(null), []);
  const [menu, setMenu] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [toolOpen, setToolOpen] = useState<Record<string, boolean>>({});
  const scroller = useRef<HTMLDivElement>(null);
  const menuRef = useOutside<HTMLDivElement>(() => setMenu(false));
  const optsBtn = useRef<HTMLButtonElement>(null);
  const menuPos = useAnchored(menu, optsBtn, 280, { align: 'right' });

  useEffect(() => { if (chatId && !messages) loadMessages(chatId); }, [chatId]);
  useEffect(() => { if (chatId) loadPermissions(chatId); }, [chatId, live]);
  useEffect(() => {
    setNotes([]); setCmds([]);
    if (!chatId) return;
    get<{ commands: ChatCommand[] }>(`/api/ws/${wsId()}/chats/${chatId}/commands`).then((r) => setCmds(r.commands), () => {});
  }, [chatId, snap.chats.find((c) => c.id === chatId)?.agent]);
  const following = useRef(true);
  const lastTop = useRef(0);
  const shown = useRef<string | null>(null);
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onScroll = () => {
      following.current = followsBottom(following.current, lastTop.current, el.scrollTop, el.scrollHeight - el.scrollTop - el.clientHeight);
      lastTop.current = el.scrollTop;
    };
    el.addEventListener('scroll', onScroll, { passive: true });
    return () => el.removeEventListener('scroll', onScroll);
  }, [!!chat]);
  // Opening a chat lands on its end with no animation. After that, a new message only moves a reader who is following, or who just sent it.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el || !messages) return;
    if (shown.current !== chatId) {
      shown.current = chatId;
      following.current = true;
      el.scrollTop = el.scrollHeight;
      lastTop.current = el.scrollTop;
      return;
    }
    if (messages[messages.length - 1]?.role === 'user') following.current = true;
    if (following.current) el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' });
  }, [messages?.length, chatId]);
  const composerInput = useRef<HTMLTextAreaElement>(null);
  const files = useFileMention(chat?.project ?? '', draft, setDraft, composerInput);
  if (!chat) return <div style={{ margin: 'auto' }} className="muted">This chat is gone. <button className="link" onClick={() => go('home')}>Go home</button></div>;

  const ws = snap.workspace;
  const webMode = ws.kind === 'org' && snap.policy ? snap.policy.web.mode : ws.web.mode;
  const webOn = webMode !== 'off' && chat.web !== false;
  const model = pickModel(agentModels, chat.agent, chat.model, effModel(snap, chat.project) || ws.defaultModel || models[0] || '');
  const projPath = projectPathName(snap.projects, chat.project);
  const nArts = snap.artifacts.filter((a) => a.chat === chat.id).length;
  const base = `/api/ws/${wsId()}/chats/${chat.id}`;
  // Artifact, run and map all sit above the reads panel.
  const readsShown = readsOn && !mapOn && !artId && !runId;

  const agentName = chat.agent === 'opencode' ? 'opencode' : ws.agents.find((a) => a.id === chat.agent)?.name || chat.agent;
  const showCmds = draft.startsWith('/') && !/\s/.test(draft) && !cmdHidden;
  const cmdItems = showCmds ? matchCommands(cmds, draft) : [];
  const addNote = (n: Omit<ChatNote, 'id'> & Record<string, unknown>) => setNotes((x) => [...x, { id: 'note_' + Date.now(), ...n } as ChatNote]);
  const efforts = (chat.agent === 'opencode' ? modelGroups.flatMap((g) => g.models) : agentModels[chat.agent]?.models || []).find((m) => m.id === model)?.efforts || [];
  const setEffort = (arg: string) => {
    const e = /^(default|auto|off)$/i.test(arg) ? '' : arg.toLowerCase();
    if (e && !efforts.includes(e)) { toast(efforts.length ? `${model.split('/').pop()} takes: ${efforts.map(effortLabel).join(', ')} or default` : `${model.split('/').pop()} has no effort setting`, 'error'); return; }
    act(() => patch(base, { effort: e }), 'Effort · ' + (e ? effortLabel(e) : 'model default'));
  };
  const runSlash = async (t: string) => {
    const c = parseSlash(t)!;
    setDraft(''); setCmdHidden(false);
    const at = Date.now();
    // Info commands open a sheet straight away and fill it when the agent answers; nothing goes into the chat.
    const h = findHarnessCommand(c.name);
    // /graph: pick one of the project's graphs, or run the one named; the run shows in this chat.
    if (h?.name === 'graph') {
      const list = await act(() => post<{ graphs: GraphMeta[] }>(`/api/ws/${wsId()}/graphs/scan`).then((r) => r.graphs));
      if (!list) return;
      const want = c.args.toLowerCase();
      const match = want && (list.find((g) => g.name.toLowerCase() === want) || list.find((g) => g.name.toLowerCase().includes(want) || g.file.toLowerCase().endsWith(want)));
      if (want && match) { void runGraph(match.id, /--dry/.test(t)); return; }
      if (want) toast(`No graph called “${c.args}”`, 'error');
      const here = new Set(snap.projects.filter((p) => p.id === chat.project || projectPathName(snap.projects, p.id).startsWith(projectPathName(snap.projects, chat.project))).map((p) => p.id));
      setSheet({ title: 'Graphs', command: 'graph', agentName, phase: 'graphs', graphs: [...list].sort((a, b) => Number(here.has(b.project)) - Number(here.has(a.project))).map((g) => ({ id: g.id, name: g.name, file: g.file, project: projectPathName(snap.projects, g.project) })) });
      return;
    }
    if (h?.sheet) {
      const head = { title: h.sheet, command: h.name, agentName };
      if (h.name === 'help') { setSheet({ ...head, phase: 'help', commands: cmds }); return; }
      setSheet({ ...head, phase: 'loading' });
      try {
        const r = await post<{ ui?: string; sheet?: SheetContent; status?: ChatStatus }>(base + '/command', { text: t });
        const content = r.sheet || (r.status ? { command: h.name, title: h.sheet, status: r.status } : null);
        setSheet((cur) => (cur && cur.command === h.name ? (content ? { ...head, phase: 'ready', content } : { ...head, phase: 'error', error: 'No answer' }) : cur));
      } catch (e) { setSheet((cur) => (cur && cur.command === h.name ? { ...head, phase: 'error', error: (e as Error).message } : cur)); }
      return;
    }
    const r = await act(() => post<{ ran?: string; ui?: string; args?: string; status?: ChatStatus }>(base + '/command', { text: t }));
    if (!r) { setDraft(t); return; }
    if (r.ui === 'status' && r.status) addNote({ kind: 'status', at, status: r.status, ran: r.ran });
    else if (r.ui === 'help') addNote({ kind: 'help', at, commands: cmds, agentName });
    else if (r.ui === 'model') setPickerOpen(true);
    else if (r.ui === 'effort') { if (r.args) setEffort(r.args); else setPickerOpen(true); }
    else if (r.ui === 'new') { go('home', { chatId: null, composeProj: chat.project, homeAgent: chat.agent, homeModel: chat.model || null, homeEffort: chat.effort || null, draft: c.args }); toast(`New chat with ${agentName} — same model and effort`); }
    if (r.ran) setState((s) => ({ messages: { ...s.messages, [chat.id]: [...(s.messages[chat.id] || []), { id: 'local_' + Date.now(), role: 'user', text: r.ran!, createdAt: Date.now() }] }, live: { ...s.live, [chat.id]: { text: '', tools: [], messageId: '' } } }));
  };
  const runGraph = async (gid: string, dryRun = false) => {
    setSheet(null);
    await act(() => post(`/api/ws/${wsId()}/graphs/${gid}/run`, { dryRun, from: chat.id }).then(() => loadMessages(chat.id)));
  };
  const pickCmd = (c: ChatCommand) => {
    // A command that needs an argument is completed for typing it; the rest run straight away.
    if (c.hint?.startsWith('<')) { setDraft('/' + c.name + ' '); return; }
    void runSlash('/' + c.name);
  };
  const onKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (files.onKey(e)) return;
    if (cmdItems.length) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); setCmdIdx((i) => (i + (e.key === 'ArrowDown' ? 1 : -1) + cmdItems.length) % cmdItems.length); return; }
      if (e.key === 'Tab') { e.preventDefault(); setDraft('/' + cmdItems[Math.min(cmdIdx, cmdItems.length - 1)].name + ' '); return; }
      if (e.key === 'Escape') { e.preventDefault(); setCmdHidden(true); return; }
      if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); pickCmd(cmdItems[Math.min(cmdIdx, cmdItems.length - 1)]); return; }
    }
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); }
  };

  const send = async (text = draft) => {
    const t = text.trim();
    if (!t) { toast('Type a message first'); return; }
    if (parseSlash(t)) { void runSlash(t); return; }
    if (live) return;
    const body = artId ? `[Revise the open artifact “${snap.artifacts.find((a) => a.id === artId)?.title}” (artifact id ${artId}) with the artifact_update tool, so Open Design saves a new version.] ${t}` : t;
    setDraft('');
    setState((s) => ({ messages: { ...s.messages, [chat.id]: [...(s.messages[chat.id] || []), { id: 'local_' + Date.now(), role: 'user', text: t, createdAt: Date.now() }] }, live: { ...s.live, [chat.id]: { text: '', tools: [], messageId: '' } } }));
    const ok = await act(() => post(base + '/send', { text: body }));
    if (!ok) setState((s) => { const l = { ...s.live }; delete l[chat.id]; return { live: l }; });
  };
  const stop = () => act(() => post(base + '/stop'), 'Stopped');
  const copyAttach = async () => {
    setMenu(false);
    const r = await act(() => get<{ command: string }>(base + '/attach'));
    if (!r) return;
    try { await navigator.clipboard.writeText(r.command); toast('Copied the opencode TUI command'); } catch { toast(r.command); }
  };
  const fork = async () => { setMenu(false); const c = await act(() => post<Chat>(base + '/fork'), 'Forked into a new opencode session'); if (c) { await refreshSnap(); openChat(c); } };
  const saveRename = async () => { const t = (renaming || '').trim(); setRenaming(null); if (t && t !== chat.title) await act(() => patch(base, { title: t }), 'Renamed'); };
  const move = async (pid: string, name: string) => { setMenu(false); const r = await act(() => post<{ movedArtifacts: number }>(base + '/move', { project: pid }), (x) => 'Moved to ' + name + (x.movedArtifacts ? ` with ${x.movedArtifacts === 1 ? 'its artifact' : 'its ' + x.movedArtifacts + ' artifacts'}` : '')); if (r) setState((s) => ({ projId: pid, open: { ...s.open, [pid]: true } })); };
  const askDelete = () => { setMenu(false); setState({ confirm: { title: 'Delete this chat?', body: `The chat and its opencode session are removed from ${projPath}. Artifacts it made stay in the library.`, ok: 'Delete', run: async () => { if (await act(() => del(base), 'Chat deleted')) go('project', { projId: chat.project, chatId: null }); } } }); };
  const toggleWeb = () => { if (webMode === 'off') { go('settings', { setPage: 'web' }); toast(`Web search is off for ${ws.name} — pick a search here`); return; } act(() => patch(base, { web: !webOn }), 'Web search ' + (webOn ? 'off' : 'on') + ' for this chat'); };
  const toggleYolo = async () => {
    const next = !chat.yolo;
    const r = await act(() => patch<Chat>(base, { yolo: next }), next ? 'Full access on — tools run without asking in this chat' : 'Ask first restored for this chat');
    if (!r) return;
    if (next) setState((s) => ({ permissions: { ...s.permissions, [chat.id]: [] } }));
    else void loadPermissions(chat.id);
  };
  const pick = (agent: string, m: string, effort: string) => {
    if (agent !== chat.agent && (messages?.length || 0) > 0) { toast('A chat keeps its agent — start a new chat for ' + (ws.agents.find((a) => a.id === agent)?.name || agent)); return; }
    act(() => patch(base, { ...(agent !== chat.agent ? { agent } : {}), model: m, effort }), 'Model · ' + m.split('/').pop() + (effort ? ' · ' + effortLabel(effort) : ''));
  };
  const remember = (m: ChatMessage) => act(() => post(`/api/ws/${wsId()}/facts`, { text: m.text.split('\n').find((l) => l.trim())?.slice(0, 200) || m.text.slice(0, 200), scope: chat.project, source: 'chat: ' + chat.title }), 'Saved to ' + (snap.projects.find((p) => p.id === chat.project)?.name || 'project') + ' memory');
  const retry = (i: number) => { const lastUser = (messages || []).slice(0, i).reverse().find((x) => x.role === 'user'); if (lastUser) send(lastUser.text); };
  const dropPerm = (rid: string) => setState((s) => ({ permissions: { ...s.permissions, [chat.id]: (s.permissions[chat.id] || []).filter((p) => p.id !== rid) } }));
  const reply = async (rid: string, decision: 'once' | 'always' | 'reject') => {
    try {
      await post(`${base}/permission/${rid}`, { decision });
      dropPerm(rid);
      toast(decision === 'reject' ? 'Rejected' : 'Allowed');
    } catch (e) {
      // 409 means the engine already settled or dropped this request; refresh instead of leaving a dead card.
      dropPerm(rid);
      if ((e as { status?: number }).status === 409) {
        toast('That request had already expired — the agent has moved on', 'error');
        void loadPermissions(chat.id);
      } else toast((e as Error).message || 'Could not answer that request', 'error');
    }
  };

  return (
    <div style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column' }}>
      <div ref={menuRef} className="row" style={{ gap: 10, padding: '10px 20px', borderBottom: '1px solid var(--bg2)', position: 'relative' }}>
        {renaming === null ? <b className="ellipsis" style={{ fontSize: 14, maxWidth: 360, minWidth: 60, flex: '0 1 auto' }}>{chat.title}</b> : (
          <form className="row" onSubmit={(e) => { e.preventDefault(); saveRename(); }}><input className="inp" autoFocus value={renaming} onChange={(e) => setRenaming(e.target.value)} style={{ height: 30, width: 320, maxWidth: '100%' }} aria-label="Chat title" onKeyDown={(e) => e.key === 'Escape' && setRenaming(null)} /><button type="submit" className="btn btn-p" style={{ height: 30 }}>Save</button></form>
        )}
        <button type="button" className="chip chat-proj" onClick={() => go('project', { projId: chat.project, ptab: 'chats' })}>▤ {projPath}</button>
        <button type="button" className="chip mono chat-sid" onClick={copyAttach} title="opencode session — copy the TUI command" style={{ fontSize: 11 }}><Icon name="terminal" size={12} /> {chat.sid}{chat.imported ? ' · imported' : ''}</button>
        {nArts > 0 && <span className="muted" style={{ fontSize: 12 }}>{nArts} artifact{nArts === 1 ? '' : 's'}</span>}
        <span className="grow" />
        <button type="button" className="btn" onClick={() => (mapOn ? setState({ mapOpen: false, mapWide: false }) : setState({ mapOpen: true, artId: null, runId: null }))} aria-pressed={mapOn} title="The project’s mind map, beside the chat" style={{ height: 30 }}><Icon name="map" size={15} /> Map</button>
        <button type="button" className="btn" onClick={() => (readsShown ? setState({ readsOpen: false }) : setState({ readsOpen: true, mapOpen: false, mapWide: false, artId: null, runId: null }))} aria-pressed={readsShown} title="The memory and vault notes this session read" style={{ height: 30 }}><Icon name="memory" size={15} /> Reads</button>
        <button type="button" className="btn" onClick={() => (browserOn && !readsOn ? setState({ browser: null, browserWide: false }) : openBrowser())} aria-pressed={browserOn} title="A browser beside the chat — run the dev server or open any page" style={{ height: 30 }}><Icon name="panel" size={15} /> Browser</button>
        <button ref={optsBtn} type="button" className="btn" onClick={() => setMenu(!menu)} aria-expanded={menu} aria-label="Chat options" style={{ height: 30, width: 30, padding: 0, justifyContent: 'center' }}><Icon name="more" /></button>
        {menu && (
          <div className="menu rise menu-sheet" style={menuPos}>
            <button type="button" className="navrow" onClick={() => { setMenu(false); setRenaming(chat.title); }}>Rename</button>
            <button type="button" className="navrow" onClick={copyAttach}>Open in opencode TUI</button>
            <button type="button" className="navrow" onClick={fork}>Fork session</button>
            <span className="muted" style={{ fontSize: 11, padding: '6px 8px 2px' }}>Move to project</span>
            {flatProjects(snap.projects).map(({ project: p, depth }) => <button key={p.id} type="button" className="navrow" aria-current={p.id === chat.project ? 'true' : 'false'} onClick={() => p.id !== chat.project && move(p.id, p.name)} style={{ paddingLeft: 8 + depth * 14 }}>▤ {p.name}</button>)}
            <button type="button" className="navrow" onClick={askDelete} style={{ color: 'var(--error-text)' }}>Delete chat…</button>
          </div>
        )}
      </div>

      <div ref={scroller} style={{ flex: '1 1 auto', overflowY: 'auto', padding: '24px 0' }}>
        <div style={{ maxWidth: 760, margin: '0 auto', padding: '0 28px', display: 'flex', flexDirection: 'column', gap: 18 }}>
          {(messages || []).map((m, i) => [
            ...notes.filter((x) => x.at <= m.createdAt && (i === 0 || x.at > (messages || [])[i - 1].createdAt)).map((x) => <NoteCard key={x.id} note={x} onClose={() => setNotes((all) => all.filter((y) => y.id !== x.id))} />),
            m.run ? <div key={m.id} className={fresh(m) ? 'rise' : undefined}><RunCard runId={m.run} text={m.text} /></div>
            : m.role === 'user' ? (
            <div key={m.id} className={fresh(m) ? 'rise' : undefined} style={{ alignSelf: 'flex-end', maxWidth: '80%', padding: '11px 14px', borderRadius: 14, background: 'var(--bg2)', whiteSpace: 'pre-wrap' }}>{m.text}</div>
          ) : (
            <div key={m.id} className={fresh(m) ? 'rise' : undefined} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              {!!m.tools?.length && <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                {m.tools.map((t, j) => { const k = m.id + j; const o = !!toolOpen[k]; return (
                  <div key={k}><button type="button" className="link mono" onClick={() => setToolOpen({ ...toolOpen, [k]: !o })} style={{ fontSize: 12, color: 'var(--muted)', textAlign: 'left' }}><span style={{ color: t.status === 'error' ? 'var(--error-text)' : 'var(--success-text)', display: 'inline-flex', verticalAlign: '-2px' }}><Icon name={t.status === 'error' ? 'x' : 'check'} size={13} stroke={2.25} /></span> {t.name} <span style={{ color: 'var(--dim)', display: 'inline-flex', verticalAlign: '-2px' }}><Icon name={o ? 'down' : 'right'} size={12} /></span></button>
                    {o && <pre className="pre rise" style={{ margin: '4px 0 4px 18px', maxHeight: 220 }}>{(t.input ? 'input ' + t.input + '\n' : '') + (t.output || 'ok')}</pre>}</div>
                ); })}
              </div>}
              {m.signIn && i === (messages || []).length - 1
                ? <SignInCard sub={m.signIn} agentName={snap.workspace.agents.find((a) => a.id === chat.agent)?.name || chat.agent} onSignedIn={() => retry(i)} />
                : m.error ? <div style={{ color: 'var(--error-text)' }}>{m.error}</div> : <Markdown text={m.text || '(no text)'} />}
              {!!m.sources?.length && <div className="row" style={{ flexWrap: 'wrap', gap: 6 }}><span className="muted" style={{ fontSize: 11 }}>Sources</span>{m.sources.map((s) => <a key={s.url} href={s.url} target="_blank" rel="noreferrer noopener" className="chip" style={{ height: 22, fontSize: 11, textDecoration: 'none' }}>🌐 {s.title}</a>)}</div>}
              {m.artifact && <ArtifactCard id={m.artifact} />}
              <div className="row" style={{ gap: 6 }}>
                <button type="button" className="chip" onClick={() => navigator.clipboard.writeText(m.text).then(() => toast('Copied to clipboard'), () => toast('Copy failed', 'error'))} style={{ height: 24, fontSize: 11 }}>Copy</button>
                <button type="button" className="chip" onClick={() => retry(i)} disabled={!!live} style={{ height: 24, fontSize: 11 }}>Retry</button>
                <button type="button" className="chip" onClick={() => remember(m)} style={{ height: 24, fontSize: 11 }}>Remember this</button>
                {(() => { const u = localUrlIn(m.text) || localUrlIn((m.tools || []).map((t) => t.output || '').join('\n')); return u ? <button type="button" className="chip mono" onClick={() => openBrowser(u)} title="Open beside the chat" style={{ height: 24, fontSize: 11 }}><Icon name="panel" size={12} /> Preview {u.replace(/^https?:\/\//, '').replace(/\/$/, '')}</button> : null; })()}
                {m.model && <span className="muted mono" style={{ fontSize: 10.5, marginLeft: 'auto' }}>{m.model}</span>}
              </div>
            </div>
          )])}
          {notes.filter((x) => !messages?.length || x.at > messages[messages.length - 1].createdAt).map((x) => <NoteCard key={x.id} note={x} onClose={() => setNotes((all) => all.filter((y) => y.id !== x.id))} />)}
          {!!(perms || []).length && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {(perms || []).map((p) => (
                <div key={p.id} className="card rise" style={{ padding: '10px 12px', display: 'flex', flexDirection: 'column', gap: 8, borderColor: 'var(--warning)' }}>
                  <span style={{ fontSize: 13 }}><b>{chat.agent === 'opencode' ? 'opencode' : snap.workspace.agents.find((a) => a.id === chat.agent)?.name || chat.agent}</b> asks to {p.title}</span>
                  {p.detail && <span className="mono muted" style={{ fontSize: 11.5 }}>{p.detail}</span>}
                  <span className="row"><button className="btn btn-p" onClick={() => reply(p.id, 'once')}>Allow once</button><button className="btn" onClick={() => reply(p.id, 'always')}>Always allow</button><button className="btn btn-d" onClick={() => reply(p.id, 'reject')}>Reject</button></span>
                </div>
              ))}
            </div>
          )}
          {live && <LiveReply chatId={chat.id} scroller={scroller} following={following} />}
          {messages && !messages.length && !live && <span className="muted" style={{ textAlign: 'center', paddingTop: 80 }}>New chat in {projPath}. Ask anything — it starts with this project’s instructions and memory.</span>}
          {!messages && <span className="muted pulse" style={{ textAlign: 'center', paddingTop: 80 }}>Loading the session…</span>}
        </div>
      </div>

      <div style={{ padding: '0 28px 18px', maxWidth: 760, width: '100%', margin: '0 auto', position: 'relative' }}>
        {sheet && <ActionSheet sheet={sheet} onClose={closeSheet} onRun={(cmd) => { setSheet(null); void runSlash(cmd); }} onGraph={(id, dry) => void runGraph(id, dry)} />}
        {showCmds && <CommandMenu items={cmdItems} active={Math.min(cmdIdx, cmdItems.length - 1)} agentName={agentName} onHover={setCmdIdx} onPick={pickCmd} />}
        {files.open && <FileMenu items={files.items} active={files.active} onHover={files.setActive} onPick={files.pick} style={{ left: 28, right: 28, bottom: '100%', marginBottom: 6 }} />}
        {chat.yolo && <div className="rise" style={{ marginBottom: 6, fontSize: 11.5, color: 'var(--warning-text)', display: 'flex', alignItems: 'center', gap: 6 }}><Icon name="zap" size={13} /> Full access is on for this chat — the agent can run tools and edit files without asking.</div>}
        <div className="card" style={{ borderRadius: 14, padding: '10px 12px', display: 'flex', flexDirection: 'column', gap: 8 }}>
          <textarea ref={composerInput} rows={2} aria-label="Message" aria-autocomplete="list" placeholder={artId ? 'Ask for a change to the open artifact…' : 'Reply, ask for a doc or deck — / for commands, @ for files'} value={draft} onSelect={(e) => files.track(e.currentTarget)} onChange={(e) => { setDraft(e.target.value); files.track(e.target); setCmdIdx(0); setCmdHidden(false); }} onKeyDown={onKey} style={{ resize: 'none', border: 0, outline: 'none', background: 'transparent', color: 'var(--text)', font: 'inherit' }} />
          <div className="row" style={{ flexWrap: 'wrap' }}>
            <ModelPicker agent={chat.agent} model={model} effort={chat.effort || ''} lockAgent={(messages?.length || 0) > 0} onPick={pick} open={pickerOpen} onOpenChange={setPickerOpen} />
            <button type="button" className="chip" onClick={() => go('memory')} style={{ fontSize: 11 }}><Icon name="memory" size={13} /> {{ facts: 'Facts', graph: 'Graphify', both: 'Graphify + facts', mcp: 'Custom MCP' }[ws.memory]}</button>
            <button type="button" className="chip" onClick={() => go('settings', { setPage: 'connectors' })} style={{ fontSize: 11 }}><Icon name="plug" size={13} /> {ws.mcp.filter((m) => m.on).length} connectors</button>
            <button type="button" className="chip" aria-pressed={webOn} onClick={toggleWeb} style={{ fontSize: 11 }}><Icon name="globe" size={13} /> Web {webOn ? 'on' : 'off'}</button>
            <button type="button" className="chip" aria-pressed={!!chat.yolo} onClick={toggleYolo} title={chat.yolo ? 'Full permissions: tools run without asking in this chat. Click to ask first again.' : 'Ask before tools run. Click to give this chat full permissions (YOLO).'} style={{ fontSize: 11, ...(chat.yolo ? { borderColor: 'var(--warning)', color: 'var(--warning-text)' } : {}) }}>{chat.yolo ? <><Icon name="zap" size={13} /> Full access</> : <><Icon name="lock" size={13} /> Ask first</>}</button>
            <span className="grow" />
            {live ? <button type="button" className="btn btn-d" onClick={stop} style={{ height: 32 }}><Icon name="stop" size={14} /> Stop</button> : <button type="button" className="btn btn-p" onClick={() => send()} aria-label="Send" style={{ width: 32, padding: 0, justifyContent: 'center' }}><Icon name="send" size={16} stroke={2.25} /></button>}
          </div>
        </div>
      </div>
    </div>
  );
}

function ArtifactCard({ id }: { id: string }) {
  const a = useStore((s) => s.snap?.artifacts.find((x) => x.id === id));
  if (!a) return null;
  return (
    <button type="button" className="tile" onClick={() => setState({ artId: a.id })} style={{ flexDirection: 'row', alignItems: 'center', gap: 12, maxWidth: 440 }}>
      <span className="mono" style={{ fontSize: 10.5, padding: '3px 6px', borderRadius: 4, background: 'var(--bg3)', color: 'var(--accent-p-text)' }}>{a.type.toUpperCase()}</span>
      <span className="grow" style={{ display: 'flex', flexDirection: 'column', textAlign: 'left' }}><b style={{ fontSize: 13 }}>{a.title}</b><span className="muted" style={{ fontSize: 12 }}>{a.type} · v{a.version}</span></span>
      <span style={{ color: 'var(--accent-p-text)', fontSize: 12 }}>Open</span>
    </button>
  );
}

function effModel(snap: NonNullable<ReturnType<typeof getState>['snap']>, pid: string): string {
  let p = snap.projects.find((x) => x.id === pid);
  for (let i = 0; p && i < 32; i++) { if (p.model) return p.model; p = snap.projects.find((x) => x.id === p!.parent); }
  return '';
}
