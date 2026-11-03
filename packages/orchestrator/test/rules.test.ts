// R-25: the editor rejects a model that breaks a rule.
import { describe, it, expect } from 'vitest';
import { buildDiagram, PATTERNS } from '../src/model.ts';
import { lintBpmn } from '../src/lint.ts';

const rules = (r: { issues: { rule: string; category: string }[] }) => r.issues.map((i) => `${i.category}:${i.rule}`);

describe('harness model-check rules', () => {
  it('every built-in pattern passes, including the evaluator–optimizer loop', async () => {
    for (const p of PATTERNS) {
      const r = await lintBpmn(p.xml);
      expect(r.ok, p.id + ' ' + JSON.stringify(r.issues)).toBe(true);
    }
  });

  it('rejects a loop without a limit', async () => {
    const xml = buildDiagram('Loop', [
      { id: 'S', kind: 'start', x: 0, y: 0 }, { id: 'Draft', kind: 'task', name: 'Draft', x: 100, y: 0 },
      { id: 'Ok', kind: 'xor', name: 'Good enough?', x: 250, y: 0, gatewayDefault: 'Back' }, { id: 'E', kind: 'end', x: 400, y: 0 },
    ], [
      { id: 'F1', from: 'S', to: 'Draft' }, { id: 'F2', from: 'Draft', to: 'Ok' },
      { id: 'Done', from: 'Ok', to: 'E', cond: '${environment.output.ok}' }, { id: 'Back', from: 'Ok', to: 'Draft' },
    ]);
    const r = await lintBpmn(xml);
    expect(r.ok).toBe(false);
    expect(rules(r)).toContain('error:harness/loop-limit');
    // With a loop maximum on the draft step it passes.
    const bounded = await lintBpmn(xml.replace('harness:type="agent" harness:agent="opencode" harness:prompt="Draft"', 'harness:type="agent" harness:agent="opencode" harness:prompt="Draft" harness:maxLoops="3"'));
    expect(rules(bounded)).not.toContain('error:harness/loop-limit');
  });

  it('rejects a parallel join after an exclusive split', async () => {
    const xml = buildDiagram('Deadlock', [
      { id: 'S', kind: 'start', x: 0, y: 0 }, { id: 'X', kind: 'xor', name: 'Which?', x: 100, y: 0, gatewayDefault: 'Fb' },
      { id: 'A', kind: 'task', name: 'A', x: 200, y: -80 }, { id: 'B', kind: 'task', name: 'B', x: 200, y: 80 },
      { id: 'J', kind: 'and', name: 'Join', x: 350, y: 0 }, { id: 'E', kind: 'end', x: 450, y: 0 },
    ], [
      { id: 'F0', from: 'S', to: 'X' }, { id: 'Fa', from: 'X', to: 'A', cond: '${environment.variables.a}' }, { id: 'Fb', from: 'X', to: 'B' },
      { id: 'Faj', from: 'A', to: 'J' }, { id: 'Fbj', from: 'B', to: 'J' }, { id: 'Fe', from: 'J', to: 'E' },
    ]);
    const r = await lintBpmn(xml);
    expect(r.ok).toBe(false);
    expect(rules(r)).toContain('error:harness/no-parallel-join-after-exclusive');
    expect(r.issues.find((i) => i.rule === 'harness/no-parallel-join-after-exclusive')!.message).toMatch(/deadlock/);
  });

  it('warns about an implicit merge', async () => {
    const xml = buildDiagram('Merge', [
      { id: 'S', kind: 'start', x: 0, y: 0 }, { id: 'P', kind: 'and', x: 100, y: 0 },
      { id: 'A', kind: 'task', name: 'A', x: 200, y: -80 }, { id: 'B', kind: 'task', name: 'B', x: 200, y: 80 },
      { id: 'M', kind: 'task', name: 'Merge here', x: 350, y: 0 }, { id: 'E', kind: 'end', x: 450, y: 0 },
    ], [
      { id: 'F0', from: 'S', to: 'P' }, { id: 'Fa', from: 'P', to: 'A' }, { id: 'Fb', from: 'P', to: 'B' },
      { id: 'Fam', from: 'A', to: 'M' }, { id: 'Fbm', from: 'B', to: 'M' }, { id: 'Fe', from: 'M', to: 'E' },
    ]);
    expect(rules(await lintBpmn(xml))).toContain('warn:harness/no-implicit-merge');
  });
});
