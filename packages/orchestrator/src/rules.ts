// Harness model-check rules as a bpmnlint plugin (R-25). Same shape as bpmnlint's own rules:
// a factory returning { check(node, reporter) }. They run in the editor, before a run and in CI.
type Node = { $type: string; id: string; name?: string; flowElements?: Node[]; laneSets?: { lanes?: { name?: string; flowNodeRef?: Node[] }[] }[]; incoming?: Flow[]; outgoing?: Flow[]; get?: (k: string) => unknown; $attrs?: Record<string, string>; loopCharacteristics?: { $type: string; loopMaximum?: number } };
type Flow = { $type: string; id: string; sourceRef: Node; targetRef: Node; conditionExpression?: { body?: string } };
type Reporter = { report: (id: string, message: string) => void };

const attr = (n: Node, k: string): string | undefined => (n.get?.('harness:' + k) as string | undefined) ?? n.$attrs?.['harness:' + k];
const label = (n: Node) => `“${n.name || n.id}”`;
const isGateway = (n: Node) => /Gateway$/.test(n.$type);
const isTask = (n: Node) => /Task$/.test(n.$type) || n.$type === 'bpmn:Task';

function hasLoopLimit(n: Node): boolean {
  if (attr(n, 'maxLoops')) return true;
  if (n.loopCharacteristics?.loopMaximum) return true;
  // A counter condition on a gateway's outgoing flow also bounds the loop.
  if (isGateway(n)) return (n.outgoing || []).some((f) => /(count|loop|attempt|tries|iteration)\w*\s*(<|<=|>|>=)/i.test(f.conditionExpression?.body || ''));
  return false;
}

/** Every cycle must pass through a loop limit (a loop maximum or a counter condition on the gateway). */
export function loopLimit() {
  return {
    check(node: Node, reporter: Reporter) {
      if (node.$type !== 'bpmn:Process' && node.$type !== 'bpmn:SubProcess') return;
      const nodes = (node.flowElements || []).filter((e) => e.$type !== 'bpmn:SequenceFlow');
      const color = new Map<string, number>();
      const stack: Node[] = [];
      const visit = (n: Node) => {
        color.set(n.id, 1); stack.push(n);
        for (const f of n.outgoing || []) {
          const t = f.targetRef;
          if (!t) continue;
          if (color.get(t.id) === 1) {
            const cycle = stack.slice(stack.findIndex((x) => x.id === t.id));
            if (!cycle.some(hasLoopLimit)) reporter.report(f.id, `The loop back to ${label(t)} has no limit — set a loop maximum on a step in it, or a counter condition on its gateway`);
          } else if (!color.get(t.id)) visit(t);
        }
        stack.pop(); color.set(n.id, 2);
      };
      for (const n of nodes) if (!color.get(n.id)) visit(n);
    },
  };
}

function nearestSplit(flow: Flow, guard = 0): Node | null {
  let n: Node | undefined = flow.sourceRef;
  while (n && guard++ < 200) {
    if (isGateway(n) && (n.outgoing || []).length > 1) return n;
    if ((n.incoming || []).length !== 1) return null;
    n = n.incoming![0].sourceRef;
  }
  return null;
}

/** A parallel join fed by branches of one exclusive split waits forever (deadlock). */
export function parallelJoinAfterExclusive() {
  return {
    check(node: Node, reporter: Reporter) {
      if (node.$type !== 'bpmn:ParallelGateway' || (node.incoming || []).length < 2) return;
      const splits = (node.incoming || []).map((f) => nearestSplit(f)).filter((s): s is Node => !!s && s.$type === 'bpmn:ExclusiveGateway');
      const seen = new Set<string>();
      for (const s of splits) { if (seen.has(s.id)) { reporter.report(node.id, `Parallel join ${label(node)} waits for branches of exclusive split ${label(s)}, which only ever takes one — it would deadlock. Use an exclusive merge`); return; } seen.add(s.id); }
    },
  };
}

/** A task with several incoming flows and no gateway is an implicit merge. */
export function implicitMerge() {
  return { check(node: Node, reporter: Reporter) { if (isTask(node) && (node.incoming || []).length > 1) reporter.report(node.id, `${label(node)} has ${node.incoming!.length} incoming flows and no gateway (implicit merge)`); } };
}

/** User tasks belong in the You lane when the process has lanes. */
export function userTaskLane() {
  return {
    check(node: Node, reporter: Reporter) {
      if (node.$type !== 'bpmn:Process') return;
      const lanes = (node.laneSets || []).flatMap((s) => s.lanes || []);
      if (!lanes.length) return;
      for (const lane of lanes) for (const ref of lane.flowNodeRef || []) if (ref.$type === 'bpmn:UserTask' && !/\byou\b/i.test(lane.name || '')) reporter.report(ref.id, `User task ${label(ref)} sits in lane “${lane.name}” — people act in the You lane`);
    },
  };
}

/** An agent task with no model runs on the project's default model; say so. */
export function agentModel() {
  return { check(node: Node, reporter: Reporter) { if (node.$type === 'bpmn:ServiceTask' && (attr(node, 'type') || 'agent') === 'agent' && !attr(node, 'model')) reporter.report(node.id, `Agent task ${label(node)} has no model; it uses the project’s default`); } };
}

export const HARNESS_RULES: Record<string, () => { check: (n: Node, r: Reporter) => void }> = {
  'loop-limit': loopLimit,
  'no-parallel-join-after-exclusive': parallelJoinAfterExclusive,
  'no-implicit-merge': implicitMerge,
  'user-task-in-you-lane': userTaskLane,
  'agent-task-model': agentModel,
};
export const HARNESS_RULE_LEVELS: Record<string, 'error' | 'warn'> = {
  'harness/loop-limit': 'error',
  'harness/no-parallel-join-after-exclusive': 'error',
  'harness/no-implicit-merge': 'warn',
  'harness/user-task-in-you-lane': 'warn',
  'harness/agent-task-model': 'warn',
};
