// Previews on the mind map: an artifact's real page and a graph's diagram, as thumbnails on their nodes and larger on
// request. Content is fetched once per version and only for nodes that have come into view.
import { useEffect, useState, type ReactNode } from 'react';
import { get } from '../api.ts';
import { wsId } from '../store.ts';
import type { GraphRun, MemoryNodeDetail } from '../../../../packages/shared/src/types.ts';
import { graphPreview, type GraphPreview } from '../../../../packages/shared/src/bpmnPreview.ts';
import { stepState } from './runPaint.ts';
import { Markdown } from '../ui/Markdown.tsx';
import { gitlabMarkdown } from '../../../../packages/shared/src/markdown.ts';
import { initialsOf } from '../../../../packages/shared/src/util.ts';

type Loaded<T> = { state: 'loading' } | { state: 'ready'; value: T } | { state: 'error'; message: string };
const cache = new Map<string, Promise<unknown>>();
function cached<T>(key: string, load: () => Promise<T>): Promise<T> {
  let p = cache.get(key) as Promise<T> | undefined;
  if (!p) {
    p = load();
    cache.set(key, p);
    p.catch(() => cache.delete(key));
    // Old versions and long sessions shouldn't pile up.
    if (cache.size > 80) cache.delete(cache.keys().next().value as string);
  }
  return p;
}
function useLoaded<T>(key: string, load: () => Promise<T>): Loaded<T> {
  const [got, setGot] = useState<Loaded<T>>({ state: 'loading' });
  useEffect(() => {
    let stop = false;
    setGot({ state: 'loading' });
    cached(key, load).then((value) => { if (!stop) setGot({ state: 'ready', value }); }, (e) => { if (!stop) setGot({ state: 'error', message: (e as Error).message }); });
    return () => { stop = true; };
  }, [key]);
  return got;
}

/** A step's name in up to three short lines, to stay inside its box (SVG text doesn't wrap by itself). */
export function wrap(name: string, width = 13, maxLines = 3): string[] {
  const lines: string[] = [];
  let line = '';
  for (const word of name.split(/\s+/).filter(Boolean)) {
    const piece = word.length > width ? word.slice(0, width - 1) + '…' : word;
    if (line && (line + ' ' + piece).length > width) { lines.push(line); line = piece; } else line = line ? line + ' ' + piece : piece;
  }
  if (line) lines.push(line);
  if (lines.length <= maxLines) return lines;
  const kept = lines.slice(0, maxLines);
  kept[maxLines - 1] = kept[maxLines - 1].slice(0, width - 1) + '…';
  return kept;
}

const note = (text: string): ReactNode => <span className="muted map-thumb-note">{text}</span>;

/** An artifact's page in a sandboxed frame. As a thumbnail it is drawn at desktop width, scaled down and not clickable. */
export function ArtifactPreview({ id, version, width, height, large, page: pageWidth = 1200 }: { id: string; version: string; width?: number; height?: number; large?: boolean; page?: number }) {
  const page = useLoaded(`art:${wsId()}:${id}:${version}`, () => get<{ html: string }>(`/api/ws/${wsId()}/artifacts/${id}/content`).then((r) => r.html));
  if (page.state === 'loading') return note('Loading…');
  if (page.state === 'error') return note('No preview');
  if (large) return <iframe title="Artifact preview" sandbox="allow-scripts" srcDoc={page.value} className="map-peek-frame" />;
  const scale = (width || 240) / pageWidth;
  return <iframe title="" aria-hidden tabIndex={-1} sandbox="allow-scripts" srcDoc={page.value} className="map-thumb-frame" style={{ width: pageWidth, height: (height || 130) / scale, transform: `scale(${scale})` }} />;
}

