// Lightweight SVG view of the Graphify memory graph (R-23). No external graph library.
import { useMemo, useState } from 'react';
import type { MemoryGraph, MemoryGraphNode } from '../../../../packages/shared/src/types.ts';

const W = 920;
const H = 560;

interface Pos { x: number; y: number }

function layout(graph: MemoryGraph): Map<string, Pos> {
  const pos = new Map<string, Pos>();
  const allXY = graph.nodes.length > 0 && graph.nodes.every((n) => typeof n.x === 'number' && typeof n.y === 'number');
  if (allXY) {
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of graph.nodes) {
      minX = Math.min(minX, n.x as number); minY = Math.min(minY, n.y as number);
      maxX = Math.max(maxX, n.x as number); maxY = Math.max(maxY, n.y as number);
    }
    const spanX = Math.max(1, maxX - minX), spanY = Math.max(1, maxY - minY);
    for (const n of graph.nodes) {
      pos.set(n.id, { x: 40 + ((n.x as number) - minX) / spanX * (W - 80), y: 40 + ((n.y as number) - minY) / spanY * (H - 80) });
    }
    return pos;
  }
  const groups = new Map<string, MemoryGraphNode[]>();
  for (const n of graph.nodes) {
    const list = groups.get(n.community) || [];
    list.push(n);
    groups.set(n.community, list);
  }
  const keys = Array.from(groups.keys());
  const R = keys.length > 1 ? Math.min(W, H) * 0.31 : 0;
  keys.forEach((key, ci) => {
    const angle = (ci / Math.max(1, keys.length)) * Math.PI * 2 - Math.PI / 2;
    const cx = W / 2 + Math.cos(angle) * R;
    const cy = H / 2 + Math.sin(angle) * R;
    const list = groups.get(key) as MemoryGraphNode[];
    list.sort((a, b) => (b.size || 0) - (a.size || 0));
    list.forEach((n, i) => {
      if (R === 0) {
        const a = i * 2.39996;
        const r = 6 + Math.sqrt(i) * 13;
        pos.set(n.id, { x: cx + Math.cos(a) * r, y: cy + Math.sin(a) * r });
      } else {
        // A small cloud around the community centre.
        const a = i * 2.39996;
        const r = 6 + Math.sqrt(i) * 7.5;
        pos.set(n.id, {
          x: Math.max(18, Math.min(W - 18, cx + Math.cos(a) * r)),
          y: Math.max(18, Math.min(H - 18, cy + Math.sin(a) * r)),
        });
      }
    });
  });
  return pos;
}

function radius(n: MemoryGraphNode): number {
  if (n.kind === 'community') return 11;
  if (n.kind === 'folder') return 7;
  if (n.kind === 'symbol') return 4.5;
  if (n.kind === 'concept') return 5;
  return Math.max(2.6, Math.min(8, 2.6 + (n.size || 0) * 0.7));
}

const KIND_LABEL: Record<MemoryGraphNode['kind'], string> = { folder: 'folder', file: 'file', symbol: 'symbol', concept: 'concept', community: 'community' };

