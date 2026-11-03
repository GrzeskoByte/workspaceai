// BPMN 2.0 patterns and the harness moddle extension (R-24, R-25, R-26, R-37).
// Agent steps are service tasks. Their name is the prompt. harness:agent and harness:prompt
// are extra attributes the editor can store; bpmn-engine runs the service task itself.

export const HARNESS_ATTRS = ['type', 'runner', 'agent', 'model', 'tools', 'prompt', 'askFirst', 'maxLoops', 'mcp', 'url', 'command'] as const;
export type HarnessAttr = typeof HARNESS_ATTRS[number];
/** harness:type values: what a task does when a token reaches it. */
export const HARNESS_TYPES = ['agent', 'browser', 'script', 'service', 'rule'] as const;

/** Moddle extension: harness attributes survive saveXML/importXML; other BPMN tools ignore them (R-37). */
export const HARNESS_MODDLE = {
  name: 'Harness',
  uri: 'http://harness.local/schema/bpmn',
  prefix: 'harness',
  xml: { tagAlias: 'lowerCase' },
  types: [
    {
      name: 'HarnessAttrs',
      extends: ['bpmn:FlowNode'],
      properties: HARNESS_ATTRS.map((name) => ({ name, isAttr: true, type: 'String' })),
    },
  ],
};

export interface GraphPattern { id: string; name: string; blurb: string; xml: string }

type Kind = 'start' | 'end' | 'task' | 'user' | 'xor' | 'and';
interface Node { id: string; kind: Kind; name?: string; x: number; y: number; gatewayDefault?: string }
interface Edge { id: string; from: string; to: string; cond?: string }

function size(kind: Kind): [number, number] {
  if (kind === 'start' || kind === 'end') return [36, 36];
  if (kind === 'xor' || kind === 'and') return [50, 50];
  return [100, 80];
}