/** A graph's diagram, with its latest run's progress on it. */
export function GraphPreviewView({ id, version, run, large }: { id: string; version: string; run?: GraphRun; large?: boolean }) {
  const graph = useLoaded<GraphPreview | null>(`graph:${wsId()}:${id}:${version}`, () => get<{ xml: string }>(`/api/ws/${wsId()}/graphs/${id}`).then((r) => graphPreview(r.xml)));
  if (graph.state === 'loading') return note('Loading…');
  if (graph.state === 'error' || !graph.value) return note('No preview');
  const g = graph.value, pad = 12;
  return (
    <svg className={'map-graph' + (large ? ' large' : '')} viewBox={`${g.bounds.x - pad} ${g.bounds.y - pad} ${g.bounds.w + pad * 2} ${g.bounds.h + pad * 2}`} preserveAspectRatio="xMidYMid meet" role="img" aria-label="Graph diagram">
      {g.edges.map((e, i) => <polyline key={e.id || i} points={e.points.map((p) => `${p.x},${p.y}`).join(' ')} className={run?.taken?.includes(e.id) ? 'gp-edge taken' : 'gp-edge'} />)}
      {g.shapes.map((s) => {
        const cls = 'gp-shape st-' + stepState(run, s.id);
        const cx = s.x + s.w / 2, cy = s.y + s.h / 2;
        return (
          <g key={s.id}>
            {s.kind === 'start' || s.kind === 'end' || s.kind === 'event'
              ? <circle cx={cx} cy={cy} r={Math.min(s.w, s.h) / 2} className={cls + (s.kind === 'end' ? ' thick' : '')} />
              : s.kind === 'gateway'
                ? <polygon points={`${cx},${s.y} ${s.x + s.w},${cy} ${cx},${s.y + s.h} ${s.x},${cy}`} className={cls} />
                : <rect x={s.x} y={s.y} width={s.w} height={s.h} rx={10} className={cls + (s.kind === 'user' ? ' user' : '')} />}
            {s.kind === 'task' || s.kind === 'user' ? (() => { const lines = wrap(s.name); return <text x={cx} y={cy + 4 - (lines.length - 1) * 7} textAnchor="middle" className="gp-text">{lines.map((l, i) => <tspan key={i} x={cx} dy={i ? 14 : 0}>{l}</tspan>)}</text>; })() : null}
            {large && s.kind === 'gateway' && s.name ? <text x={cx} y={s.y - 6} textAnchor="middle" className="gp-text">{s.name}</text> : null}
          </g>
        );
      })}
    </svg>
  );
}

