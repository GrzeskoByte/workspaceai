// New artifact: Open Design's pipeline (R-15) — brief → questions → direction → design system → generate →
// critique → handoff → memory. Model steps run in one pipeline chat in the chosen project; the artifact is made
// with the artifact tool, so Open Design saves and lints every version.
import { useEffect, useState } from 'react';
import { useStore, go, act, refreshSnap, wsId, setState, toast } from '../store.ts';
import { get, post, download } from '../api.ts';
import { flatProjects, projectPathName } from '../../../../packages/shared/src/util.ts';
import type { Artifact, ArtifactType } from '../../../../packages/shared/src/types.ts';
import { Markdown } from '../ui/Markdown.tsx';

const STAGES = ['Brief', 'Questions', 'Direction', 'Design system', 'Generate', 'Critique', 'Handoff', 'Memory'] as const;
const OUTPUTS: { type: ArtifactType; label: string; note: string }[] = [
  { type: 'Doc', label: 'Doc', note: 'report, runbook, memo' }, { type: 'Deck', label: 'Deck', note: 'slides' },
  { type: 'Prototype', label: 'Prototype', note: 'a working HTML page' }, { type: 'Sheet', label: 'Sheet', note: 'a table of data' },
  { type: 'LiveArtifact', label: 'Live artifact', note: 'HTML that re-renders from data' }, { type: 'Image', label: 'Image', note: 'an SVG illustration' },
  { type: 'Video', label: 'Video', note: 'storyboard and shot list' }, { type: 'DesignSystem', label: 'Design system', note: 'a DESIGN.md' },
];
const TEMPLATES: { label: string; type: ArtifactType; title: string; brief: string }[] = [
  { label: 'Landing page', type: 'Prototype', title: 'Beta landing page', brief: 'A landing page announcing a private beta: headline, how it works in three steps, and a sign-up call to action.' },
  { label: 'Teaser deck', type: 'Deck', title: 'Teaser deck', brief: 'A 6-slide teaser deck: problem, solution, demo, roadmap, the ask.' },
  { label: 'Runbook', type: 'Doc', title: 'Runbook', brief: 'A step-by-step runbook with owners, checks and rollback.' },
  { label: 'Budget sheet', type: 'Sheet', title: 'Budget', brief: 'A budget sheet with items, estimates and actuals.' },
];
interface DS { id: string; title: string; summary: string; swatches: string[]; source: string }
interface Direction { name: string; summary: string; tone: string }
interface Critique { scores: Record<string, number>; issues: { role: string; issue: string; fix: string }[]; composite: number; threshold: number; passed: boolean; roles: { id: string; name: string; scores: string; weight: number }[] }

