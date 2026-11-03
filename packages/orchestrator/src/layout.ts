// A layout for BPMN files that come without one (written by hand or by an agent): the diagram (BPMN DI) the editor
// draws and the model check asks for. Steps are laid left to right by their distance from the start.

const NODE = /^(startEvent|endEvent|intermediateCatchEvent|intermediateThrowEvent|task|\w+Task|subProcess|callActivity|\w+Gateway)$/;

function size(kind: string): [number, number] {
  if (/Event$/.test(kind)) return [36, 36];
  if (/Gateway$/.test(kind)) return [50, 50];
  return [100, 80];
}

/** The same XML with a diagram added when it has none; unchanged when it already has one or isn't BPMN. */
export function withLayout(xml: string): string {
  if (/BPMNDiagram\b/.test(xml)) return xml;
  const proc = /<(?:\w+:)?process\b[^>]*\bid="([^"]+)"/.exec(xml);
  const defs = /<((?:\w+:)?)definitions\b([^>]*)>/.exec(xml);
  if (!proc || !defs) return xml;
  const nodes: { id: string; kind: string }[] = [];
  const flows: { id: string; from: string; to: string }[] = [];
  const tag = /<(?:\w+:)?(\w+)\b([^>]*?)\/?>/g;
  for (let m = tag.exec(xml); m; m = tag.exec(xml)) {
    const id = /\bid="([^"]+)"/.exec(m[2])?.[1];
    if (!id) continue;
    if (m[1] === 'sequenceFlow') {
      const from = /\bsourceRef="([^"]+)"/.exec(m[2])?.[1], to = /\btargetRef="([^"]+)"/.exec(m[2])?.[1];
      if (from && to) flows.push({ id, from, to });
    } else if (NODE.test(m[1])) nodes.push({ id, kind: m[1] });
  }
  if (!nodes.length) return xml;
  // Columns: the longest path from a node nothing flows into (loops don't push columns further).
  const col = new Map<string, number>(nodes.map((n) => [n.id, 0]));
  for (let pass = 0; pass < nodes.length; pass++) {
    let moved = false;
    for (const f of flows) {
      const next = (col.get(f.from) ?? 0) + 1;
      if (col.has(f.to) && next > (col.get(f.to) ?? 0) && next < nodes.length) { col.set(f.to, next); moved = true; }
    }
    if (!moved) break;
  }
  const rows = new Map<number, number>();
  const box = new Map<string, { x: number; y: number; w: number; h: number }>();
  for (const n of nodes) {
    const c = col.get(n.id) ?? 0, r = rows.get(c) ?? 0;
    rows.set(c, r + 1);
    const [w, h] = size(n.kind);
    box.set(n.id, { x: 160 + c * 170 + (100 - w) / 2, y: 120 + r * 130 + (80 - h) / 2, w, h });
  }
  const shapes = nodes.map((n) => { const b = box.get(n.id)!; return `<bpmndi:BPMNShape id="${n.id}_di" bpmnElement="${n.id}"><dc:Bounds x="${b.x}" y="${b.y}" width="${b.w}" height="${b.h}" /></bpmndi:BPMNShape>`; }).join('');
  const edges = flows.map((f) => {
    const a = box.get(f.from), b = box.get(f.to);
    if (!a || !b) return '';
    return `<bpmndi:BPMNEdge id="${f.id}_di" bpmnElement="${f.id}"><di:waypoint x="${a.x + a.w}" y="${a.y + a.h / 2}" /><di:waypoint x="${b.x}" y="${b.y + b.h / 2}" /></bpmndi:BPMNEdge>`;
  }).join('');
  const ns: Record<string, string> = { bpmndi: 'http://www.omg.org/spec/BPMN/20100524/DI', dc: 'http://www.omg.org/spec/DD/20100524/DC', di: 'http://www.omg.org/spec/DD/20100524/DI' };
  const missing = Object.entries(ns).filter(([p]) => !new RegExp(`xmlns:${p}=`).test(defs[2])).map(([p, u]) => ` xmlns:${p}="${u}"`).join('');
  const diagram = `<bpmndi:BPMNDiagram id="Diagram_auto"><bpmndi:BPMNPlane id="Plane_auto" bpmnElement="${proc[1]}">${shapes}${edges}</bpmndi:BPMNPlane></bpmndi:BPMNDiagram>`;
  return xml.replace(defs[0], defs[0].replace(/\s*>$/, missing + '>')).replace(new RegExp(`</${defs[1]}definitions>\\s*$`), `${diagram}</${defs[1]}definitions>`);
}

/**
 * Tasks that name a harness step type (harness:type="script", "agent"…) run through the harness, whatever BPMN task
 * they were written as: they become service tasks that call it, as the editor makes them.
 */
export function withHarnessSteps(xml: string): string {
  const IMPL = 'implementation="${environment.services.agent}"';
  const isStep = (attrs: string) => /\bharness:type="/.test(attrs) && !/\bimplementation="/.test(attrs);
  return xml
    .replace(/<((?:\w+:)?)(task|scriptTask|sendTask|serviceTask|businessRuleTask)\b([^>]*?)\/>/g, (all, p: string, _t: string, attrs: string) => (isStep(attrs) ? `<${p}serviceTask${attrs} ${IMPL} />` : all))
    .replace(/<((?:\w+:)?)(task|scriptTask|sendTask|serviceTask|businessRuleTask)\b([^>]*?)>([\s\S]*?)<\/\1\2>/g, (all, p: string, _t: string, attrs: string, body: string) => (isStep(attrs) ? `<${p}serviceTask${attrs} ${IMPL}>${body}</${p}serviceTask>` : all));
}
