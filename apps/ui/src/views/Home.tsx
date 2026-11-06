// Home: a new chat always goes to a project (R-10); mode chips; needs-you, continue, coming up.
import { useRef } from 'react';
import { useStore, setState, go, act, wsId, refreshSnap, toast } from '../store.ts';
import { post } from '../api.ts';
import { projectPathName } from '../../../../packages/shared/src/util.ts';
import type { Chat } from '../../../../packages/shared/src/types.ts';
import { ProjectPicker, ModelPicker, pickModel } from './Composer.tsx';
import { openChat } from '../shell/Shell.tsx';
import { describeNext } from '../../../../packages/shared/src/schedule.ts';
import { FileMenu, useFileMention } from './FileMention.tsx';
import { Icon } from '../ui/Icon.tsx';
import { ChatActivity } from '../ui/ChatActivity.tsx';

const MODES: [string, string, string][] = [['chat', 'Chat', 'Ask anything — it stays in the project you pick'], ['doc', 'Doc', 'Describe the document you need'], ['deck', 'Deck', 'What is the deck about, and who is it for?'], ['proto', 'Prototype', 'Describe the screen or page'], ['research', 'Research', 'What should I find out?'], ['loop', 'Loop', 'Describe a task to repeat until it’s done']];
const MODE_PREFIX: Record<string, string> = {
  doc: 'Create a document artifact — read Open Design’s reference with artifact_guide, then save it with artifact_create — for this: ',
  deck: 'Create a slide deck artifact — read Open Design’s reference with artifact_guide, then save it with artifact_create — for this: ',
  proto: 'Create an interactive HTML prototype artifact — read Open Design’s reference with artifact_guide (and load the skill that fits), then save it with artifact_create — for this: ',
  research: 'Research this using web search where available, and cite the pages you used: ',
};