export function BriefView() {
  const snap = useStore((s) => s.snap)!;
  const [stage, setStage] = useState(0);
  const [type, setType] = useState<ArtifactType>('Doc');
  const projId = useStore((s) => s.projId);
  const [project, setProject] = useState(projId || snap.workspace.defaultProject || snap.projects[0]?.id || '');
  const [title, setTitle] = useState('');
  const [brief, setBrief] = useState('');
  const [chat, setChat] = useState<string | null>(null);
  const [questions, setQuestions] = useState<string[]>([]);
  const [answers, setAnswers] = useState<string[]>([]);
  const [directions, setDirections] = useState<Direction[]>([]);
  const [direction, setDirection] = useState<Direction | null>(null);
  const [systems, setSystems] = useState<DS[]>([]);
  const [system, setSystem] = useState('claude-warm-editorial');
  const [dsQuery, setDsQuery] = useState('');
  const [art, setArt] = useState<Artifact | null>(null);
  const [busy, setBusy] = useState('');
  const base = `/api/ws/${wsId()}/pipeline`;

  useEffect(() => { get<{ systems: DS[] }>(`/api/ws/${wsId()}/design-systems`).then((r) => setSystems(r.systems)).catch(() => {}); }, []);
  const live = art ? snap.artifacts.find((a) => a.id === art.id) || art : null;
  const sys = systems.find((d) => d.id === system);
  const run = async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => { setBusy(label); try { return await act(fn); } finally { setBusy(''); } };

  const start = async (ask: boolean) => {
    const r = await run('Opening the pipeline chat…', () => post<{ chat: string }>(base, { project, type, title: title || type, brief }));
    if (!r) return;
    setChat(r.chat); await refreshSnap();
    if (!ask) { setQuestions([]); await loadDirections(r.chat, []); return; }
    const q = await run('Thinking of questions…', () => post<{ questions: string[] }>(`${base}/${r.chat}/questions`, { type, title, brief }));
    if (q) { setQuestions(q.questions); setAnswers(q.questions.map(() => '')); setStage(1); }
  };
  const loadDirections = async (c = chat, qa = questions.map((q, i) => ({ q, a: answers[i] }))) => {
    const d = await run('Sketching three directions…', () => post<{ directions: Direction[] }>(`${base}/${c}/directions`, { type, title, brief, answers: qa }));
    if (d) { setDirections(d.directions); setDirection(d.directions[0] || null); setStage(2); }
  };
  const generate = async () => {
    setStage(4);
    const a = await run(`Generating with ${sys?.title || 'the design system'}…`, () => post<Artifact>(`${base}/${chat}/generate`, { type, title: title || type, direction, designSystem: system, designSystemTitle: sys?.title }));
    if (a) { setArt(a); await refreshSnap(); toast(`${a.type} saved · v${a.version}`); } else setStage(3);
  };

  const done = (i: number) => (i < stage ? '✓' : String(i + 1));
  return (
    <div className="split" style={{ flex: '1 1 auto', display: 'flex', minHeight: 0, minWidth: 0 }}>
      <nav aria-label="Pipeline" className="side-col" style={{ flex: '0 0 200px', borderRight: '1px solid var(--bg2)', padding: 14, display: 'flex', flexDirection: 'column', gap: 2 }}>
        <span className="lbl" style={{ paddingBottom: 6 }}>New artifact</span>
        {STAGES.map((s, i) => (
          <button key={s} type="button" className="navrow" aria-current={i === stage ? 'step' : 'false'} disabled={i > stage && !(art && i >= 4)} onClick={() => setStage(i)} style={{ gap: 10 }}>
            <span style={{ width: 20, height: 20, borderRadius: '50%', border: `2px solid ${i <= stage ? 'var(--accent-p)' : 'var(--line2)'}`, background: i < stage ? 'var(--accent-p-fill)' : 'transparent', color: 'var(--on-accent)', fontSize: 10, fontWeight: 700, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>{done(i)}</span>{s}
          </button>
        ))}
        <span className="grow" />
        {chat && <button type="button" className="link" style={{ fontSize: 12, textAlign: 'left' }} onClick={() => go('chat', { chatId: chat, projId: project })}>Pipeline transcript 💬</button>}
        <span className="muted" style={{ fontSize: 11 }}>Open Design pipeline · {projectPathName(snap.projects, project)}</span>
      </nav>

      <div className="page-pad" style={{ flex: '1 1 auto', minWidth: 0, overflowY: 'auto', padding: '26px 30px', display: 'flex', flexDirection: 'column', gap: 14 }}>
        {busy && <div className="card row" style={{ padding: '10px 14px' }}><span className="dot pulse" style={{ background: 'var(--accent-p)' }} /><span>{busy}</span></div>}

        {stage === 0 && <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 14, maxWidth: 820 }}>
          <h1 className="disp" style={{ fontSize: 30 }}>New artifact brief</h1>
          <span className="lbl">Output</span>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))', gap: 8 }}>{OUTPUTS.map((o) => <button key={o.type} type="button" className="opt" aria-pressed={type === o.type} onClick={() => setType(o.type)} style={{ padding: '10px 12px' }}><b style={{ fontSize: 13 }}>{o.label}</b><span className="muted" style={{ fontSize: 11 }}>{o.note}</span></button>)}</div>
          <div className="row" style={{ flexWrap: 'wrap' }}><span className="lbl">Start from a template</span>{TEMPLATES.map((t) => <button key={t.label} type="button" className="chip" onClick={() => { setType(t.type); setTitle(t.title); setBrief(t.brief); }}>{t.label}</button>)}</div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))', gap: 10 }}>
            <label className="field">Title<input className="inp" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Beta landing page" /></label>
            <label className="field">Project — artifacts always belong to one<select className="inp" value={project} onChange={(e) => setProject(e.target.value)}>{flatProjects(snap.projects).map(({ project: p, depth }) => <option key={p.id} value={p.id}>{' '.repeat(depth * 2)}{p.name}</option>)}</select></label>
          </div>
          <label className="field">Brief<textarea className="inp" rows={5} value={brief} onChange={(e) => setBrief(e.target.value)} placeholder="What is it, who is it for, and what must it include?" /></label>
          <span className="muted" style={{ fontSize: 12 }}>It follows {projectPathName(snap.projects, project)}’s instructions and memory, like a chat there.</span>
          <div className="row"><span className="grow" /><button type="button" className="btn" disabled={!brief.trim() || !!busy} onClick={() => void start(false)}>Skip questions</button><button type="button" className="btn btn-p" disabled={!brief.trim() || !!busy} onClick={() => void start(true)}>Ask me questions</button></div>
        </div>}

        {stage === 1 && <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 12, maxWidth: 760 }}>
          <h1 className="disp" style={{ fontSize: 30 }}>A few questions</h1>
          {questions.map((q, i) => <label key={i} className="field">{q}<input className="inp" value={answers[i] || ''} onChange={(e) => setAnswers(answers.map((a, j) => (j === i ? e.target.value : a)))} /></label>)}
          <div className="row"><span className="grow" /><button type="button" className="btn btn-p" disabled={!!busy} onClick={() => void loadDirections()}>Pick a direction</button></div>
        </div>}

        {stage === 2 && <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 12, maxWidth: 900 }}>
          <h1 className="disp" style={{ fontSize: 30 }}>Pick a direction</h1>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))', gap: 10 }}>{directions.map((d) => <button key={d.name} type="button" className="opt" aria-pressed={direction?.name === d.name} onClick={() => setDirection(d)}><b>{d.name}</b><span style={{ fontSize: 13 }}>{d.summary}</span><span className="muted" style={{ fontSize: 12 }}>Tone · {d.tone}</span></button>)}</div>
          <div className="row"><button type="button" className="btn" disabled={!!busy} onClick={() => void loadDirections()}>Three more</button><span className="grow" /><button type="button" className="btn btn-p" disabled={!direction} onClick={() => setStage(3)}>Design system</button></div>
        </div>}

        {stage === 3 && <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div className="row"><h1 className="disp grow" style={{ fontSize: 30 }}>Design system</h1><input className="inp" value={dsQuery} onChange={(e) => setDsQuery(e.target.value)} placeholder={`Search ${systems.length} systems`} aria-label="Search design systems" style={{ width: 240 }} /></div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 8, maxHeight: 460, overflowY: 'auto' }}>
            {systems.filter((d) => !dsQuery || (d.title + ' ' + d.summary).toLowerCase().includes(dsQuery.toLowerCase())).slice(0, 80).map((d) => (
              <button key={d.id} type="button" className="opt" aria-pressed={system === d.id} onClick={() => setSystem(d.id)} style={{ padding: '10px 12px' }}>
                <span className="row" style={{ gap: 3 }}>{d.swatches.slice(0, 5).map((c, i) => <span key={i} style={{ width: 16, height: 16, borderRadius: 4, background: c, border: '1px solid var(--line)' }} />)}</span>
                <b style={{ fontSize: 13 }}>{d.title}</b><span className="muted ellipsis" style={{ fontSize: 11, maxWidth: '100%' }}>{d.source === 'harness' ? 'harness default' : d.summary}</span>
              </button>
            ))}
          </div>
          <div className="row"><span className="muted grow" style={{ fontSize: 12 }}>Its DESIGN.md is given to the agent as the brand contract.</span><button type="button" className="btn btn-p" disabled={!!busy || !chat} onClick={() => void generate()}>Generate</button></div>
        </div>}

        {stage >= 4 && <Generated art={live} chat={chat} type={type} direction={direction} sys={sys} busy={busy} run={run} setArt={setArt} stage={stage} setStage={setStage} />}
      </div>
    </div>
  );
}

