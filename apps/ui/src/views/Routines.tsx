// Routines in plain words: interpretation shown before saving, week view, run history, run now, pause (R-27, R-43).
import { useMemo, useState } from 'react';
import { useStore, act, refreshSnap, wsId, go, toast } from '../store.ts';
import { get, post, patch, del } from '../api.ts';
import { describeNext, formatWhen, nextRuns, parseSchedule, localTz } from '../../../../packages/shared/src/schedule.ts';
import { flatProjects, projectPathName } from '../../../../packages/shared/src/util.ts';
import type { Routine } from '../../../../packages/shared/src/types.ts';

const DAY = 864e5;
const STATUS_COLOR: Record<string, string> = { ok: 'var(--success)', running: 'var(--info)', waiting: 'var(--warning)', error: 'var(--error)', skipped: 'var(--dim)', next: 'var(--accent-p)' };

function startOfDay(t: number): number { const d = new Date(t); d.setHours(0, 0, 0, 0); return d.getTime(); }

export function RoutinesView() {
  const snap = useStore((s) => s.snap)!;
  const app = useStore((s) => s.app)!;
  const [phrase, setPhrase] = useState('every weekday at 9');
  const [name, setName] = useState('');
  const [prompt, setPrompt] = useState('');
  const [project, setProject] = useState(snap.workspace.defaultProject || snap.projects[0]?.id || '');
  const [kind, setKind] = useState<'prompt' | 'graph' | 'artifact'>('prompt');
  const [graph, setGraph] = useState(snap.graphs[0]?.id || '');
  const [artifact, setArtifact] = useState(snap.artifacts[0]?.id || '');
  const [approval, setApproval] = useState<'ask' | 'pre-approved'>(snap.workspace.routineAskFirst ? 'ask' : 'pre-approved');
  const [runsOn, setRunsOn] = useState<Routine['runsOn']>('cloud');
  const [computer, setComputer] = useState(app.computers[0]?.id || '');
  const [ifAsleep, setIfAsleep] = useState<Routine['ifAsleep']>(snap.workspace.routineIfAsleep);
  const [notify, setNotify] = useState<string[]>(['chat']);
  const [weekStart, setWeekStart] = useState(() => startOfDay(Date.now() - ((new Date().getDay() + 6) % 7) * DAY));
  const [open, setOpen] = useState<string | null>(null);
  const tz = localTz();
  const parsed = useMemo(() => parseSchedule(phrase, { tz }), [phrase, tz]);
  const isOrg = snap.workspace.kind === 'org';

  // Week view: past runs and upcoming slots per day.
  const days = useMemo(() => Array.from({ length: 7 }, (_, i) => weekStart + i * DAY), [weekStart]);
  const cells = useMemo(() => {
    const map = new Map<number, { at: number; label: string; status: string; routine: Routine }[]>();
    for (const d of days) map.set(d, []);
    const end = weekStart + 7 * DAY;
    for (const r of snap.routines) {
      for (const run of r.runs) if (run.at >= weekStart && run.at < end) map.get(startOfDay(run.at))?.push({ at: run.at, label: r.name, status: run.status, routine: r });
      if (!r.paused && r.trigger.kind === 'schedule') {
        const from = new Date(Math.max(Date.now(), weekStart));
        const upcoming = r.trigger.once ? (r.trigger.once >= from.getTime() ? [r.trigger.once] : []) : nextRuns(r.trigger.cron, r.trigger.tz, 60, from);
        for (const at of upcoming) if (at < end) map.get(startOfDay(at))?.push({ at, label: r.name, status: 'next', routine: r });
      }
    }
    for (const list of map.values()) list.sort((a, b) => a.at - b.at);
    return map;
  }, [snap.routines, days, weekStart]);

  const save = async () => {
    if (!parsed.ok) return;
    const what = kind === 'graph' ? { kind, graph } : kind === 'artifact' ? { kind, artifact } : { kind, prompt: prompt || phrase };
    if (kind === 'graph' && !graph) { toast('Make a graph first'); return; }
    if (kind === 'artifact' && !artifact) { toast('There is no artifact to refresh yet'); return; }
    const r = await act(() => post<Routine>(`/api/ws/${wsId()}/routines`, { name: name || phrase, phrase, tz, project, approval, runsOn, computer: runsOn === 'computer' ? computer : undefined, ifAsleep, notify, what }).then(async (x) => { await refreshSnap(); return x; }), (x) => `Saved · ${x.trigger.kind === 'schedule' ? describeNext(x.trigger.cron, x.trigger.tz) : 'waits for its event'}`);
    if (r) { setName(''); setPrompt(''); setOpen(r.id); }
  };

  return (
    <div className="page-pad" style={{ flex: '1 1 auto', overflowY: 'auto', padding: '26px 30px', display: 'flex', flexDirection: 'column', gap: 16 }}>
      <div className="row"><h1 className="disp" style={{ fontSize: 30 }}>Routines</h1><span className="muted" style={{ fontSize: 13 }}>{snap.routines.length} in {snap.workspace.name} · times in {tz}</span></div>

      <form className="card rise" style={{ padding: 16, display: 'flex', flexDirection: 'column', gap: 10 }} onSubmit={(e) => { e.preventDefault(); void save(); }}>
        <label className="field">When — in plain words<input className="inp" value={phrase} onChange={(e) => setPhrase(e.target.value)} aria-label="Schedule in plain words" placeholder="every 2 hours 9–18 weekdays · tomorrow at 9 · on push to main" /></label>
        <div className="card" aria-live="polite" style={{ padding: 12, background: 'var(--bg0)', display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))', gap: 8, fontSize: 12.5 }}>
          {parsed.ok ? <>
            <span><span className="lbl">Understood as</span><br /><b>{parsed.human}</b></span>
            <span><span className="lbl">{parsed.kind === 'event' ? 'Trigger' : 'Cron'}</span><br /><span className="mono">{parsed.kind === 'event' ? parsed.event?.event + ' · ' + parsed.event?.detail : parsed.cron}</span></span>
            <span><span className="lbl">Type</span><br />{parsed.kind === 'once' ? 'once' : parsed.kind === 'event' ? 'on an event' : 'repeating'}</span>
            <span><span className="lbl">Next run</span><br />{parsed.next ? formatWhen(parsed.next, parsed.tz) : '—'}</span>
            <span><span className="lbl">Time zone</span><br />{parsed.tz}</span>
            <span><span className="lbl">Matched</span><br /><span className="mono">{parsed.matched.join(' · ') || '—'}</span></span>
          </> : <span style={{ color: 'var(--error-text)' }}>{parsed.error || 'Could not read that'}</span>}
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 10 }}>
          <label className="field">Name<input className="inp" value={name} onChange={(e) => setName(e.target.value)} placeholder="Morning brief" /></label>
          <label className="field">Project<select className="inp" value={project} onChange={(e) => setProject(e.target.value)}>{flatProjects(snap.projects).map(({ project: p, depth }) => <option key={p.id} value={p.id}>{' '.repeat(depth * 2)}{p.name}</option>)}</select></label>
        </div>
        <div className="row" style={{ flexWrap: 'wrap' }}><span className="lbl">What</span>
          {(['prompt', 'graph', 'artifact'] as const).map((k) => <button key={k} type="button" className="chip" aria-pressed={kind === k} onClick={() => setKind(k)}>{{ prompt: 'A prompt', graph: 'A graph', artifact: 'Refresh an artifact' }[k]}</button>)}
        </div>
        {kind === 'prompt' && <label className="field">Prompt<textarea className="inp" rows={3} value={prompt} onChange={(e) => setPrompt(e.target.value)} placeholder="Summarise yesterday’s commits and open PRs" /></label>}
        {kind === 'graph' && <label className="field">Graph<select className="inp" value={graph} onChange={(e) => setGraph(e.target.value)}>{snap.graphs.map((g) => <option key={g.id} value={g.id}>{g.name}</option>)}{!snap.graphs.length && <option value="">No graphs yet</option>}</select></label>}
        {kind === 'artifact' && <label className="field">Artifact<select className="inp" value={artifact} onChange={(e) => setArtifact(e.target.value)}>{snap.artifacts.map((a) => <option key={a.id} value={a.id}>{a.type} · {a.title}</option>)}{!snap.artifacts.length && <option value="">No artifacts yet</option>}</select></label>}
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 10 }}>
          <div className="field">Runs on<span className="row" style={{ flexWrap: 'wrap' }}>{(['cloud', 'computer', 'org-runner'] as const).filter((x) => x !== 'org-runner' || isOrg).map((x) => <button key={x} type="button" className="chip" aria-pressed={runsOn === x} onClick={() => setRunsOn(x)}>{{ cloud: 'Cloud runner', computer: 'A linked computer', 'org-runner': 'Org runner' }[x]}</button>)}</span></div>
          {runsOn === 'computer' && <label className="field">Computer<select className="inp" value={computer} onChange={(e) => setComputer(e.target.value)}>{app.computers.map((c) => <option key={c.id} value={c.id}>{c.name} · {c.status}</option>)}{!app.computers.length && <option value="">No computers linked</option>}</select></label>}
          <div className="field">Approval<span className="row"><button type="button" className="chip" aria-pressed={approval === 'ask'} onClick={() => setApproval('ask')}>Ask before acting</button><button type="button" className="chip" aria-pressed={approval === 'pre-approved'} onClick={() => setApproval('pre-approved')}>Pre-approved</button></span></div>
          <div className="field">If the computer is asleep<span className="row" style={{ flexWrap: 'wrap' }}>{(['Run when it wakes', 'Wake it', 'Skip'] as const).map((x) => <button key={x} type="button" className="chip" aria-pressed={ifAsleep === x} onClick={() => setIfAsleep(x)}>{x}</button>)}</span></div>
          <div className="field">When done, notify<span className="row">{['push', 'email', 'chat'].map((x) => <button key={x} type="button" className="chip" aria-pressed={notify.includes(x)} onClick={() => setNotify(notify.includes(x) ? notify.filter((n) => n !== x) : [...notify, x])}>{x}</button>)}</span></div>
        </div>
        {runsOn === 'cloud' && <span className="muted" style={{ fontSize: 12 }}>The cloud runner has no access to local files; it works from the project’s instructions and memory.</span>}
        <div className="row"><span className="grow" /><button type="submit" className="btn btn-p" disabled={!parsed.ok}>Save routine</button></div>
      </form>

      <div className="card" style={{ padding: 14, display: 'flex', flexDirection: 'column', gap: 10 }}>
        <div className="row"><span className="lbl grow">Week of {new Date(weekStart).toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}</span>
          <button type="button" className="btn" aria-label="Previous week" onClick={() => setWeekStart(weekStart - 7 * DAY)}>←</button>
          <button type="button" className="btn" onClick={() => setWeekStart(startOfDay(Date.now() - ((new Date().getDay() + 6) % 7) * DAY))}>This week</button>
          <button type="button" className="btn" aria-label="Next week" onClick={() => setWeekStart(weekStart + 7 * DAY)}>→</button>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(7, minmax(72px, 1fr))', gap: 6, overflowX: 'auto' }}>
          {days.map((d) => {
            const today = startOfDay(Date.now()) === d;
            const list = cells.get(d) || [];
            return (
              <div key={d} style={{ minWidth: 92, minHeight: 150, borderRadius: 8, border: `1px solid ${today ? 'var(--accent-p)' : 'var(--line)'}`, padding: 6, display: 'flex', flexDirection: 'column', gap: 4 }}>
                <span className="lbl" style={{ color: today ? 'var(--accent-p-text)' : undefined }}>{new Date(d).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric' })}</span>
                {list.slice(0, 8).map((c, i) => (
                  <button key={i} type="button" onClick={() => setOpen(c.routine.id)} title={`${c.label} · ${c.status}`} style={{ textAlign: 'left', fontSize: 11, padding: '3px 5px', borderRadius: 5, border: 0, background: 'var(--bg2)', color: 'var(--text)', borderLeft: `3px solid ${STATUS_COLOR[c.status] || 'var(--line)'}`, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                    <span className="mono">{new Date(c.at).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })}</span> {c.label}
                  </button>
                ))}
                {list.length > 8 && <span className="muted" style={{ fontSize: 11 }}>+{list.length - 8} more</span>}
              </div>
            );
          })}
        </div>
        <span className="row muted" style={{ fontSize: 11, gap: 12, flexWrap: 'wrap' }}>{Object.entries(STATUS_COLOR).map(([k, c]) => <span key={k} className="row" style={{ gap: 4 }}><span className="dot" style={{ background: c }} />{k === 'next' ? 'coming up' : k}</span>)}</span>
      </div>

      {snap.routines.map((r) => <RoutineCard key={r.id} r={r} open={open === r.id} onToggle={() => setOpen(open === r.id ? null : r.id)} />)}
      {!snap.routines.length && <p className="muted">No routines yet. Write one above in plain words.</p>}
    </div>
  );
}

