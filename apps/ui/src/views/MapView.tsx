// A project's mind map (or the workspace overview). Everything the project holds appears on it by itself; ideas, tasks
// and notes are added here, items can be moved between branches, and any two nodes can be linked. Positions are
// computed here and never saved. Nodes are HTML over an SVG layer of edges, under one pan-and-zoom transform.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { useStore, setState, toast, wsId, go, getState, openRun, refreshSnap, act, openExternal } from '../store.ts';
import { get, post } from '../api.ts';
import { childrenOf, isCollapsed, layoutMap, toMarkdown, type MapBox } from '../../../../packages/shared/src/mindmap.ts';
import type { Chat, GraphRun, MapLayout, MapOp, MapTaskStatus, ResolvedMap, ResolvedMapNode } from '../../../../packages/shared/src/types.ts';
import { Icon } from '../ui/Icon.tsx';
import { openArtifact, openChat } from '../shell/Shell.tsx';
import { ART_ICON } from './ArtifactPanel.tsx';
import { ArtifactPreview, GraphPreviewView, MemoryPreview, TicketPreview } from './MapPreview.tsx';
import { ChatActivity } from '../ui/ChatActivity.tsx';

const FILTERS = [['chat', 'Chats'], ['artifact', 'Artifacts'], ['graph', 'Graphs'], ['run', 'Runs'], ['memory', 'Memory'], ['routine', 'Routines'], ['repo', 'Repository and tickets'], ['own', 'Notes and tasks']] as const;
type FilterKey = (typeof FILTERS)[number][0];
// Memory opens where it is: no screen behind it, so these only have a preview.
const opensInPlace = (n: ResolvedMapNode) => n.kind === 'memory' || n.kind === 'fact';
const isMemoryGroup = (n: ResolvedMapNode) => n.id === 'group:memory' || n.id.startsWith('group:chat-memory:');
const filterOf = (n: ResolvedMapNode): FilterKey | null => (n.kind === 'fact' || n.kind === 'memory' || isMemoryGroup(n) ? 'memory' : n.own ? 'own' : n.kind === 'repo' || n.kind === 'milestone' || n.kind === 'ticket' ? 'repo' : n.kind === 'chat' || n.kind === 'artifact' || n.kind === 'graph' || n.kind === 'run' || n.kind === 'routine' ? n.kind : null);

const NEXT_STATUS: Record<MapTaskStatus, MapTaskStatus> = { todo: 'doing', doing: 'done', done: 'todo' };
const STATUS_ICON: Record<MapTaskStatus, string> = { todo: 'circle', doing: 'dot', done: 'check' };

function iconOf(n: ResolvedMapNode): string {
  switch (n.kind) {
    case 'workspace': return 'layers';
    case 'project': return 'folder';
    case 'chat': return 'chat';
    case 'artifact': return ART_ICON[n.subtype || ''] || 'layers';
    case 'graph': return 'workflow';
    case 'run': return 'play';
    case 'fact': return 'memory';
    case 'memory': return 'memory';
    case 'group': return isMemoryGroup(n) ? 'memory' : 'chat';
    case 'routine': return 'clock';
    case 'repo': return 'branch';
    case 'milestone': return 'filter';
    case 'ticket': return 'circle';
    case 'idea': return 'sparkle';
    case 'note': return 'doc';
    default: return STATUS_ICON[n.status || 'todo'];
  }
}

const canvas = typeof document !== 'undefined' ? document.createElement('canvas').getContext('2d') : null;
const widths = new Map<string, number>();
function textWidth(text: string, font: string): number {
  const key = font + '\n' + text;
  const hit = widths.get(key);
  if (hit !== undefined) return hit;
  let w = text.length * 7;
  if (canvas) { canvas.font = font; w = canvas.measureText(text).width; }
  widths.set(key, w);
  return w;
}
const UI_FONT = 'Inter, ui-sans-serif, system-ui, sans-serif';
const THUMB_W = 240, THUMB_H = 130, HEAD_H = 46;
const hasThumb = (n: ResolvedMapNode) => n.kind === 'artifact' || n.kind === 'graph';
const hasPreview = (n: ResolvedMapNode) => hasThumb(n) || opensInPlace(n) || n.kind === 'ticket';
function sizeOf(n: ResolvedMapNode, isRoot: boolean, previews: boolean): { w: number; h: number } {
  if (previews && hasThumb(n)) return { w: THUMB_W, h: HEAD_H + THUMB_H };
  if (isRoot) return { w: Math.min(320, Math.max(150, textWidth(n.title, `600 15px ${UI_FONT}`) + 64)), h: 48 };
  const title = textWidth(n.title, `500 13px ${UI_FONT}`), meta = n.meta ? textWidth(n.meta, `11px ${UI_FONT}`) : 0;
  return { w: Math.round(Math.min(280, Math.max(120, Math.max(title, meta) + 52))), h: n.meta ? 48 : 34 };
}

function storedJson<T>(key: string, fallback: T): T {
  try { const v = localStorage.getItem(key); return v ? (JSON.parse(v) as T) : fallback; } catch { return fallback; }
}
function storeJson(key: string, value: unknown): void { try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* per-viewer convenience only */ } }

function download(name: string, text: string, type: string): void {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = document.createElement('a'); a.href = url; a.download = name; a.click(); URL.revokeObjectURL(url);
}
const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

function edgePath(a: MapBox, b: MapBox): string {
  const left = b.side === 'left';
  const x1 = left ? a.x : a.x + a.w, y1 = a.y + a.h / 2, x2 = left ? b.x + b.w : b.x, y2 = b.y + b.h / 2;
  const mid = (x1 + x2) / 2;
  return `M${x1} ${y1} C${mid} ${y1} ${mid} ${y2} ${x2} ${y2}`;
}
function linkPath(a: MapBox, b: MapBox): { d: string; x: number; y: number } {
  const ax = a.x + a.w / 2, ay = a.y + a.h / 2, bx = b.x + b.w / 2, by = b.y + b.h / 2;
  // Bowed sideways so it doesn't run along the tree's own edges.
  const dx = bx - ax, dy = by - ay, bow = Math.min(120, 24 + Math.hypot(dx, dy) * 0.18);
  const len = Math.hypot(dx, dy) || 1;
  const cx = (ax + bx) / 2 - (dy / len) * bow, cy = (ay + by) / 2 + (dx / len) * bow;
  return { d: `M${ax} ${ay} Q${cx} ${cy} ${bx} ${by}`, x: (ax + 2 * cx + bx) / 4, y: (ay + 2 * cy + by) / 4 };
}

