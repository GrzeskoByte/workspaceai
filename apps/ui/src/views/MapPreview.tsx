// Previews on the mind map: an artifact's real page and a graph's diagram, as thumbnails on their nodes and larger on
// request. Content is fetched once per version and only for nodes that have come into view.
import { useEffect, useState, type ReactNode } from 'react';
import { get } from '../api.ts';
import { wsId } from '../store.ts';
import type { GraphRun } from '../../../../packages/shared/src/types.ts';
import { graphPreview, type GraphPreview } from '../../../../packages/shared/src/bpmnPreview.ts';
import { stepState } from './runPaint.ts';

type Loaded<T> = { state: 'loading' } | { state: 'ready'; value: T } | { state: 'error' };
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
    cached(key, load).then((value) => { if (!stop) setGot({ state: 'ready', value }); }, () => { if (!stop) setGot({ state: 'error' }); });
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