export function Home() {
  const snap = useStore((s) => s.snap)!;
  const draft = useStore((s) => s.draft);
  const mode = useStore((s) => s.mode);
  const composeProj = useStore((s) => s.composeProj);
  const projId = useStore((s) => s.projId);
  const homeAgent = useStore((s) => s.homeAgent);
  const homeWeb = useStore((s) => s.homeWeb);
  const homeModel = useStore((s) => s.homeModel);
  const homeEffort = useStore((s) => s.homeEffort);
  const models = useStore((s) => s.models);
  const agentModels = useStore((s) => s.agentModels);
  const ws = snap.workspace;
  const isOrg = ws.kind === 'org';
  const pid = (composeProj && snap.projects.some((p) => p.id === composeProj) ? composeProj : null) || (projId && snap.projects.some((p) => p.id === projId) ? projId : null) || ws.defaultProject || snap.projects[0].id;
  const proj = snap.projects.find((p) => p.id === pid) || snap.projects[0];
  const composerInput = useRef<HTMLTextAreaElement>(null);
  const files = useFileMention(proj.id, draft, (text) => setState({ draft: text }), composerInput);
  const webMode = isOrg && snap.policy ? snap.policy.web.mode : ws.web.mode;
  const webOn = webMode !== 'off' && (homeWeb ?? false);
  // An agent that was switched off or disconnected (Claude Code, Grok) can't take new chats; fall back to opencode.
  const agent = [homeAgent, ws.defaultAgent].find((id) => id && ws.agents.some((a) => a.id === id && a.on)) || 'opencode';
  const model = pickModel(agentModels, agent, homeModel || '', projModel(snap, pid) || ws.defaultModel || models[0] || '');

  const send = async () => {
    const text = draft.trim();
    if (!text) { toast('Type a message first'); return; }
    if (mode === 'loop') { go('graph', { draft: '' }); toast('Opened the graph editor for this loop'); setState({ mode: 'chat' }); return; }
    const chat = await act(() => post<Chat>(`/api/ws/${wsId()}/chats`, { project: proj.id, title: text.slice(0, 60), text: (MODE_PREFIX[mode] || '') + text, agent, model: homeModel || undefined, effort: homeEffort || undefined, web: homeWeb }));
    if (!chat) return;
    setState((s) => ({ draft: '', mode: 'chat', live: { ...s.live, [chat.id]: { text: '', tools: [], messageId: '' } } }));
    await refreshSnap();
    setState((s) => ({ open: { ...s.open, [proj.id]: true } }));
    openChat(chat);
  };

  const h = new Date().getHours();
  const greeting = (h < 12 ? 'Good morning' : h < 19 ? 'Good afternoon' : 'Good evening') + '. What are we making?';
  const needs = [
    ...snap.facts.filter((f) => f.status === 'suggested').slice(0, 1).map((f) => ({ title: `Review ${snap.facts.filter((x) => x.status === 'suggested').length} memory suggestion${snap.facts.filter((x) => x.status === 'suggested').length === 1 ? '' : 's'}`, sub: f.text.slice(0, 60), go: () => go('memory') })),
    ...snap.routines.filter((r) => r.runs[0]?.status === 'waiting').map((r) => ({ title: 'Approve: ' + r.name, sub: r.runs[0].note + ' · ' + projectPathName(snap.projects, r.project), go: () => go('routines') })),
  ];
  const recent = snap.chats.slice().sort((a, b) => b.updatedAt - a.updatedAt).slice(0, 3);
  const upcoming = snap.routines.filter((r) => !r.paused).slice(0, 2);
  const suggest = [isOrg ? 'Draft a runbook for rotating the realm signing keys' : `Make a 5-slide deck about ${proj.name}`, isOrg ? 'Summarise last week’s incidents' : 'Write a one-page plan for this week', 'Build a sheet tracking my open tasks'];

  return (
    <div style={{ flex: '1 1 auto', overflowY: 'auto', display: 'flex', justifyContent: 'center' }}>
      <div style={{ width: '100%', maxWidth: 860, padding: '56px 32px 32px', display: 'flex', flexDirection: 'column', gap: 26 }}>
        <h1 className="disp rise" style={{ textAlign: 'center', fontSize: 42, lineHeight: 1.1 }}>{greeting}</h1>
        <div className="card rise" style={{ position: 'relative', borderRadius: 16, padding: 14, display: 'flex', flexDirection: 'column', gap: 12, animationDelay: '.06s' }}>
          {files.open && <FileMenu items={files.items} active={files.active} onHover={files.setActive} onPick={files.pick} style={{ left: 14, right: 14, top: '100%', marginTop: 6 }} />}
          <textarea ref={composerInput} rows={3} aria-label="Message" autoFocus placeholder={MODES.find((m) => m[0] === mode)?.[2]} value={draft} onSelect={(e) => files.track(e.currentTarget)} onChange={(e) => { setState({ draft: e.target.value }); files.track(e.target); }}
            onKeyDown={(e) => { if (files.onKey(e)) return; if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } }}
            style={{ resize: 'none', border: 0, outline: 'none', background: 'transparent', color: 'var(--text)', font: 'inherit', fontSize: 16 }} />
          <div className="row" style={{ flexWrap: 'wrap' }}>
            <ProjectPicker value={proj.id} onPick={(id) => setState({ composeProj: id })} />
            {MODES.map((m) => <button key={m[0]} type="button" className="chip" aria-pressed={mode === m[0]} onClick={() => setState({ mode: m[0] })}>{m[1]}</button>)}
            <button type="button" className="chip" aria-pressed={webOn} onClick={() => { if (webMode === 'off') { go('settings', { setPage: 'web' }); toast(`Web search is off for ${ws.name} — pick a search here`); return; } setState({ homeWeb: !webOn }); }}><Icon name="globe" size={13} /> Web {webOn ? 'on' : 'off'}</button>
            {/* Model + send stay together on the right, also when the chips wrap. */}
            <span className="row" style={{ marginLeft: 'auto', gap: 8 }}>
              <ModelPicker agent={agent} model={model} effort={homeEffort || ''} onPick={(a, m, e) => setState({ homeAgent: a, homeModel: m, homeEffort: e || null })} />
              <button type="button" className="btn btn-p" onClick={send} aria-label="Send" style={{ width: 34, padding: 0, justifyContent: 'center' }}><Icon name="send" size={16} stroke={2.25} /></button>
            </span>
          </div>
        </div>
        <div className="row" style={{ flexWrap: 'wrap', justifyContent: 'center' }}>
          {suggest.map((t) => <button key={t} type="button" className="chip" onClick={() => setState({ draft: t })}>{t}</button>)}
        </div>
        <div className="rise" style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(min(200px, 100%), 1fr))', gap: 14, animationDelay: '.12s' }}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}><span className="lbl">Needs you</span>
            {needs.map((n, i) => <button key={i} type="button" className="tile" onClick={n.go}><b>{n.title}</b><span className="muted" style={{ fontSize: 12 }}>{n.sub}</span></button>)}
            {!needs.length && <span className="muted" style={{ fontSize: 12 }}>Nothing waiting on you.</span>}
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}><span className="lbl">Continue</span>
            {recent.map((c) => <button key={c.id} type="button" className="tile" onClick={() => openChat(c)}><span className="row" style={{ gap: 6, maxWidth: '100%' }}><b className="ellipsis">{c.title}</b><ChatActivity chatId={c.id} /></span><span className="muted" style={{ fontSize: 12 }}>{projectPathName(snap.projects, c.project)}</span></button>)}
            {!recent.length && <span className="muted" style={{ fontSize: 12 }}>Your chats will show up here.</span>}
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}><span className="lbl">Coming up</span>
            {upcoming.map((r) => <button key={r.id} type="button" className="tile" onClick={() => go('routines')}><b>{r.name}</b><span className="muted" style={{ fontSize: 12 }}>{r.trigger.kind === 'schedule' ? describeNext(r.trigger.cron, r.trigger.tz) : r.trigger.event} · {r.runsOn}</span></button>)}
            <button type="button" className="tile" onClick={() => go('routines')} style={{ borderStyle: 'dashed' }}><span className="muted" style={{ fontSize: 12 }}>+ Make something run on a schedule</span></button>
          </div>
        </div>
      </div>
    </div>
  );
}

function projModel(snap: NonNullable<ReturnType<typeof import('../store.ts').getState>['snap']>, pid: string): string {
  let p = snap.projects.find((x) => x.id === pid);
  for (let i = 0; p && i < 32; i++) { if (p.model) return p.model; p = snap.projects.find((x) => x.id === p!.parent); }
  return '';
}