interface View { x: number; y: number; k: number }
type Gesture =
  | { type: 'pan'; sx: number; sy: number; ox: number; oy: number }
  | { type: 'node'; id: string; sx: number; sy: number; mx: number; my: number; moved: boolean }
  | { type: 'link'; from: string };
interface Drop { target: string | null; zone: 'child' | 'before' | 'after' | 'side' }
/** How close to a node a drop still counts as aimed at it (map units): enough to land in the gaps between siblings. */
const DROP_REACH = 56;

export function MapView({ scope, compact, focus, onClose, wide, onWide }: { scope: string; compact?: boolean; focus?: string; onClose?: () => void; wide?: boolean; onWide?: () => void }) {
  const snap = useStore((s) => s.snap)!;
  const tick = useStore((s) => s.mapTick);
  const reduceMotion = useStore((s) => !!s.app?.settings.reduceMotion);
  const base = scope === 'ws' ? `/api/ws/${wsId()}/map` : `/api/ws/${wsId()}/projects/${scope}/map`;
  const prefKey = `harness.map.${wsId()}.${scope}`;

  const [map, setMap] = useState<ResolvedMap | null>(null);
  const [error, setError] = useState('');
  const [overrides, setOverrides] = useState<Record<string, boolean>>(() => storedJson(prefKey + '.open', {}));
  const [off, setOff] = useState<FilterKey[]>(() => storedJson(prefKey + '.off', []));
  const [sel, setSel] = useState<string | null>(null);
  const [selLink, setSelLink] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ id: string; text: string; fresh?: boolean } | null>(null);
  const [view, setView] = useState<View>({ x: 0, y: 0, k: 1 });
  const [q, setQ] = useState('');
  const [previews, setPreviews] = useState<boolean>(() => storedJson(prefKey + '.previews', true));
  const [peek, setPeek] = useState<string | null>(null);
  const [tokenFor, setTokenFor] = useState('');
  const [tokenDraft, setTokenDraft] = useState('');
  const [starting, setStarting] = useState(false);
  const actionsBar = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState<{ ids: Set<string>; dx: number; dy: number } | null>(null);
  const runs = useStore((s) => s.runs);
  const seen = useRef(new Set<string>());
  const [menu, setMenu] = useState<null | 'filter' | 'export' | 'hidden'>(null);
  const [drop, setDrop] = useState<Drop | null>(null);
  const [ghost, setGhost] = useState<{ x: number; y: number } | null>(null);
  const [rubber, setRubber] = useState<{ from: string; x: number; y: number } | null>(null);
  const host = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture | null>(null);
  const viewRef = useRef(view); viewRef.current = view;
  const fitted = useRef(false);
  const focused = useRef<string | undefined>(undefined);
  const pointers = useRef(new Map<number, { x: number; y: number }>());
  const pinch = useRef<{ d: number; k: number } | null>(null);
  const closing = useRef(false);
  useEffect(() => { if (editing) closing.current = false; }, [editing?.id]);

  useEffect(() => storeJson(prefKey + '.open', overrides), [overrides, prefKey]);
  useEffect(() => storeJson(prefKey + '.off', off), [off, prefKey]);
  useEffect(() => storeJson(prefKey + '.previews', previews), [previews, prefKey]);
  // Graph previews show the latest run, so runs must be known before any has started in this window.
  useEffect(() => { void get<{ runs: GraphRun[] }>(`/api/ws/${wsId()}/runs`).then((r) => setState((st) => ({ runs: [...st.runs, ...r.runs.filter((x) => !st.runs.some((y) => y.id === x.id))] })), () => {}); }, []);

  const refresh = useCallback(() => get<ResolvedMap>(base).then((m) => { setMap(m); setError(''); }, (e) => setError((e as Error).message)), [base]);
  useEffect(() => { setMap(null); fitted.current = false; focused.current = undefined; setSel(null); void refresh(); }, [base, refresh]);
  // The map is built from the workspace, so it follows every change to it.
  useEffect(() => { const t = window.setTimeout(() => void refresh(), 150); return () => window.clearTimeout(t); }, [tick, snap.chats, snap.artifacts, snap.graphs, snap.facts, snap.routines, snap.projects, refresh]);

  const apply = useCallback(async (ops: MapOp[]) => {
    try { const r = await post<{ map: ResolvedMap; added: string[] }>(base + '/ops', { ops }); setMap(r.map); return r.added; }
    catch (e) { toast((e as Error).message, 'error'); return null; }
  }, [base]);

  // Filtered-out kinds step aside: what hangs off them moves up to the nearest node still shown.
  const shownMap = useMemo<ResolvedMap | null>(() => {
    if (!map) return null;
    if (!off.length) return map;
    const byId = new Map(map.nodes.map((n) => [n.id, n]));
    const gone = (n: ResolvedMapNode) => { const f = filterOf(n); return !!f && off.includes(f); };
    const nodes = map.nodes.filter((n) => !gone(n) && !(isMemoryGroup(n) && off.includes('memory'))).map((n) => {
      let p = n.parent;
      while (p && byId.get(p) && gone(byId.get(p)!)) p = byId.get(p)!.parent;
      return p === n.parent ? n : { ...n, parent: p };
    });
    const ids = new Set(nodes.map((n) => n.id));
    return { ...map, nodes, links: map.links.filter((l) => ids.has(l.from) && ids.has(l.to)) };
  }, [map, off]);

  const matches = useMemo(() => {
    const t = q.trim().toLowerCase();
    if (!t || !shownMap) return null;
    return new Set(shownMap.nodes.filter((n) => n.title.toLowerCase().includes(t) || (n.meta || '').toLowerCase().includes(t)).map((n) => n.id));
  }, [q, shownMap]);

  // While searching, branches holding a match open up.
  const open = useMemo(() => {
    if (!matches || !shownMap) return overrides;
    const byId = new Map(shownMap.nodes.map((n) => [n.id, n]));
    const out = { ...overrides };
    for (const id of matches) for (let p = byId.get(id)?.parent; p; p = byId.get(p)?.parent) out[p] = false;
    return out;
  }, [matches, overrides, shownMap]);

  const layout = useMemo(() => (shownMap ? layoutMap(shownMap, (n) => sizeOf(n, n.id === shownMap.root, previews), open) : null), [shownMap, open, previews]);
  const kids = useMemo(() => (shownMap ? childrenOf(shownMap) : new Map<string, ResolvedMapNode[]>()), [shownMap]);
  const nodeById = useMemo(() => new Map((shownMap?.nodes || []).map((n) => [n.id, n])), [shownMap]);

  // Nodes and edges glide from where they were to where the layout puts them.
  const [boxes, setBoxes] = useState<Map<string, MapBox>>(new Map());
  const boxesRef = useRef(boxes); boxesRef.current = boxes;
  useEffect(() => {
    if (!layout) return;
    const from = boxesRef.current, to = layout.boxes;
    const moving = [...to.values()].some((b) => { const a = from.get(b.id); return a && (a.x !== b.x || a.y !== b.y); });
    if (!moving || reduceMotion || !from.size) { setBoxes(to); return; }
    const start = performance.now();
    let frame = 0;
    const step = (now: number) => {
      const t = Math.min(1, (now - start) / 180), e = 1 - (1 - t) ** 3;
      if (t >= 1) { setBoxes(to); return; }
      const out = new Map<string, MapBox>();
      for (const b of to.values()) { const a = from.get(b.id); out.set(b.id, a ? { ...b, x: a.x + (b.x - a.x) * e, y: a.y + (b.y - a.y) * e } : b); }
      setBoxes(out);
      frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [layout, reduceMotion]);

  // The branch being dragged follows the pointer; everything else stays put until it lands.
  const drawn = useMemo(() => {
    if (!dragging) return boxes;
    const out = new Map(boxes);
    for (const id of dragging.ids) { const b = out.get(id); if (b) out.set(id, { ...b, x: b.x + dragging.dx, y: b.y + dragging.dy }); }
    return out;
  }, [boxes, dragging]);

  const fit = useCallback(() => {
    const el = host.current;
    if (!el || !layout) return;
    const r = el.getBoundingClientRect();
    const b = layout.bounds;
    const k = Math.max(0.25, Math.min(1, Math.min((r.width - 80) / Math.max(1, b.w), (r.height - 80) / Math.max(1, b.h))));
    setView({ k, x: r.width / 2 - (b.x + b.w / 2) * k, y: r.height / 2 - (b.y + b.h / 2) * k });
  }, [layout]);
  const centre = useCallback((id: string, k?: number) => {
    const el = host.current, b = layout?.boxes.get(id);
    if (!el || !b) return;
    const r = el.getBoundingClientRect();
    setView((v) => { const z = k ?? v.k; return { k: z, x: r.width / 2 - (b.x + b.w / 2) * z, y: r.height / 2 - (b.y + b.h / 2) * z }; });
  }, [layout]);
  useEffect(() => {
    if (!layout || !shownMap) return;
    if (focus && layout.boxes.has(focus) && focused.current !== focus) { focused.current = focus; fitted.current = true; setSel(focus); centre(focus, 0.85); return; }
    if (!fitted.current) { fitted.current = true; setSel(shownMap.root); fit(); }
  }, [layout, shownMap, focus, fit, centre]);

  // Keep the same spot centred when the canvas changes size (a panel opening, a window resizing).
  useEffect(() => {
    const el = host.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    let last = el.getBoundingClientRect();
    const ro = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      if (r.width === last.width && r.height === last.height) return;
      const dx = (r.width - last.width) / 2, dy = (r.height - last.height) / 2;
      last = r;
      setView((v) => ({ ...v, x: v.x + dx, y: v.y + dy }));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const toMapXY = (cx: number, cy: number) => { const r = host.current!.getBoundingClientRect(), v = viewRef.current; return { x: (cx - r.left - v.x) / v.k, y: (cy - r.top - v.y) / v.k }; };
  const zoomAt = (cx: number, cy: number, k: number) => {
    const r = host.current!.getBoundingClientRect();
    setView((v) => { const z = Math.max(0.25, Math.min(2, k)), px = cx - r.left, py = cy - r.top; return { k: z, x: px - (px - v.x) * (z / v.k), y: py - (py - v.y) * (z / v.k) }; });
  };

  const descendants = useCallback((id: string) => {
    const out = new Set<string>([id]);
    const walk = (x: string) => { for (const c of kids.get(x) || []) { out.add(c.id); walk(c.id); } };
    walk(id);
    return out;
  }, [kids]);

  const findDrop = useCallback((id: string, x: number, y: number): Drop => {
    const bad = descendants(id);
    // The nearest node takes it, so the gaps between siblings work as well as the nodes themselves.
    let best: MapBox | null = null, dist = Infinity;
    for (const b of layout?.boxes.values() || []) {
      if (bad.has(b.id)) continue;
      const d = Math.hypot(Math.max(b.x - x, 0, x - (b.x + b.w)), Math.max(b.y - y, 0, y - (b.y + b.h)));
      if (d < dist) { dist = d; best = b; }
    }
    if (!best || dist > DROP_REACH) return { target: null, zone: 'side' };
    if (best.id === shownMap?.root) return { target: best.id, zone: 'child' };
    if (y < best.y) return { target: best.id, zone: 'before' };
    if (y > best.y + best.h) return { target: best.id, zone: 'after' };
    if (dist > 0) {
      // Level with it: out past the node makes a child, back toward the centre places it after.
      const outward = best.side === 'right' ? x > best.x + best.w : x < best.x;
      return { target: best.id, zone: outward ? 'child' : 'after' };
    }
    const f = (y - best.y) / best.h;
    return { target: best.id, zone: f < 0.25 ? 'before' : f > 0.75 ? 'after' : 'child' };
  }, [layout, shownMap, descendants]);

  const finishDrop = useCallback((id: string, d: Drop, x: number) => {
    const n = nodeById.get(id);
    if (!n || !shownMap) return;
    if (!d.target) {
      // Dragged into open space: a top-level branch changes side.
      if (n.parent === shownMap.root && shownMap.layout === 'balance') void apply([{ op: 'move', id, side: x < 0 ? 'left' : 'right' }]);
      return;
    }
    if (d.zone === 'child') { if (n.parent !== d.target) { setOverrides((o) => ({ ...o, [d.target as string]: false })); void apply([{ op: 'move', id, parent: d.target }]); } return; }
    const t = nodeById.get(d.target);
    if (!t?.parent) return;
    const side = t.parent === shownMap.root ? layout?.boxes.get(t.id)?.side : undefined;
    const list = (kids.get(t.parent) || []).filter((c) => c.id !== id);
    const before = d.zone === 'before' ? t.id : list[list.findIndex((c) => c.id === t.id) + 1]?.id;
    void apply([{ op: 'move', id, parent: t.parent, ...(before ? { before } : {}), ...(side ? { side } : {}) }]);
  }, [apply, kids, layout, nodeById, shownMap]);

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (pointers.current.has(e.pointerId)) pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pointers.current.size === 2 && pinch.current) {
        const [a, b] = [...pointers.current.values()];
        zoomAt((a.x + b.x) / 2, (a.y + b.y) / 2, pinch.current.k * (Math.hypot(a.x - b.x, a.y - b.y) / pinch.current.d));
        return;
      }
      const g = gesture.current;
      if (!g) return;
      if (g.type === 'pan') { setView((v) => ({ ...v, x: g.ox + e.clientX - g.sx, y: g.oy + e.clientY - g.sy })); return; }
      const p = toMapXY(e.clientX, e.clientY);
      if (g.type === 'link') { setRubber({ from: g.from, x: p.x, y: p.y }); return; }
      if (!g.moved && Math.hypot(e.clientX - g.sx, e.clientY - g.sy) < 5) return;
      if (g.id === shownMap?.root) return;
      g.moved = true;
      setGhost(p);
      setDragging({ ids: descendants(g.id), dx: p.x - g.mx, dy: p.y - g.my });
      setDrop(findDrop(g.id, p.x, p.y));
    };
    const up = (e: PointerEvent) => {
      pointers.current.delete(e.pointerId);
      if (pointers.current.size < 2) pinch.current = null;
      const g = gesture.current;
      gesture.current = null;
      if (!g) return;
      const p = host.current ? toMapXY(e.clientX, e.clientY) : { x: 0, y: 0 };
      if (g.type === 'link') {
        setRubber(null);
        const d = findDrop(g.from, p.x, p.y);
        if (d.target) void apply([{ op: 'link', from: g.from, to: d.target }]);
      } else if (g.type === 'node' && g.moved) {
        setGhost(null); setDrop(null); setDragging(null);
        finishDrop(g.id, findDrop(g.id, p.x, p.y), p.x);
      }
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', up);
    return () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', up); window.removeEventListener('pointercancel', up); };
  }, [apply, descendants, findDrop, finishDrop, shownMap]);

  const onWheel = useCallback((e: WheelEvent) => {
    if ((e.target as HTMLElement).closest?.('.map-peek')) return;
    e.preventDefault();
    zoomAt(e.clientX, e.clientY, viewRef.current.k * Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0015)));
  }, []);
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [onWheel]);

  const projectId = useMemo(() => {
    if (scope !== 'ws') return scope;
    return snap.projects.find((p) => p.isGeneral)?.id || snap.projects[0]?.id || '';
  }, [scope, snap.projects]);

  const openNode = (n: ResolvedMapNode) => {
    switch (n.kind) {
      case 'chat': { const c = snap.chats.find((x) => x.id === n.ref); if (c) openChat(c); return; }
      case 'artifact': { const a = snap.artifacts.find((x) => x.id === n.ref); if (a) openArtifact(a); return; }
      case 'graph': go('graph', { graphId: n.ref || null }); return;
      case 'run': if (n.ref) { if (getState().view !== 'chat' && getState().view !== 'project') go('project', { projId: scope === 'ws' ? projectId : scope, ptab: 'map' }); openRun(n.ref); } return;
      case 'project': if (n.ref && n.ref !== scope) go('project', { projId: n.ref, ptab: 'map' }); else if (n.ref) go('project', { projId: n.ref, ptab: 'chats' }); return;
      case 'fact': case 'memory': setPeek(n.id); return;
      case 'group': if (isMemoryGroup(n)) setOverrides((o) => ({ ...o, [n.id]: !isCollapsed(n, open) })); return;
      case 'routine': go('routines'); return;
      // GitLab's pages refuse to be shown in a frame, so they go to the system browser.
      case 'repo': case 'milestone': case 'ticket': if (n.url) openExternal(n.url); return;
      default: return;
    }
  };
  const canOpen = (n: ResolvedMapNode) => !n.own && n.kind !== 'workspace' && n.id !== 'group:run-chats' && (n.kind !== 'milestone' || !!n.url);
  const addToken = (n: ResolvedMapNode) => void act(() => post(`/api/ws/${wsId()}/projects/${scope}/gitlab-token`, { host: n.ref, token: tokenDraft }), 'Token saved · asking GitLab').then((r) => { if (r) { setTokenDraft(''); setTokenFor(''); } });

  const toggle = (id: string) => { const n = nodeById.get(id); if (n) setOverrides((o) => ({ ...o, [id]: !isCollapsed(n, o) })); };
  const refocus = () => host.current?.focus({ preventScroll: true });

  const addNode = async (parent: string, before?: string) => {
    setOverrides((o) => ({ ...o, [parent]: false }));
    const added = await apply([{ op: 'add', parent, kind: 'idea', text: 'New idea', ...(before ? { before } : {}) }]);
    if (added?.[0]) { setSel(added[0]); setEditing({ id: added[0], text: '', fresh: true }); }
  };
  const addSibling = (id: string) => {
    const n = nodeById.get(id);
    if (!n?.parent) { void addNode(id); return; }
    const list = kids.get(n.parent) || [];
    void addNode(n.parent, list[list.findIndex((c) => c.id === id) + 1]?.id);
  };
  const commitEdit = async (then?: 'child' | 'sibling') => {
    const e = editing;
    // The input losing focus as it closes must not finish the same edit a second time.
    if (!e || closing.current) return;
    closing.current = true;
    setEditing(null);
    const text = e.text.trim();
    if (!text) { if (e.fresh) await apply([{ op: 'remove', id: e.id }]); refocus(); return; }
    await apply([{ op: 'update', id: e.id, text }]);
    refocus();
    if (then === 'child') void addNode(e.id);
    if (then === 'sibling') addSibling(e.id);
  };
  const removeOrHide = async (n: ResolvedMapNode) => {
    if (n.id === shownMap?.root) return;
    if (n.own) await apply([{ op: 'remove', id: n.id }]);
    else { await apply([{ op: 'hide', id: n.id }]); toast(`${n.title} is hidden from this map — it’s still in the project`); }
    setSel(n.parent || null);
    refocus();
  };

  // Work from a node: the chat is created on its branch.
  const chatFrom = async (n: ResolvedMapNode, prompt: string, title: string) => {
    if (!projectId || starting) return;
    // An agent can take up to a minute to open a session, so the wait has to show.
    setStarting(true);
    try {
      const chat = await post<Chat>(`/api/ws/${wsId()}/chats`, { project: projectId, title: title.slice(0, 60) });
      await refreshSnap();
      // The workspace map shows projects, not their chats, so there is no branch to put it on.
      if (scope !== 'ws') await apply([{ op: 'move', id: 'chat:' + chat.id, parent: n.id }]);
      openChat(chat);
      await post(`/api/ws/${wsId()}/chats/${chat.id}/send`, { text: prompt });
    } catch (e) { toast((e as Error).message, 'error'); }
    finally { setStarting(false); }
  };
  const planWith = (n: ResolvedMapNode) => chatFrom(n, `Break “${n.title}” down into concrete tasks. Use map_read to see the project map, then add them with map_add under the node ${n.id}. Ask me only if something is truly unclear.`, 'Plan · ' + n.title);
  const runGraph = async (n: ResolvedMapNode) => {
    if (!n.ref) return;
    try { const r = await post<{ id: string }>(`/api/ws/${wsId()}/graphs/${n.ref}/run`, {}); toast('Run started'); if (getState().view === 'chat' || getState().view === 'project') openRun(r.id); }
    catch (e) { toast((e as Error).message, 'error'); }
  };

  const nav = (key: string, id: string) => {
    const n = nodeById.get(id), b = layout?.boxes.get(id);
    if (!n || !b) return null;
    const list = (kids.get(id) || []).filter((c) => layout?.boxes.has(c.id));
    const towardRoot = () => n.parent;
    const away = () => (id === shownMap?.root ? [...(layout?.boxes.values() || [])].filter((x) => x.depth === 1 && x.side === (key === 'ArrowLeft' ? 'left' : 'right'))[0]?.id : list[0]?.id);
    if (key === 'ArrowLeft') return id === shownMap?.root ? away() : b.side === 'left' ? away() : towardRoot();
    if (key === 'ArrowRight') return id === shownMap?.root ? away() : b.side === 'left' ? towardRoot() : away();
    // Up and down: the nearest node in that direction at the same depth, so it crosses branches.
    const same = [...(layout?.boxes.values() || [])].filter((x) => x.depth === b.depth && x.side === b.side && x.id !== id && (key === 'ArrowUp' ? x.y < b.y : x.y > b.y));
    same.sort((x, y) => Math.abs(x.y - b.y) - Math.abs(y.y - b.y));
    return same[0]?.id;
  };
  /** Move a node with the keyboard: among its siblings, in or out a level, or across the centre. */
  const shift = (id: string, key: string) => {
    const n = nodeById.get(id), b = layout?.boxes.get(id);
    if (!n?.parent || !b || !shownMap) return;
    const top = n.parent === shownMap.root;
    const list = (kids.get(n.parent) || []).filter((c) => !top || layout?.boxes.get(c.id)?.side === b.side);
    const i = list.findIndex((c) => c.id === id);
    if (key === 'ArrowUp' || key === 'ArrowDown') {
      const j = key === 'ArrowUp' ? i - 1 : i + 2;
      if ((key === 'ArrowUp' && i === 0) || (key === 'ArrowDown' && i === list.length - 1)) return;
      const before = list[j]?.id;
      void apply([{ op: 'move', id, parent: n.parent, ...(before ? { before } : {}), ...(top ? { side: b.side } : {}) }]);
      return;
    }
    const inward = b.side === 'right' ? key === 'ArrowLeft' : key === 'ArrowRight';
    if (inward) {
      if (top) { if (shownMap.layout === 'balance') void apply([{ op: 'move', id, side: b.side === 'right' ? 'left' : 'right' }]); return; }
      // Out a level: right after the branch it was on.
      const parent = nodeById.get(n.parent);
      if (!parent?.parent) return;
      const outer = kids.get(parent.parent) || [];
      const before = outer[outer.findIndex((c) => c.id === parent.id) + 1]?.id;
      void apply([{ op: 'move', id, parent: parent.parent, ...(before ? { before } : {}) }]);
    } else if (i > 0) {
      // In a level: onto the end of the sibling above.
      setOverrides((o) => ({ ...o, [list[i - 1].id]: false }));
      void apply([{ op: 'move', id, parent: list[i - 1].id }]);
    }
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (peek && e.key === 'Escape') { e.preventDefault(); setPeek(null); return; }
    if (editing || (e.target as HTMLElement).closest('input, button')) return;
    if (selLink && (e.key === 'Delete' || e.key === 'Backspace')) {
      e.preventDefault();
      if (selLink.startsWith('auto:')) toast('This link comes from the routine itself'); else void apply([{ op: 'unlink', id: selLink }]);
      setSelLink(null);
      return;
    }
    const id = sel || shownMap?.root;
    const n = id ? nodeById.get(id) : undefined;
    if (!id || !n) return;
    const act = (fn: () => void) => { e.preventDefault(); fn(); };
    if (e.key === 'Tab') return act(() => void addNode(id));
    if (e.key === 'Enter') return act(() => addSibling(id));
    if (e.key === ' ') return act(() => toggle(id));
    if (e.key === 'Delete' || e.key === 'Backspace') return act(() => void removeOrHide(n));
    if (e.key === 'F2' && n.own) return act(() => setEditing({ id, text: n.title }));
    if (e.key === 'Escape') return act(() => { setMenu(null); setSel(null); setSelLink(null); });
    if (e.key.startsWith('Arrow') && e.altKey) return act(() => shift(id, e.key));
    if (e.key === 'p' && !n.own && hasPreview(n)) return act(() => setPeek(id));
    if (e.key.startsWith('Arrow')) return act(() => { const next = nav(e.key, id); if (next) setSel(next); });
    if (n.own && e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) return act(() => setEditing({ id, text: e.key }));
  };

  const exportAs = (format: 'md' | 'json' | 'svg') => {
    setMenu(null);
    if (!map || !layout || !shownMap) return;
    const name = (nodeById.get(map.root)?.title || 'map').toLowerCase().replace(/[^a-z0-9]+/g, '-') + '-map';
    if (format === 'md') download(name + '.md', toMarkdown(map), 'text/markdown');
    else if (format === 'json') download(name + '.json', JSON.stringify(map, null, 2), 'application/json');
    else {
      const b = layout.bounds, m = 40;
      const parts: string[] = [`<svg xmlns="http://www.w3.org/2000/svg" viewBox="${b.x - m} ${b.y - m} ${b.w + m * 2} ${b.h + m * 2}" width="${b.w + m * 2}" height="${b.h + m * 2}" font-family="Inter, system-ui, sans-serif"><rect x="${b.x - m}" y="${b.y - m}" width="${b.w + m * 2}" height="${b.h + m * 2}" fill="#faf9f5"/>`];
      for (const e of layout.edges) { const a = layout.boxes.get(e.from), c = layout.boxes.get(e.to); if (a && c) parts.push(`<path d="${edgePath(a, c)}" fill="none" stroke="#b9b5a8" stroke-width="1.5"/>`); }
      for (const l of shownMap.links) { const a = layout.boxes.get(l.from), c = layout.boxes.get(l.to); if (a && c) parts.push(`<path d="${linkPath(a, c).d}" fill="none" stroke="#c96442" stroke-width="1.5" stroke-dasharray="5 4"/>`); }
      for (const c of layout.boxes.values()) {
        const n = nodeById.get(c.id);
        if (!n) continue;
        parts.push(`<rect x="${c.x}" y="${c.y}" width="${c.w}" height="${c.h}" rx="9" fill="${c.depth === 0 ? '#c96442' : '#ffffff'}" stroke="#d8d4c8"/><text x="${c.x + 12}" y="${c.y + (n.meta ? 20 : c.h / 2 + 4)}" font-size="13" font-weight="500" fill="${c.depth === 0 ? '#ffffff' : '#222'}">${esc(n.title)}</text>${n.meta ? `<text x="${c.x + 12}" y="${c.y + 36}" font-size="11" fill="#777">${esc(n.meta)}</text>` : ''}`);
      }
      download(name + '.svg', parts.join('') + '</svg>', 'image/svg+xml');
    }
    toast('Exported the map');
  };

  const hostBox = host.current?.getBoundingClientRect();
  const latestRun = (graph?: string) => runs.filter((r) => r.graph === graph).sort((a, b) => b.startedAt - a.startedAt)[0];
  const peekNode = peek ? nodeById.get(peek) : undefined;
  const selected = sel ? nodeById.get(sel) : undefined;
  const selBox = sel ? boxes.get(sel) : undefined;
  // The bar sits over its node but stays on the canvas, or the actions at its ends can't be reached.
  useLayoutEffect(() => {
    const el = actionsBar.current, canvasEl = host.current;
    if (!el || !canvasEl || !selBox) return;
    const half = el.offsetWidth / 2, anchor = (selBox.x + selBox.w / 2) * view.k + view.x;
    el.style.left = Math.max(half + 8, Math.min(canvasEl.clientWidth - half - 8, anchor)) + 'px';
  });
  const iconBtn: CSSProperties = { height: 28, width: 28, padding: 0, justifyContent: 'center' };
  const setLayout = (l: MapLayout) => void apply([{ op: 'setLayout', layout: l }]);
  const hiddenItems = map?.hiddenItems || [];

  if (error && !map) return <div className="muted" style={{ margin: 'auto', padding: 24 }}>{error}</div>;
  return (
    <div className="map-wrap">
      <div className="map-bar" onKeyDown={(e) => { if (e.key === 'Escape' && menu) { setMenu(null); refocus(); } }}>
        {compact && <b className="ellipsis" style={{ fontSize: 13, maxWidth: 160 }}>{nodeById.get(map?.root || '')?.title || 'Map'}</b>}
        <label className="map-search"><Icon name="search" size={13} /><input type="search" aria-label="Search the map" placeholder="Search the map" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter' && matches?.size) { const id = [...matches][0]; setSel(id); centre(id); } if (e.key === 'Escape') { setQ(''); refocus(); } }} /></label>
        {matches && <span className="muted mono" style={{ fontSize: 11 }}>{matches.size} found</span>}
        <span className="grow" />
        <span className="map-seg" role="group" aria-label="Layout">
          <button type="button" className="chip" aria-pressed={map?.layout === 'balance'} onClick={() => setLayout('balance')} title="Branches on both sides">Both sides</button>
          <button type="button" className="chip" aria-pressed={map?.layout === 'right'} onClick={() => setLayout('right')} title="All branches to the right">Right</button>
        </span>
        <button type="button" className="chip" aria-pressed={previews} title="Show previews on artifacts and graphs" onClick={() => setPreviews(!previews)}><Icon name="image" size={13} /> Previews</button>
        <span style={{ position: 'relative' }}>
          <button type="button" className="btn" style={iconBtn} aria-label="Show or hide kinds of items" aria-expanded={menu === 'filter'} title="Show or hide kinds of items" onClick={() => setMenu(menu === 'filter' ? null : 'filter')}><Icon name="filter" size={14} /></button>
          {menu === 'filter' && <div className="menu rise map-menu">{FILTERS.map(([k, label]) => <label key={k} className="navrow" style={{ gap: 8 }}><input type="checkbox" checked={!off.includes(k)} onChange={() => setOff((o) => (o.includes(k) ? o.filter((x) => x !== k) : [...o, k]))} />{label}</label>)}</div>}
        </span>
        {!!hiddenItems.length && <span style={{ position: 'relative' }}>
          <button type="button" className="chip" aria-expanded={menu === 'hidden'} title="Items hidden from this map" onClick={() => setMenu(menu === 'hidden' ? null : 'hidden')} style={{ fontSize: 11 }}><Icon name="eyeoff" size={13} /> {hiddenItems.length}</button>
          {menu === 'hidden' && <div className="menu rise map-menu">{hiddenItems.map((h) => <button key={h.id} type="button" className="navrow" onClick={() => void apply([{ op: 'unhide', id: h.id }])} title="Show it again"><span className="ellipsis grow">{h.title}</span><Icon name="plus" size={13} /></button>)}</div>}
        </span>}
        <span style={{ position: 'relative' }}>
          <button type="button" className="btn" style={iconBtn} aria-label="Export the map" aria-expanded={menu === 'export'} title="Export" onClick={() => setMenu(menu === 'export' ? null : 'export')}><Icon name="download" size={14} /></button>
          {menu === 'export' && <div className="menu rise map-menu"><button type="button" className="navrow" onClick={() => exportAs('md')}>Markdown</button><button type="button" className="navrow" onClick={() => exportAs('json')}>JSON</button><button type="button" className="navrow" onClick={() => exportAs('svg')}>SVG picture</button></div>}
        </span>
        <span className="map-seg" role="group" aria-label="Zoom">
          <button type="button" className="btn" style={iconBtn} aria-label="Zoom out" onClick={() => { const r = host.current!.getBoundingClientRect(); zoomAt(r.left + r.width / 2, r.top + r.height / 2, view.k / 1.25); }}><Icon name="minus" size={14} /></button>
          <button type="button" className="btn" style={iconBtn} aria-label="Zoom in" onClick={() => { const r = host.current!.getBoundingClientRect(); zoomAt(r.left + r.width / 2, r.top + r.height / 2, view.k * 1.25); }}><Icon name="plus" size={14} /></button>
          <button type="button" className="btn" style={iconBtn} aria-label="Fit the map to the window" title="Fit" onClick={fit}><Icon name="fullscreen" size={14} /></button>
        </span>
        {onWide && <button type="button" className="btn wide-toggle" style={iconBtn} aria-pressed={wide} aria-label={wide ? 'Show the chat beside the map' : 'Expand the map'} onClick={onWide}><Icon name={wide ? 'collapse' : 'expand'} size={15} /></button>}
        {onClose && <button type="button" className="btn" style={iconBtn} aria-label="Close map" onClick={onClose}><Icon name="x" size={15} /></button>}
      </div>

      <div ref={host} className="map-canvas" tabIndex={0} role="application" aria-label="Mind map. Tab adds a child, Enter a sibling, arrows move around, Alt with arrows moves the node, Space collapses, P previews." onKeyDown={onKey}
        onPointerDown={(e) => {
          setMenu(null);
          pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
          if (pointers.current.size === 2) { const [a, b] = [...pointers.current.values()]; pinch.current = { d: Math.hypot(a.x - b.x, a.y - b.y) || 1, k: viewRef.current.k }; gesture.current = null; return; }
          if (e.target !== e.currentTarget && !(e.target as HTMLElement).classList?.contains('map-layer')) return;
          setSel(null); setSelLink(null);
          gesture.current = { type: 'pan', sx: e.clientX, sy: e.clientY, ox: view.x, oy: view.y };
        }}>
        <div className="map-layer" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.k})` }}>
          <svg className="map-edges" width="1" height="1" aria-hidden={false}>
            {layout?.edges.map((e) => { const a = drawn.get(e.from), b = drawn.get(e.to); return a && b ? <path key={e.from + '>' + e.to} d={edgePath(a, b)} className="map-edge" /> : null; })}
            {shownMap?.links.map((l) => {
              const a = drawn.get(l.from), b = drawn.get(l.to);
              if (!a || !b) return null;
              const p = linkPath(a, b);
              return <g key={l.id}>
                <path d={p.d} className={'map-link' + (selLink === l.id ? ' on' : '')} />
                <path d={p.d} className="map-link-hit" onPointerDown={(e) => { e.stopPropagation(); pointers.current.delete(e.pointerId); setSelLink(l.id); setSel(null); refocus(); }}><title>{l.label || 'Link'} — select and press Delete to remove</title></path>
                {l.label && <text x={p.x} y={p.y - 4} className="map-link-label" textAnchor="middle">{l.label}</text>}
              </g>;
            })}
            {rubber && drawn.get(rubber.from) && <path d={`M${drawn.get(rubber.from)!.x + drawn.get(rubber.from)!.w / 2} ${drawn.get(rubber.from)!.y + drawn.get(rubber.from)!.h / 2} L${rubber.x} ${rubber.y}`} className="map-link on" />}
          </svg>
          {[...drawn.values()].map((b) => {
            const n = nodeById.get(b.id);
            if (!n) return null;
            const isRoot = b.id === shownMap?.root;
            const count = (kids.get(b.id) || []).length;
            const dropping = drop?.target === b.id ? ' drop-' + drop.zone : '';
            const dim = matches && !matches.has(b.id) ? ' dim' : '';
            const thumb = previews && hasThumb(n);
            const moving = dragging?.ids.has(b.id) ? ' moving' : '';
            // Previews load once a node has come into view, and stay.
            if (thumb && hostBox && (b.x + b.w) * view.k + view.x > -80 && b.x * view.k + view.x < hostBox.width + 80 && (b.y + b.h) * view.k + view.y > -80 && b.y * view.k + view.y < hostBox.height + 80) seen.current.add(b.id);
            const head = <>
                {n.kind === 'task'
                  ? <button type="button" className={'map-check st-' + (n.status || 'todo')} aria-label={`Status: ${n.status || 'todo'}. Change it`} title={n.status || 'todo'} onPointerDown={(e) => e.stopPropagation()} onClick={(e) => { e.stopPropagation(); void apply([{ op: 'update', id: n.id, status: NEXT_STATUS[n.status || 'todo'] }]); }}><Icon name={STATUS_ICON[n.status || 'todo']} size={14} stroke={2.25} /></button>
                  : <span className="map-ico"><Icon name={iconOf(n)} size={isRoot ? 17 : 14} /></span>}
                {editing?.id === b.id
                  ? <input className="map-edit" autoFocus value={editing.text} aria-label="Node text" placeholder="Type…" onPointerDown={(e) => e.stopPropagation()} onChange={(e) => setEditing({ ...editing, text: e.target.value })} onBlur={() => void commitEdit()} onKeyDown={(e) => { e.stopPropagation(); if (e.key === 'Enter') { e.preventDefault(); void commitEdit('sibling'); } if (e.key === 'Tab') { e.preventDefault(); void commitEdit('child'); } if (e.key === 'Escape') { e.preventDefault(); closing.current = true; if (editing.fresh) void apply([{ op: 'remove', id: b.id }]); setEditing(null); refocus(); } }} />
                  : <span className="map-txt"><span className="ellipsis map-title" title={n.title}>{n.title}</span>{n.meta && <span className="ellipsis map-meta" title={n.meta}>{n.meta}</span>}</span>}
                {n.kind === 'chat' && n.ref && <ChatActivity chatId={n.ref} />}
            </>;
            return (
              <div key={b.id} className={`map-node kind-${n.kind}${isRoot ? ' root' : ''}${sel === b.id ? ' sel' : ''}${dropping}${dim}${moving}${thumb ? ' has-thumb' : ''}${n.status === 'done' ? ' done' : ''}`} style={{ transform: `translate(${b.x}px, ${b.y}px)`, width: b.w, height: b.h }} data-node={b.id} role="treeitem" aria-selected={sel === b.id} aria-label={n.title}
                onPointerDown={(e) => { if (e.button !== 0) return; e.stopPropagation(); pointers.current.delete(e.pointerId); setMenu(null); setSel(b.id); setSelLink(null); gesture.current = { type: 'node', id: b.id, sx: e.clientX, sy: e.clientY, ...(() => { const m = toMapXY(e.clientX, e.clientY); return { mx: m.x, my: m.y }; })(), moved: false }; refocus(); }}
                onDoubleClick={() => { if (n.own) setEditing({ id: n.id, text: n.title }); else if (n.kind === 'ticket') setPeek(n.id); else if (canOpen(n)) openNode(n); }}>
                {thumb ? <>
                  <div className="map-head">{head}</div>
                  <div className="map-thumb">{seen.current.has(b.id) && (n.kind === 'artifact'
                    ? <ArtifactPreview id={n.ref!} version={String(snap.artifacts.find((a) => a.id === n.ref)?.version ?? 0)} width={THUMB_W} height={THUMB_H} page={n.subtype === 'Doc' || n.subtype === 'Sheet' ? 640 : 1200} />
                    : <GraphPreviewView id={n.ref!} version={String(snap.graphs.find((g) => g.id === n.ref)?.updatedAt ?? 0)} run={latestRun(n.ref)} />)}</div>
                </> : head}
                <button type="button" className="map-handle" aria-label={`Link ${n.title} to another node`} title="Drag to another node to link them" onPointerDown={(e) => { e.stopPropagation(); pointers.current.delete(e.pointerId); gesture.current = { type: 'link', from: b.id }; }} />
                {count > 0 && !isRoot && <button type="button" className={'map-fold ' + b.side} aria-label={isCollapsed(n, open) ? `Open ${n.title}` : `Collapse ${n.title}`} aria-expanded={!isCollapsed(n, open)} onPointerDown={(e) => e.stopPropagation()} onClick={(e) => { e.stopPropagation(); toggle(b.id); }}>{isCollapsed(n, open) ? b.hiddenBelow : <Icon name="minus" size={10} stroke={2.5} />}</button>}
              </div>
            );
          })}
          {ghost && drop?.zone === 'side' && gesture.current?.type === 'node' && <div className="map-ghost" style={{ transform: `translate(${ghost.x + 10}px, ${ghost.y + 10}px)` }}>{nodeById.get(gesture.current.id)?.parent === shownMap?.root && shownMap?.layout === 'balance' ? `Move to the ${ghost.x < 0 ? 'left' : 'right'} side` : 'Drop near a node to place it here'}</div>}
        </div>

        {selected && selBox && !editing && !dragging && <div ref={actionsBar} className="map-actions" style={{ top: Math.max(8, selBox.y * view.k + view.y - 42) }} onPointerDown={(e) => e.stopPropagation()}>
          {hasPreview(selected) && <button type="button" className="chip" onClick={() => setPeek(selected.id)} title="P"><Icon name="image" size={12} /> Preview</button>}
          {canOpen(selected) && !opensInPlace(selected) && <button type="button" className="chip" onClick={() => openNode(selected)}><Icon name="external" size={12} /> Open</button>}
          {selected.kind === 'graph' && <button type="button" className="chip" onClick={() => void runGraph(selected)}><Icon name="play" size={12} /> Run</button>}
          {selected.needsToken && (tokenFor === selected.id
            ? <form className="row" style={{ gap: 4 }} onSubmit={(e) => { e.preventDefault(); addToken(selected); }}><input className="inp" type="password" autoFocus autoComplete="off" aria-label={`GitLab token for ${selected.ref}`} placeholder={`read_api token for ${selected.ref}`} value={tokenDraft} onChange={(e) => setTokenDraft(e.target.value)} style={{ height: 26, width: 210 }} /><button type="submit" className="chip" disabled={!tokenDraft.trim()}>Save</button></form>
            : <button type="button" className="chip" onClick={() => setTokenFor(selected.id)}><Icon name="lock" size={12} /> Add token</button>)}
          <button type="button" className="chip" onClick={() => void addNode(selected.id)} title="Tab"><Icon name="plus" size={12} /> Add</button>
          {selected.own && <button type="button" className="chip" onClick={() => setEditing({ id: selected.id, text: selected.title })} title="F2"><Icon name="compose" size={12} /> Edit</button>}
          {selected.own && selected.kind !== 'task' && <button type="button" className="chip" onClick={() => void apply([{ op: 'update', id: selected.id, kind: 'task' }])}><Icon name="check" size={12} /> Make task</button>}
          {selected.own && (starting
            ? <span className="chip pulse" role="status"><Icon name="chat" size={12} /> Starting the chat…</span>
            : <>
              <button type="button" className="chip" onClick={() => void chatFrom(selected, selected.title, selected.title)}><Icon name="chat" size={12} /> Start a chat</button>
              <button type="button" className="chip" onClick={() => void planWith(selected)}><Icon name="sparkle" size={12} /> Plan with the agent</button>
            </>)}
          {selected.id !== shownMap?.root && <>
            <button type="button" className="chip" aria-label="Move up" title="Move up (Alt+↑)" onClick={() => shift(selected.id, 'ArrowUp')}><Icon name="up" size={12} /></button>
            <button type="button" className="chip" aria-label="Move down" title="Move down (Alt+↓)" onClick={() => shift(selected.id, 'ArrowDown')}><Icon name="down" size={12} /></button>
          </>}
          {selected.id !== shownMap?.root && <button type="button" className="chip" aria-label={selected.own ? 'Remove' : 'Hide from this map'} title={selected.own ? 'Remove' : 'Hide from this map'} onClick={() => void removeOrHide(selected)}><Icon name={selected.own ? 'trash' : 'eyeoff'} size={12} /></button>}
        </div>}

        {peekNode && <div className="map-peek-scrim" onPointerDown={(e) => { e.stopPropagation(); if (e.target === e.currentTarget) { setPeek(null); refocus(); } }}>
          <div className="map-peek" role="dialog" aria-label={`Preview of ${peekNode.title}`}>
            <header className="row" style={{ gap: 8, padding: '10px 12px', borderBottom: '1px solid var(--bg2)' }}>
              <span className="map-ico"><Icon name={iconOf(peekNode)} size={16} /></span>
              <span className="grow" style={{ display: 'flex', flexDirection: 'column', minWidth: 0 }}><b className="ellipsis" style={{ fontSize: 14 }}>{peekNode.kind === 'ticket' ? `Ticket #${peekNode.ref}` : peekNode.title}</b>{peekNode.meta && peekNode.kind !== 'ticket' && <span className="muted ellipsis" style={{ fontSize: 11 }}>{peekNode.meta}</span>}</span>
              {peekNode.kind === 'graph' && <button type="button" className="btn" style={{ height: 28 }} onClick={() => { setPeek(null); void runGraph(peekNode); }}><Icon name="play" size={13} /> Run</button>}
              {!opensInPlace(peekNode) && <button type="button" className="btn btn-p" style={{ height: 28 }} onClick={() => { setPeek(null); openNode(peekNode); }}><Icon name="external" size={13} /> Open</button>}
              <button type="button" className="btn" style={iconBtn} aria-label="Close preview" onClick={() => { setPeek(null); refocus(); }}><Icon name="x" size={15} /></button>
            </header>
            <div className="map-peek-body">{peekNode.kind === 'ticket' ? <TicketPreview project={scope} iid={peekNode.ref!} /> : peekNode.kind === 'memory' ? <MemoryPreview id={peekNode.ref!} /> : peekNode.kind === 'fact' ? <div className="map-memory"><p style={{ margin: 0, whiteSpace: 'pre-wrap' }}>{snap.facts.find((f) => f.id === peekNode.ref)?.text}</p></div> : peekNode.kind === 'artifact'
              ? <ArtifactPreview large id={peekNode.ref!} version={String(snap.artifacts.find((a) => a.id === peekNode.ref)?.version ?? 0)} />
              : <GraphPreviewView large id={peekNode.ref!} version={String(snap.graphs.find((g) => g.id === peekNode.ref)?.updatedAt ?? 0)} run={latestRun(peekNode.ref)} />}</div>
          </div>
        </div>}

        {!map && !error && <div className="muted pulse map-hint">Loading the map…</div>}
        {map && map.nodes.length === 1 && <div className="muted map-hint">Nothing here yet. Press Tab to add the first idea; chats, artifacts and graphs appear by themselves.</div>}
      </div>
    </div>
  );
}