/** A Graphify node on the map: its connections, which can be followed here, and its file. */
/** What GitLab returns for a ticket, as far as the preview shows it. */
interface Ticket {
  title: string; web_url: string; description?: string | null; state?: string; labels?: string[]; due_date?: string | null; created_at?: string; updated_at?: string; user_notes_count?: number;
  references?: { full?: string }; author?: { name: string } | null; assignees?: { name: string }[]; assignee?: { name: string } | null;
  milestone?: { title: string; due_date?: string | null } | null; task_completion_status?: { count: number; completed_count: number };
}
const day = (d?: string | null) => (d ? new Date(d).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' }) : '');

function Person({ name }: { name: string }) {
  return <span className="ticket-person"><span className="ticket-avatar" aria-hidden>{initialsOf(name)}</span>{name}</span>;
}

/** A GitLab ticket from the map's copy, laid out to read: GitLab's own page refuses to be shown here. */
export function TicketPreview({ project, iid }: { project: string; iid: string }) {
  const [state, setState] = useState<Loaded<Ticket>>({ state: 'loading' });
  useEffect(() => {
    let stop = false;
    setState({ state: 'loading' });
    get<Ticket>(`/api/ws/${wsId()}/projects/${project}/tickets/${iid}`).then((value) => { if (!stop) setState({ state: 'ready', value }); }, (e) => { if (!stop) setState({ state: 'error', message: (e as Error).message }); });
    return () => { stop = true; };
  }, [project, iid]);
  if (state.state !== 'ready') return <div className="ticket"><p className={'muted' + (state.state === 'loading' ? ' pulse' : '')}>{state.state === 'loading' ? 'Loading…' : state.message}</p></div>;
  const t = state.value;
  const people = t.assignees?.length ? t.assignees : t.assignee ? [t.assignee] : [];
  const tasks = t.task_completion_status;
  const due = t.due_date || t.milestone?.due_date;
  const late = !!due && new Date(due + 'T23:59:59') < new Date();
  const text = gitlabMarkdown(t.description || '', t.web_url);
  return (
    <div className="ticket">
      <div className="ticket-main">
        <span className="lbl">{t.references?.full || `#${iid}`}</span>
        <h1 className="disp ticket-title">{t.title}</h1>
        <div className="ticket-sub">
          <span className={'ticket-state ' + (t.state === 'closed' ? 'closed' : 'open')}><span className="dot" aria-hidden />{t.state === 'closed' ? 'Closed' : 'Open'}</span>
          {t.author && <span className="muted">Opened {day(t.created_at)} by {t.author.name}</span>}
        </div>
        <hr />
        {text ? <div className="ticket-body"><Markdown text={text} /></div> : <p className="muted">No description.</p>}
      </div>
      <aside className="ticket-side" aria-label="Ticket details">
        <div><span className="lbl">Assignee{people.length > 1 ? 's' : ''}</span>{people.length ? people.map((p) => <Person key={p.name} name={p.name} />) : <span className="muted">No one yet</span>}</div>
        {t.milestone && <div><span className="lbl">Milestone</span><span>{t.milestone.title}</span></div>}
        {due && <div><span className="lbl">Due</span><span className={late ? 'ticket-late' : undefined}>{day(due + 'T12:00:00')}{late ? ' · overdue' : ''}</span></div>}
        {!!t.labels?.length && <div><span className="lbl">Labels</span><span className="ticket-labels">{t.labels.map((l) => <span key={l} className="ticket-label">{l}</span>)}</span></div>}
        {!!tasks?.count && <div><span className="lbl">Tasks</span><span>{tasks.completed_count} of {tasks.count} done</span><span className="ticket-bar" role="img" aria-label={`${tasks.completed_count} of ${tasks.count} tasks done`}><span style={{ width: (tasks.completed_count / tasks.count) * 100 + '%' }} /></span></div>}
        {!!t.user_notes_count && <div><span className="lbl">Discussion</span><span>{t.user_notes_count} comment{t.user_notes_count === 1 ? '' : 's'} on GitLab</span></div>}
        {t.updated_at && <div><span className="lbl">Updated</span><span>{day(t.updated_at)}</span></div>}
      </aside>
    </div>
  );
}

export function MemoryPreview({ id }: { id: string }) {
  const [cur, setCur] = useState(id);
  useEffect(() => setCur(id), [id]);
  // Each opening reads the file again; the shared cache is for previews that have a version.
  const [opened] = useState(() => Date.now());
  const got = useLoaded(`mem:${wsId()}:${opened}:${cur}`, () => get<MemoryNodeDetail>(`/api/ws/${wsId()}/graphify/node?id=${encodeURIComponent(cur)}`));
  if (got.state === 'loading') return note('Loading…');
  // A supervisor started before this screen was built has no such route: say so rather than blame the node.
  if (got.state === 'error') return note(got.message.startsWith('No route') ? 'WorkspaceAI is running older code than this screen. Restart it to preview memory.' : got.message);
  const { node, community, links, file } = got.value;
  return (
    <div className="map-memory">
      <div className="row" style={{ gap: 8 }}>
        {cur !== id && <button type="button" className="chip" onClick={() => setCur(id)}>Back</button>}
        <b>{node.label}</b><span className="muted" style={{ fontSize: 12 }}>{node.kind}{community ? ' · ' + community : ''}</span>
      </div>
      {links.length > 0 && <div className="row" style={{ flexWrap: 'wrap', gap: 6 }} aria-label="Connections">
        {links.map((l, i) => <button key={l.id + i} type="button" className="chip" title={l.kind} onClick={() => setCur(l.id)}><span className="muted">{l.relation}</span> {l.label}</button>)}
      </div>}
      {file && <>
        <span className="mono muted ellipsis" style={{ fontSize: 11 }} title={file.path}>{file.path}</span>
        {file.note && <span className="muted">{file.note}</span>}
        {file.image && <img src={file.image} alt={'Preview of ' + node.label} style={{ maxWidth: '100%', alignSelf: 'flex-start' }} />}
        {file.text !== undefined && <pre className="pre map-memory-file">{file.text}</pre>}
        {file.truncated && <span className="muted" style={{ fontSize: 12 }}>Showing the start of this file.</span>}
      </>}
    </div>
  );
}
