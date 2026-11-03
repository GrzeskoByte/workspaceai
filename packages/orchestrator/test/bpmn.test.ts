import { describe, expect, it } from 'vitest';
import { PATTERNS, blankGraph } from '../src/model.ts';
import { lintBpmn } from '../src/lint.ts';
import { launchGraph } from '../src/runner.ts';

describe('bpmn patterns', () => {
  it('accepts every built-in pattern and the blank graph', async () => {
    for (const pattern of [blankGraph('Blank'), ...PATTERNS.map((p) => p.xml)]) {
      const result = await lintBpmn(pattern);
      expect(result.issues.filter((i) => i.category === 'error'), result.issues.map((i) => i.rule + ' ' + i.message).join('\n')).toEqual([]);
    }
  });

  it('rejects a process with no end event', async () => {
    const xml = blankGraph('Broken').replace(/<bpmn:endEvent[\s\S]*?\/>/, '').replace(/<bpmn:sequenceFlow id="Flow_2"[\s\S]*?<\/bpmn:sequenceFlow>/, '');
    const result = await lintBpmn(xml);
    expect(result.ok).toBe(false);
  });

  it('runs a prompt chain to the end and saves state after a step', async () => {
    const chain = PATTERNS.find((p) => p.id === 'chain');
    expect(chain).toBeTruthy();
    let sawState = false;
    const live = launchGraph({
      source: chain!.xml,
      dryRun: true,
      onUpdate: (u) => { if (u.state) sawState = true; },
    });
    await Promise.race([
      live.done,
      new Promise((_, reject) => setTimeout(() => reject(new Error('graph did not finish')), 8000)),
    ]);
    expect(sawState).toBe(true);
  });
});

describe('interchange (R-37)', () => {
  it('every pattern opens in a stock BPMN 2.0 reader with no harness extension, keeping harness attributes', async () => {
    const { BpmnModdle } = (await import('node:module')).createRequire(import.meta.url)('bpmn-moddle');
    const { HARNESS_MODDLE } = await import('../src/model.ts');
    for (const [name, xml] of Object.entries(PATTERNS).map(([k, p]: [string, any]) => [k, typeof p === 'string' ? p : p.xml] as [string, string])) {
      // What bpmn.io / Camunda Modeler do on open: parse against the plain BPMN 2.0 metamodel.
      const plain = new BpmnModdle();
      const { rootElement, warnings } = await plain.fromXML(xml);
      expect(warnings, name).toEqual([]);
      expect(rootElement.rootElements.some((e: any) => e.$type === 'bpmn:Process'), name).toBe(true);
      expect(rootElement.diagrams?.length, name + ' has diagram interchange').toBeGreaterThan(0);
      // Unknown harness:* attributes survive a save from the stock tool, then read back typed with the extension.
      const { xml: saved } = await plain.toXML(rootElement);
      const typed = await new BpmnModdle({ harness: HARNESS_MODDLE }).fromXML(saved);
      expect(typed.warnings, name).toEqual([]);
      if (/harness:type=/.test(xml)) expect(saved, name).toMatch(/harness:type=/);
    }
  });
});
