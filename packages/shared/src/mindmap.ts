// Mind map layout and text formats, free of any view so the UI, agents and tests share them. The layout is a two-pass
// tidy tree: subtree sizes from the leaves up, then positions from the root down. Each top-level branch keeps its own
// side, so a balanced map never rearranges itself when a branch is added.
import type { MapSide, MapTaskStatus, ResolvedMap, ResolvedMapNode } from './types.ts';

export const SIBLING_GAP = 40;
export const LEVEL_GAP = 90;

export interface MapBox { id: string; x: number; y: number; w: number; h: number; side: MapSide; depth: number; hiddenBelow: number }
export interface MapLayoutResult { boxes: Map<string, MapBox>; edges: { from: string; to: string }[]; bounds: { x: number; y: number; w: number; h: number } }
export type MapSize = (n: ResolvedMapNode) => { w: number; h: number };

/** Children of each node, in order. */
export function childrenOf(map: ResolvedMap): Map<string, ResolvedMapNode[]> {
  const out = new Map<string, ResolvedMapNode[]>();
  for (const n of map.nodes) if (n.parent) { const l = out.get(n.parent) || []; l.push(n); out.set(n.parent, l); }
  for (const l of out.values()) l.sort((a, b) => a.order - b.order || a.id.localeCompare(b.id));
  return out;
}

/** The side a top-level branch sits on when balanced: the one saved for it, else one fixed by its id. */
export function sideOf(n: ResolvedMapNode): MapSide {
  if (n.side) return n.side;
  let h = 0;
  for (let i = 0; i < n.id.length; i++) h = (h * 31 + n.id.charCodeAt(i)) | 0;
  return h & 1 ? 'left' : 'right';
}

export function isCollapsed(n: ResolvedMapNode, overrides: Record<string, boolean>): boolean {
  return overrides[n.id] ?? !!n.folded;
}

export function layoutMap(map: ResolvedMap, size: MapSize, overrides: Record<string, boolean> = {}): MapLayoutResult {
  const kids = childrenOf(map);
  const byId = new Map(map.nodes.map((n) => [n.id, n]));
  const boxes = new Map<string, MapBox>();
  const edges: { from: string; to: string }[] = [];
  const root = byId.get(map.root);
  if (!root) return { boxes, edges, bounds: { x: 0, y: 0, w: 0, h: 0 } };
  const shown = (n: ResolvedMapNode) => (isCollapsed(n, overrides) ? [] : kids.get(n.id) || []);
  const count = (n: ResolvedMapNode): number => (kids.get(n.id) || []).reduce((sum, c) => sum + 1 + count(c), 0);
  const heights = new Map<string, number>();
  const measure = (n: ResolvedMapNode): number => {
    const own = size(n).h;
    const list = shown(n);
    const all = list.reduce((sum, c, i) => sum + measure(c) + (i ? SIBLING_GAP : 0), 0);
    const h = Math.max(own, all);
    heights.set(n.id, h);
    return h;
  };
  const place = (n: ResolvedMapNode, x: number, top: number, side: MapSide, depth: number) => {
    const { w, h } = size(n);
    const band = heights.get(n.id) || h;
    boxes.set(n.id, { id: n.id, x: side === 'right' ? x : x - w, y: top + (band - h) / 2, w, h, side, depth, hiddenBelow: isCollapsed(n, overrides) ? count(n) : 0 });
    const list = shown(n);
    const all = list.reduce((sum, c, i) => sum + (heights.get(c.id) || 0) + (i ? SIBLING_GAP : 0), 0);
    let y = top + (band - all) / 2;
    const nextX = side === 'right' ? x + w + LEVEL_GAP : x - w - LEVEL_GAP;
    for (const c of list) {
      edges.push({ from: n.id, to: c.id });
      place(c, nextX, y, side, depth + 1);
      y += (heights.get(c.id) || 0) + SIBLING_GAP;
    }
  };
  // The root sits at the origin; its branches go to their sides.
  const rootSize = size(root);
  boxes.set(root.id, { id: root.id, x: -rootSize.w / 2, y: -rootSize.h / 2, w: rootSize.w, h: rootSize.h, side: 'right', depth: 0, hiddenBelow: isCollapsed(root, overrides) ? count(root) : 0 });
  const top = shown(root);
  const sides: Record<MapSide, ResolvedMapNode[]> = { left: [], right: [] };
  for (const c of top) sides[map.layout === 'balance' ? sideOf(c) : 'right'].push(c);
  for (const side of ['left', 'right'] as MapSide[]) {
    const list = sides[side];
    const all = list.reduce((sum, c, i) => sum + measure(c) + (i ? SIBLING_GAP : 0), 0);
    let y = -all / 2;
    const x = side === 'right' ? rootSize.w / 2 + LEVEL_GAP : -rootSize.w / 2 - LEVEL_GAP;
    for (const c of list) {
      edges.push({ from: root.id, to: c.id });
      place(c, x, y, side, 1);
      y += (heights.get(c.id) || 0) + SIBLING_GAP;
    }
  }
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const b of boxes.values()) { minX = Math.min(minX, b.x); minY = Math.min(minY, b.y); maxX = Math.max(maxX, b.x + b.w); maxY = Math.max(maxY, b.y + b.h); }
  return { boxes, edges, bounds: { x: minX, y: minY, w: maxX - minX, h: maxY - minY } };
}