function Generated({ art, chat, type, direction, sys, busy, run, setArt, stage, setStage }: { art: Artifact | null; chat: string | null; type: ArtifactType; direction: Direction | null; sys?: DS; busy: string; run: <T>(l: string, f: () => Promise<T>) => Promise<T | undefined>; setArt: (a: Artifact) => void; stage: number; setStage: (n: number) => void }) {
  const [content, setContent] = useState('');
  const [tab, setTab] = useState<'plan' | 'crit' | 'tweaks' | 'ship'>('plan');
  const [device, setDevice] = useState<'desktop' | 'mobile'>('desktop');
  const [crit, setCrit] = useState<Critique | null>(null);
  const [tweak, setTweak] = useState('');
  const [page, setPage] = useState('');
  const [formats, setFormats] = useState<{ label: string; ext: string; format: string }[]>([]);
  const base = `/api/ws/${wsId()}/pipeline/${chat}`;
  useEffect(() => {
    if (!art) return;
    get<{ content: string; format: string; html: string }>(`/api/ws/${wsId()}/artifacts/${art.id}/content`).then((r) => { setContent(r.content); setPage(r.format === 'markdown' && art.type !== 'Deck' ? '' : r.html); }).catch(() => {});
    get<{ formats: { label: string; ext: string; format: string }[] }>(`/api/ws/${wsId()}/artifacts/${art.id}/exports`).then((r) => setFormats(r.formats)).catch(() => {});
  }, [art?.id, art?.version]);
  const od = art?.versions[art.versions.length - 1]?.od;
  // HTML versions and every deck show the page Open Design stores; Markdown documents render here.
  const html = !!page;
  if (!art) return <p className="muted">{busy || 'Generation did not finish. Go back to Design system and try again.'}</p>;
  const critique = async () => { setTab('crit'); setStage(Math.max(stage, 5)); const c = await run('The design jury is reviewing…', () => post<Critique>(`${base}/critique`, { artifact: art.id })); if (c) setCrit(c); };
  const fix = async (issues: { role: string; issue: string; fix: string }[]) => { const a = await run('Applying fixes as a new version…', () => post<Artifact>(`${base}/fix`, { artifact: art.id, issues })); if (a) { setArt(a); await refreshSnap(); setCrit(null); toast(`Saved v${a.version}`); } };
  return (
    <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div className="row" style={{ flexWrap: 'wrap' }}>
        <span className="mono" style={{ fontSize: 10.5, padding: '3px 6px', borderRadius: 4, background: 'var(--bg3)', color: 'var(--accent-p-text)' }}>{art.type.toUpperCase()}</span>
        <h1 className="disp grow" style={{ fontSize: 28 }}>{art.title}</h1>
        <span className="chip mono" style={{ fontSize: 11 }}>v{art.version}</span>
        <span className="row muted" style={{ fontSize: 12, gap: 6 }}><span className="dot" style={{ background: od ? (od.lint.length ? 'var(--warning)' : 'var(--success)') : 'var(--dim)' }} />{od ? `Open Design · lint ${od.lint.length ? od.lint.length + ' findings' : 'clean'}` : 'built-in store'}</span>
        {html && <span className="row" role="group" aria-label="Device"><button type="button" className="chip" aria-pressed={device === 'desktop'} onClick={() => setDevice('desktop')}>Desktop</button><button type="button" className="chip" aria-pressed={device === 'mobile'} onClick={() => setDevice('mobile')}>Mobile</button></span>}
      </div>
      <div className="row" style={{ alignItems: 'flex-start', flexWrap: 'wrap', gap: 14 }}>
        <div className="card" style={{ flex: '1 1 480px', minWidth: 0, padding: 12, display: 'flex', justifyContent: 'center', background: 'var(--bg0)' }}>
          {html ? <iframe title={art.title} sandbox="allow-scripts" srcDoc={page} style={{ width: device === 'mobile' ? 390 : '100%', ...(art.type === 'Deck' ? { aspectRatio: '16 / 9', height: 'auto' } : { height: 560 }), border: '1px solid var(--line)', borderRadius: 10, background: 'white', transition: 'width .3s ease' }} />
            : <div style={{ width: '100%', maxHeight: 560, overflowY: 'auto', padding: '6px 10px' }}><Markdown text={art.type === 'Sheet' ? '```\n' + content + '\n```' : content} /></div>}
        </div>
        <div className="card" style={{ flex: '0 1 340px', minWidth: 280, padding: 12, display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div role="tablist" className="row" style={{ borderBottom: '1px solid var(--bg2)', gap: 2 }}>{([['plan', 'Plan'], ['crit', 'Critique'], ['tweaks', 'Tweaks'], ['ship', 'Export']] as const).map(([k, l]) => <button key={k} type="button" role="tab" className="tab" aria-selected={tab === k} onClick={() => setTab(k)}>{l}</button>)}</div>
          {tab === 'plan' && <div style={{ display: 'flex', flexDirection: 'column', gap: 6, fontSize: 13 }}>
            <span><b>Direction</b> · {direction?.name || '—'}</span><span className="muted">{direction?.summary}</span>
            <span><b>Design system</b> · {sys?.title || art.designSystem}</span>
            <span><b>Versions</b> · {art.versions.map((v) => 'v' + v.v + ' ' + v.note).join(' · ')}</span>
            <button type="button" className="btn btn-p" onClick={() => void critique()} disabled={!!busy}>Run the critique</button>
          </div>}
          {tab === 'crit' && <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {!crit && <button type="button" className="btn btn-p" onClick={() => void critique()} disabled={!!busy}>Run the critique</button>}
            {crit && <>
              <span className="row"><b className="grow">Composite {crit.composite}/10</b><span className="chip" style={{ borderColor: crit.passed ? 'var(--success)' : 'var(--warning)' }}>{crit.passed ? 'passes' : `below ${crit.threshold}`}</span></span>
              {crit.roles.map((r) => { const v = Number(crit.scores[r.id]) || 0; return (
                <div key={r.id} title={r.scores} style={{ fontSize: 12 }}><span className="row"><span className="grow">{r.name} <span className="muted">· weight {r.weight}</span></span><b>{v}</b></span><div style={{ height: 5, borderRadius: 3, background: 'var(--bg3)' }}><div style={{ width: v * 10 + '%', height: 5, borderRadius: 3, background: v >= 8 ? 'var(--success)' : v >= 6 ? 'var(--warning)' : 'var(--error)', transition: 'width .5s ease' }} /></div></div>
              ); })}
              {crit.issues.map((i, n) => <div key={n} style={{ fontSize: 12 }}><b>{i.role}</b> · {i.issue}<br /><span className="muted">→ {i.fix}</span></div>)}
              {!!crit.issues.length && <button type="button" className="btn btn-p" disabled={!!busy} onClick={() => void fix(crit.issues)}>Apply fixes</button>}
            </>}
          </div>}
          {tab === 'tweaks' && <form style={{ display: 'flex', flexDirection: 'column', gap: 8 }} onSubmit={(e) => { e.preventDefault(); if (tweak.trim()) void fix([{ role: 'tweak', issue: 'Requested change', fix: tweak }]).then(() => setTweak('')); }}>
            <div className="row" style={{ flexWrap: 'wrap' }}>{(html ? ['Split hero layout', 'Compact density', 'Use the accent for the main button only', 'Add a mobile menu'] : ['Shorter', 'Add a summary at the top', 'More specific numbers', 'Friendlier tone']).map((t) => <button key={t} type="button" className="chip" onClick={() => setTweak(t)}>{t}</button>)}</div>
            <textarea className="inp" rows={3} value={tweak} onChange={(e) => setTweak(e.target.value)} placeholder="Describe a change" aria-label="Tweak" />
            <button type="submit" className="btn btn-p" disabled={!tweak.trim() || !!busy}>Apply as a new version</button>
          </form>}
          {tab === 'ship' && <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {formats.map((f) => <button key={f.format} type="button" className="navrow" onClick={() => void download(`/api/ws/${wsId()}/artifacts/${art.id}/export?format=${f.format}`).then(() => toast('Exported ' + art.title + f.ext), (e) => toast(e.message, 'error'))}>{f.label}<span className="mono muted" style={{ marginLeft: 'auto', fontSize: 11 }}>{f.ext}</span></button>)}
          </div>}
        </div>
      </div>
      <div className="row" style={{ flexWrap: 'wrap' }}>
        <button type="button" className="btn" onClick={() => { setState({ artId: art.id }); go('artifacts', { artId: art.id, projId: art.project }); }}>Save to the library</button>
        <span className="muted" style={{ fontSize: 12 }}>Keeps it in the project’s artifact library, versioned.</span>
        <span className="grow" />
        <button type="button" className="btn" disabled={!!busy} onClick={() => void run('Saving design decisions…', () => post<{ facts: unknown[] }>(`${base}/remember`, { artifact: art.id, direction, designSystemTitle: sys?.title })).then((r) => { if (r) { setStage(7); void refreshSnap(); toast(`${r.facts.length} memory suggestions — approve them in Memory`); } })}>Remember direction and tone</button>
        <button type="button" className="btn btn-p" disabled={!!busy} onClick={() => void run('Handing off to a project chat…', () => post<{ chat: string; folder: string }>(`${base}/handoff`, { artifact: art.id })).then(async (r) => { if (r) { setStage(6); await refreshSnap(); go('chat', { chatId: r.chat, projId: art.project }); } })}>Continue as code</button>
      </div>
    </div>
  );
}

export function GenerateView() { return <BriefView />; }