function esc(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

/** A diagram bpmn-js can open. Coordinates are explicit so the canvas is never blank. */
export function buildDiagram(name: string, nodes: Node[], edges: Edge[]): string {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const flows = edges.map((e) => {
    const cond = e.cond ? `<bpmn:conditionExpression xsi:type="bpmn:tFormalExpression">${esc(e.cond)}</bpmn:conditionExpression>` : '';
    return `<bpmn:sequenceFlow id="${e.id}" sourceRef="${e.from}" targetRef="${e.to}">${cond}</bpmn:sequenceFlow>`;
  }).join('');
  const els = nodes.map((n) => {
    const name = n.name ? ` name="${esc(n.name)}"` : '';
    const def = n.gatewayDefault ? ` default="${n.gatewayDefault}"` : '';
    if (n.kind === 'start') return `<bpmn:startEvent id="${n.id}"${name} />`;
    if (n.kind === 'end') return `<bpmn:endEvent id="${n.id}"${name} />`;
    if (n.kind === 'user') return `<bpmn:userTask id="${n.id}"${name} />`;
    if (n.kind === 'xor') return `<bpmn:exclusiveGateway id="${n.id}"${name}${def} />`;
    if (n.kind === 'and') return `<bpmn:parallelGateway id="${n.id}"${name} />`;
    return `<bpmn:serviceTask id="${n.id}"${name} implementation="\${environment.services.agent}" harness:type="agent" harness:agent="opencode" harness:prompt="${esc(n.name || n.id)}" />`;
  }).join('');
  const shapes = nodes.map((n) => {
    const [w, h] = size(n.kind);
    return `<bpmndi:BPMNShape id="${n.id}_di" bpmnElement="${n.id}"><dc:Bounds x="${n.x}" y="${n.y}" width="${w}" height="${h}" /></bpmndi:BPMNShape>`;
  }).join('');
  const diEdges = edges.map((e) => {
    const a = byId.get(e.from);
    const b = byId.get(e.to);
    if (!a || !b) return '';
    const [aw, ah] = size(a.kind);
    const [, bh] = size(b.kind);
    const x1 = a.x + aw;
    const y1 = a.y + ah / 2;
    const x2 = b.x;
    const y2 = b.y + bh / 2;
    return `<bpmndi:BPMNEdge id="${e.id}_di" bpmnElement="${e.id}"><di:waypoint x="${x1}" y="${y1}" /><di:waypoint x="${x2}" y="${y2}" /></bpmndi:BPMNEdge>`;
  }).join('');
  return `<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI" xmlns:dc="http://www.omg.org/spec/DD/20100524/DC" xmlns:di="http://www.omg.org/spec/DD/20100524/DI" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:harness="http://harness.local/schema/bpmn" id="Definitions_1" targetNamespace="http://bpmn.io/schema/bpmn">
  <bpmn:process id="Process_1" name="${esc(name)}" isExecutable="true">${els}${flows}</bpmn:process>
  <bpmndi:BPMNDiagram id="Diagram_1"><bpmndi:BPMNPlane id="Plane_1" bpmnElement="Process_1">${shapes}${diEdges}</bpmndi:BPMNPlane></bpmndi:BPMNDiagram>
</bpmn:definitions>`;
}

export function blankGraph(name = 'New graph'): string {
  return buildDiagram(name, [
    { id: 'Start', kind: 'start', x: 160, y: 202 },
    { id: 'Task_1', kind: 'task', name: 'Describe this step', x: 260, y: 180 },
    { id: 'End', kind: 'end', x: 430, y: 202 },
  ], [
    { id: 'Flow_1', from: 'Start', to: 'Task_1' },
    { id: 'Flow_2', from: 'Task_1', to: 'End' },
  ]);
}

export const PATTERNS: GraphPattern[] = [
  {
    id: 'chain', name: 'Prompt chain', blurb: 'One step, then the next',
    xml: buildDiagram('Prompt chain', [
      { id: 'Start', kind: 'start', x: 140, y: 202 },
      { id: 'Draft', kind: 'task', name: 'Write the draft', x: 240, y: 180 },
      { id: 'Revise', kind: 'task', name: 'Revise the draft', x: 410, y: 180 },
      { id: 'End', kind: 'end', x: 580, y: 202 },
    ], [
      { id: 'Flow_1', from: 'Start', to: 'Draft' },
      { id: 'Flow_2', from: 'Draft', to: 'Revise' },
      { id: 'Flow_3', from: 'Revise', to: 'End' },
    ]),
  },
  {
    id: 'router', name: 'Router', blurb: 'Pick one path',
    xml: buildDiagram('Router', [
      { id: 'Start', kind: 'start', x: 120, y: 202 },
      { id: 'Route', kind: 'xor', name: 'Which path?', x: 210, y: 195, gatewayDefault: 'Flow_b' },
      { id: 'PathA', kind: 'task', name: 'Answer as a summary', x: 330, y: 80 },
      { id: 'PathB', kind: 'task', name: 'Answer in detail', x: 330, y: 280 },
      { id: 'Join', kind: 'xor', x: 500, y: 195 },
      { id: 'End', kind: 'end', x: 610, y: 202 },
    ], [
      { id: 'Flow_1', from: 'Start', to: 'Route' },
      { id: 'Flow_a', from: 'Route', to: 'PathA', cond: '${environment.variables.route == "summary"}' },
      { id: 'Flow_b', from: 'Route', to: 'PathB' },
      { id: 'Flow_aj', from: 'PathA', to: 'Join' },
      { id: 'Flow_bj', from: 'PathB', to: 'Join' },
      { id: 'Flow_end', from: 'Join', to: 'End' },
    ]),
  },
  {
    id: 'parallel', name: 'Parallel + vote', blurb: 'Two answers, then a person picks',
    xml: buildDiagram('Parallel and vote', [
      { id: 'Start', kind: 'start', x: 100, y: 202 },
      { id: 'Split', kind: 'and', x: 190, y: 195 },
      { id: 'A', kind: 'task', name: 'Draft option A', x: 300, y: 80 },
      { id: 'B', kind: 'task', name: 'Draft option B', x: 300, y: 280 },
      { id: 'Join', kind: 'and', x: 470, y: 195 },
      { id: 'Vote', kind: 'user', name: 'Pick the better draft', x: 580, y: 180 },
      { id: 'End', kind: 'end', x: 750, y: 202 },
    ], [
      { id: 'Flow_1', from: 'Start', to: 'Split' },
      { id: 'Flow_a', from: 'Split', to: 'A' },
      { id: 'Flow_b', from: 'Split', to: 'B' },
      { id: 'Flow_aj', from: 'A', to: 'Join' },
      { id: 'Flow_bj', from: 'B', to: 'Join' },
      { id: 'Flow_v', from: 'Join', to: 'Vote' },
      { id: 'Flow_end', from: 'Vote', to: 'End' },
    ]),
  },
  {
    id: 'orchestrator', name: 'Orchestrator–workers', blurb: 'Split the job, then combine',
    xml: buildDiagram('Orchestrator workers', [
      { id: 'Start', kind: 'start', x: 100, y: 202 },
      { id: 'Plan', kind: 'task', name: 'Split the job into parts', x: 200, y: 180 },
      { id: 'Split', kind: 'and', x: 370, y: 195 },
      { id: 'W1', kind: 'task', name: 'Do part one', x: 480, y: 80 },
      { id: 'W2', kind: 'task', name: 'Do part two', x: 480, y: 280 },
      { id: 'Join', kind: 'and', x: 650, y: 195 },
      { id: 'Combine', kind: 'task', name: 'Combine the parts', x: 760, y: 180 },
      { id: 'End', kind: 'end', x: 930, y: 202 },
    ], [
      { id: 'Flow_1', from: 'Start', to: 'Plan' },
      { id: 'Flow_2', from: 'Plan', to: 'Split' },
      { id: 'Flow_a', from: 'Split', to: 'W1' },
      { id: 'Flow_b', from: 'Split', to: 'W2' },
      { id: 'Flow_aj', from: 'W1', to: 'Join' },
      { id: 'Flow_bj', from: 'W2', to: 'Join' },
      { id: 'Flow_c', from: 'Join', to: 'Combine' },
      { id: 'Flow_end', from: 'Combine', to: 'End' },
    ]),
  },
  {
    id: 'evaluator', name: 'Evaluator–optimizer', blurb: 'Draft, then a person says if it is good enough',
    xml: buildDiagram('Evaluator optimizer', [
      { id: 'Start', kind: 'start', x: 120, y: 202 },
      { id: 'Draft', kind: 'task', name: 'Write a draft', x: 220, y: 180 },
      { id: 'Judge', kind: 'user', name: 'Is this good enough?', x: 390, y: 180 },
      { id: 'End', kind: 'end', x: 560, y: 202 },
    ], [
      { id: 'Flow_1', from: 'Start', to: 'Draft' },
      { id: 'Flow_2', from: 'Draft', to: 'Judge' },
      { id: 'Flow_3', from: 'Judge', to: 'End' },
    ]),
  },
  {
    id: 'human', name: 'Human in the loop', blurb: 'The agent proposes, a person approves',
    xml: buildDiagram('Human in the loop', [
      { id: 'Start', kind: 'start', x: 140, y: 202 },
      { id: 'Propose', kind: 'task', name: 'Propose the change', x: 240, y: 180 },
      { id: 'Approve', kind: 'user', name: 'Approve or send back', x: 410, y: 180 },
      { id: 'End', kind: 'end', x: 580, y: 202 },
    ], [
      { id: 'Flow_1', from: 'Start', to: 'Propose' },
      { id: 'Flow_2', from: 'Propose', to: 'Approve' },
      { id: 'Flow_3', from: 'Approve', to: 'End' },
    ]),
  },
];
