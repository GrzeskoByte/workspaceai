// Model check (R-25). bpmnlint is the rule pack; a diagram that breaks a rule is rejected before a run.
import { createRequire } from 'node:module';
import { HARNESS_MODDLE } from './model.ts';
import { HARNESS_RULES, HARNESS_RULE_LEVELS } from './rules.ts';

const require = createRequire(import.meta.url);
// bpmn-moddle is CJS and exports { BpmnModdle }, not a default constructor.
const { BpmnModdle } = require('bpmn-moddle') as {
  BpmnModdle: new (packages?: Record<string, unknown>) => {
    fromXML(xml: string): Promise<{ rootElement: unknown }>;
  };
};
const { Linter } = require('bpmnlint') as {
  Linter: new (o: { config?: { rules: Record<string, string> }; resolver: { resolveRule: (pkg: string, rule: string) => unknown } }) => { lint: (root: unknown) => Promise<Record<string, { id?: string; message: string; category?: string }[]>> };
};

const RULES: Record<string, 'error' | 'warn'> = {
  ...HARNESS_RULE_LEVELS,
  'conditional-flows': 'error',
  'end-event-required': 'error',
  'start-event-required': 'error',
  'single-blank-start-event': 'error',
  'no-disconnected': 'error',
  'no-duplicate-sequence-flows': 'error',
  'no-implicit-end': 'error',
  'no-implicit-start': 'error',
  'no-bpmndi': 'error',
  'label-required': 'warn',
};

function makeLinter() {
  // A new linter per diagram. bpmnlint caches the rule instance, and some rules
  // keep state from the previous diagram when the instance is reused.
  return new Linter({
    config: { rules: RULES },
    resolver: {
      resolveRule(pkg: string, ruleName: string) {
        // WorkspaceAI rules ship as a bpmnlint plugin ("harness/<rule>").
        if (pkg === 'bpmnlint-plugin-harness' || pkg === 'harness') return HARNESS_RULES[ruleName];
        return require('bpmnlint/rules/' + ruleName);
      },
    },
  });
}

export interface LintIssue { rule: string; id: string; message: string; category: 'error' | 'warn' }

function linkSequenceFlows(root: unknown): void {
  const seen = new Set<object>();
  const visit = (node: unknown): void => {
    if (!node || typeof node !== 'object' || seen.has(node)) return;
    seen.add(node);
    const flow = node as {
      $type?: string;
      sourceRef?: { outgoing?: unknown[] };
      targetRef?: { incoming?: unknown[] };
    };
    if (flow.$type === 'bpmn:SequenceFlow' && flow.sourceRef && flow.targetRef) {
      const outgoing = flow.sourceRef.outgoing ?? (flow.sourceRef.outgoing = []);
      const incoming = flow.targetRef.incoming ?? (flow.targetRef.incoming = []);
      if (!outgoing.includes(flow)) outgoing.push(flow);
      if (!incoming.includes(flow)) incoming.push(flow);
    }
    for (const value of Object.values(node)) {
      if (Array.isArray(value)) {
        for (const item of value) visit(item);
      } else {
        visit(value);
      }
    }
  };
  visit(root);
}

export async function lintBpmn(xml: string): Promise<{ ok: boolean; issues: LintIssue[] }> {
  let root: unknown;
  try {
    const moddle = new BpmnModdle({ harness: HARNESS_MODDLE });
    const parsed = await moddle.fromXML(xml);
    root = parsed.rootElement;
  } catch (e) {
    return { ok: false, issues: [{ rule: 'parse', id: '', message: (e as Error).message || 'BPMN XML did not parse', category: 'error' }] };
  }
  // bpmnlint reads incoming and outgoing. The XML stores sourceRef and targetRef.
  // Wire the two so a diagram without the child elements still lints.
  linkSequenceFlows(root);
  const reports = await makeLinter().lint(root);
  const names = new Map<string, string>();
  for (const m of xml.matchAll(/\bid="([^"]+)"[^>]*?\bname="([^"]*)"/g)) names.set(m[1], m[2]);
  for (const m of xml.matchAll(/\bname="([^"]*)"[^>]*?\bid="([^"]+)"/g)) if (!names.has(m[2])) names.set(m[2], m[1]);
  const issues: LintIssue[] = [];
  for (const [rule, list] of Object.entries(reports)) {
    for (const item of list) {
      const category = (item as { category?: string }).category === 'warn' ? 'warn' : (RULES[rule] === 'warn' ? 'warn' : 'error');
      const name = item.id ? names.get(item.id) : '';
      issues.push({ rule, id: item.id || '', message: name && !item.message.includes('“') ? `${name}: ${item.message}` : item.message, category });
    }
  }
  return { ok: !issues.some((i) => i.category === 'error'), issues };
}