// ---------- text formats ----------
const CHECK: Record<MapTaskStatus, string> = { todo: '[ ] ', doing: '[~] ', done: '[x] ' };

/** The map as a nested bullet list; tasks as checkboxes. */
export function toMarkdown(map: ResolvedMap): string {
  const kids = childrenOf(map);
  const root = map.nodes.find((n) => n.id === map.root);
  if (!root) return '';
  const lines = [`# ${root.title}`, ''];
  const walk = (n: ResolvedMapNode, depth: number) => {
    for (const c of kids.get(n.id) || []) {
      lines.push(`${'  '.repeat(depth)}- ${c.kind === 'task' ? CHECK[c.status || 'todo'] : ''}${c.title}`);
      walk(c, depth + 1);
    }
  };
  walk(root, 0);
  return lines.join('\n') + '\n';
}

/** An outline for agents: every node with the id to use in the map tools. */
export function toOutline(map: ResolvedMap): string {
  const kids = childrenOf(map);
  const root = map.nodes.find((n) => n.id === map.root);
  if (!root) return '';
  const label = (n: ResolvedMapNode) => `${n.kind === 'task' ? CHECK[n.status || 'todo'] : ''}${n.title} (${n.kind}${n.meta ? ', ' + n.meta : ''}) [${n.id}]`;
  const lines = [label(root)];
  const walk = (n: ResolvedMapNode, depth: number) => { for (const c of kids.get(n.id) || []) { lines.push(`${'  '.repeat(depth)}- ${label(c)}`); walk(c, depth + 1); } };
  walk(root, 0);
  if (map.links.length) lines.push('', 'Links:', ...map.links.map((l) => `- ${l.from} → ${l.to}${l.label ? ` (${l.label})` : ''} [${l.id}]`));
  return lines.join('\n');
}

export interface OutlineItem { text: string; status?: MapTaskStatus; children: OutlineItem[] }

/** A nested bullet list as a tree. Checkbox items are tasks; other lines are ignored. */
export function fromMarkdown(md: string): OutlineItem[] {
  const out: OutlineItem[] = [];
  const stack: { indent: number; item: OutlineItem }[] = [];
  for (const line of md.replace(/\r/g, '').split('\n')) {
    const m = /^(\s*)[-*+]\s+(?:\[([ xX~])\]\s+)?(.+?)\s*$/.exec(line);
    if (!m) continue;
    const indent = m[1].replace(/\t/g, '  ').length;
    const status: MapTaskStatus | undefined = m[2] === undefined ? undefined : m[2] === ' ' ? 'todo' : m[2] === '~' ? 'doing' : 'done';
    const item: OutlineItem = { text: m[3], ...(status ? { status } : {}), children: [] };
    while (stack.length && stack[stack.length - 1].indent >= indent) stack.pop();
    (stack.length ? stack[stack.length - 1].item.children : out).push(item);
    stack.push({ indent, item });
  }
  return out;
}