function RoutineCard({ r, open, onToggle }: { r: Routine; open: boolean; onToggle: () => void }) {
  const snap = useStore((s) => s.snap)!;
  const app = useStore((s) => s.app)!;
  const [hook, setHook] = useState('');
  const base = `/api/ws/${wsId()}/routines/${r.id}`;
  const last = r.runs[0];
  const when = r.paused ? 'paused' : r.trigger.kind === 'schedule' ? (r.trigger.once ? 'once · ' : '') + describeNext(r.trigger.cron, r.trigger.tz) : 'on ' + r.trigger.event + (r.trigger.detail && r.trigger.detail !== '*' ? ' · ' + r.trigger.detail : '');
  const whatLabel = r.what.kind === 'graph' ? 'graph · ' + (snap.graphs.find((g) => g.id === (r.what as { graph: string }).graph)?.name || 'missing graph') : r.what.kind === 'artifact' ? 'refresh · ' + (snap.artifacts.find((a) => a.id === (r.what as { artifact: string }).artifact)?.title || 'missing artifact') : 'prompt';
  return (
    <div className="card rise" style={{ padding: 12, display: 'flex', flexDirection: 'column', gap: 6, borderColor: open ? 'var(--accent-p)' : undefined }}>
      <button type="button" className="row" onClick={onToggle} aria-expanded={open} style={{ border: 0, background: 'transparent', color: 'var(--text)', padding: 0, textAlign: 'left' }}>
        <span className="dot" style={{ background: last ? STATUS_COLOR[last.status] : 'var(--dim)' }} />
        <b className="grow">{r.name}</b><span className="muted" style={{ fontSize: 12 }}>{when}</span>
      </button>
      <span className="muted" style={{ fontSize: 12 }}>“{r.phrase}” · {whatLabel} · {projectPathName(snap.projects, r.project)} · {r.runsOn === 'computer' ? (app.computers.find((c) => c.id === r.computer)?.name || 'computer') : r.runsOn} · {r.approval === 'ask' ? 'asks first' : 'pre-approved'} · if asleep: {r.ifAsleep.toLowerCase()} · notify {r.notify.join(', ') || 'nobody'}</span>
      {last?.status === 'waiting' && <div className="row card" style={{ padding: '8px 10px', borderColor: 'var(--warning)' }}><span className="grow" style={{ fontSize: 13 }}>{last.note}</span>{/approval/i.test(last.note) && <button type="button" className="btn btn-p" onClick={() => void act(() => post(`${base}/approve`).then(() => refreshSnap()), 'Approved — running')}>Approve and run</button>}</div>}
      <div className="row" style={{ flexWrap: 'wrap' }}>
        <button type="button" className="btn" disabled={r.paused} onClick={() => void act(() => post(`${base}/run`).then(() => refreshSnap()), r.approval === 'ask' ? 'Waiting for your approval' : 'Running now')}>Run now</button>
        <button type="button" className="btn" onClick={() => void act(() => patch(base, { paused: !r.paused }).then(() => refreshSnap()), r.paused ? 'Resumed' : 'Paused')}>{r.paused ? 'Resume' : 'Pause'}</button>
        {r.trigger.kind === 'event' && r.trigger.event === 'webhook' && <button type="button" className="btn" onClick={() => void get<{ path: string }>(`${base}/hook`).then((x) => setHook(location.origin.replace(/:\d+$/, '') + x.path))}>Webhook URL</button>}
        <span className="grow" />
        <button type="button" className="btn btn-d" onClick={() => void act(() => del(base).then(() => refreshSnap()), 'Deleted')}>Delete</button>
      </div>
      {hook && <pre className="pre">POST {hook}</pre>}
      {open && <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        <span className="lbl">Run history</span>
        {r.runs.map((run) => (
          <div key={run.id + run.at} className="row" style={{ fontSize: 12.5, padding: '5px 8px', borderRadius: 6, background: 'var(--bg0)' }}>
            <span className="dot" style={{ background: STATUS_COLOR[run.status] }} /><span className="mono muted" style={{ width: 150 }}>{new Date(run.at).toLocaleString()}</span><b style={{ width: 70 }}>{run.status}</b><span className="grow ellipsis">{run.note}</span>
            {run.chat && <button type="button" className="link" style={{ fontSize: 12 }} onClick={() => go('chat', { chatId: run.chat })}>Open chat</button>}
          </div>
        ))}
        {!r.runs.length && <span className="muted" style={{ fontSize: 12 }}>No runs yet.</span>}
      </div>}
    </div>
  );
}
