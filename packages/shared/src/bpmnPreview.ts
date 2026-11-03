// A graph's picture from its BPMN file: the shapes and paths its diagram already holds, without loading an editor.
export interface GraphPreviewShape { id: string; kind: 'start' | 'end' | 'event' | 'gateway' | 'user' | 'task'; name: string; x: number; y: number; w: number; h: number }
export interface GraphPreview { shapes: GraphPreviewShape[]; edges: { id: string; points: { x: number; y: number }[] }[]; bounds: { x: number; y: number; w: number; h: number } }

const attr = (text: string, name: string) => new RegExp(`\\b${name}="([^"]*)"`).exec(text)?.[1];
const unescape = (v: string) => v.replace(/&quot;/g, '"').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');

function kindOf(tag: string): GraphPreviewShape['kind'] {
  if (tag === 'startEvent') return 'start';
  if (tag === 'endEvent') return 'end';
  if (/Event$/.test(tag)) return 'event';
  if (/Gateway$/.test(tag)) return 'gateway';
  if (tag === 'userTask') return 'user';
  return 'task';
}

/** Null when the file holds no diagram to draw. */
export function graphPreview(xml: string): GraphPreview | null {
  const elements = new Map<string, { tag: string; name: string }>();
  for (const m of xml.matchAll(/<(?:\w+:)?(\w+)\b([^>]*)>/g)) {
    const id = attr(m[2], 'id');
    if (id) elements.set(id, { tag: m[1], name: unescape(attr(m[2], 'name') || '') });
  }
  const shapes: GraphPreviewShape[] = [];
  for (const m of xml.matchAll(/<(?:\w+:)?BPMNShape\b([^>]*)>[\s\S]*?<(?:\w+:)?Bounds\b([^>]*?)\/?>/g)) {
    const id = attr(m[1], 'bpmnElement');
    const x = Number(attr(m[2], 'x')), y = Number(attr(m[2], 'y')), w = Number(attr(m[2], 'width')), h = Number(attr(m[2], 'height'));
    if (!id || ![x, y, w, h].every(Number.isFinite)) continue;
    const el = elements.get(id);
    shapes.push({ id, kind: kindOf(el?.tag || ''), name: el?.name || '', x, y, w, h });
  }
  if (!shapes.length) return null;
  const edges: GraphPreview['edges'] = [];
  for (const m of xml.matchAll(/<(?:\w+:)?BPMNEdge\b([^>]*)>([\s\S]*?)<\/(?:\w+:)?BPMNEdge>/g)) {
    const points = [...m[2].matchAll(/<(?:\w+:)?waypoint\b([^>]*?)\/?>/g)].map((p) => ({ x: Number(attr(p[1], 'x')), y: Number(attr(p[1], 'y')) })).filter((p) => Number.isFinite(p.x) && Number.isFinite(p.y));
    if (points.length > 1) edges.push({ id: attr(m[1], 'bpmnElement') || '', points });
  }
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  const take = (x: number, y: number) => { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); };
  for (const s of shapes) { take(s.x, s.y); take(s.x + s.w, s.y + s.h); }
  for (const e of edges) for (const p of e.points) take(p.x, p.y);
  return { shapes, edges, bounds: { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } };
}