export function MemoryGraphView({ graph, highlight = [], onPick }: { graph: MemoryGraph; highlight?: string[]; onPick?: (n: MemoryGraphNode) => void }) {
  const [community, setCommunity] = useState<string>('all');
  const [hover, setHover] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);

  const nodes = useMemo(() => {
    if (community === 'all') return graph.nodes.slice(0, 520);
    return graph.nodes.filter((n) => n.community === community).slice(0, 520);
  }, [graph.nodes, community]);
  const ids = useMemo(() => new Set(nodes.map((n) => n.id)), [nodes]);
  const edges = useMemo(() => graph.edges.filter((e) => ids.has(e.from) && ids.has(e.to)).slice(0, 1800), [graph.edges, ids]);
  const pos = useMemo(() => layout({ ...graph, nodes }), [graph, nodes]);
  const hi = useMemo(() => new Set(highlight), [highlight]);
  const byId = useMemo(() => new Map(nodes.map((n) => [n.id, n])), [nodes]);
  const selected = picked ? byId.get(picked) : undefined;
  const color = new Map(graph.communities.map((c) => [c.id, c.color]));

  if (!graph.nodes.length) {
    return <p className="muted" style={{ margin: 0 }}>Nothing to draw yet. Add a folder and press Build.</p>;
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div className="row" style={{ flexWrap: 'wrap' }}>
        <button type="button" className="chip" aria-pressed={community === 'all'} onClick={() => setCommunity('all')}>All · {graph.nodes.length}</button>
        {graph.communities.slice(0, 16).map((c) => (
          <button key={c.id} type="button" className="chip" aria-pressed={community === c.id} onClick={() => setCommunity(c.id)} style={{ borderColor: c.color }}><span className="dot" style={{ width: 8, height: 8, background: c.color }} /> {c.name} · {c.size}</button>
        ))}
        <span className="grow" />
        <span className="muted" style={{ fontSize: 11 }}>{graph.source === 'graphify' ? 'Graphify graph.json' : 'Built-in folder index'} · {graph.fileCount} files{graph.truncated ? ' · truncated' : ''}</span>
      </div>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Memory graph" style={{ width: '100%', height: 'auto', maxHeight: 520, background: 'var(--bg1)', borderRadius: 12, border: '1px solid var(--bg2)' }}>
        {edges.map((e, i) => {
          const a = pos.get(e.from); const b = pos.get(e.to);
          if (!a || !b) return null;
          const dim = hover && hover !== e.from && hover !== e.to;
          return <line key={i} x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke={e.ambiguous ? 'var(--warning)' : 'var(--line2)'} strokeWidth={e.ambiguous ? 1.4 : 0.7} strokeDasharray={e.ambiguous ? '4 3' : undefined} opacity={dim ? 0.08 : e.ambiguous ? 0.85 : 0.3} />;
        })}
        {nodes.map((n) => {
          const p = pos.get(n.id);
          if (!p) return null;
          const r = radius(n);
          const isHi = hi.has(n.id);
          const isSel = picked === n.id;
          const showLabel = n.kind === 'folder' || n.kind === 'community' || nodes.length <= 70 || isHi || isSel || hover === n.id;
          return (
            <g key={n.id} onMouseEnter={() => setHover(n.id)} onMouseLeave={() => setHover(null)} onClick={() => { setPicked(n.id); onPick?.(n); }} style={{ cursor: onPick ? 'pointer' : 'default' }}>
              <title>{`${n.label} · ${KIND_LABEL[n.kind]}`}</title>
              <circle cx={p.x} cy={p.y} r={isHi ? r + 2.5 : r} fill={color.get(n.community) || 'var(--muted)'} stroke={isHi || isSel ? 'var(--text)' : 'var(--bg1)'} strokeWidth={isHi || isSel ? 1.8 : 1} opacity={hover && hover !== n.id ? 0.55 : 0.92} />
              {showLabel && <text x={p.x + r + 3} y={p.y + 3.5} fontSize={10.5} fill={isHi || isSel ? 'var(--text)' : 'var(--text2)'} style={{ pointerEvents: 'none' }}>{n.label.length > 28 ? n.label.slice(0, 27) + '…' : n.label}</text>}
            </g>
          );
        })}
      </svg>
      {selected && (
        <div className="row card" style={{ padding: '8px 12px' }}>
          <span className="grow"><b>{selected.label}</b><span className="muted" style={{ marginLeft: 8, fontSize: 12 }}>{KIND_LABEL[selected.kind]} · {graph.communities.find((c) => c.id === selected.community)?.name || selected.community}</span></span>
          <span className="mono muted" style={{ fontSize: 11 }}>{selected.id.slice(0, 90)}</span>
        </div>
      )}
    </div>
  );
}
