// R-15: Open Design pipeline — helpers, the design-system library, and an end-to-end run with a real model.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, existsSync, readFileSync } from 'node:fs';
import { tmpdir, homedir } from 'node:os';
import { join } from 'node:path';
import { jsonFrom, compositeScore } from '../src/pipeline.ts';

const home = mkdtempSync(join(tmpdir(), 'harness-pipe-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
const OD = join(homedir(), '.harness/vendor/open-design');
process.env.HARNESS_OD_DIR = existsSync(join(OD, 'apps/daemon/dist/cli.js')) ? OD : join(home, 'none');

let base = '', token = '', sup: any, close = async () => {};
const ok = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${t}`); return t ? JSON.parse(t) : {};
};
const quota = (e: unknown) => /usage limit|rate limit|quota/i.test(String((e as Error)?.message || e));

beforeAll(async () => {
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  const { installExtensions } = await import('../src/extensions.ts');
  sup = new Supervisor();
  const router = buildRouter();
  const server = await startServer(sup, router, {});
  installExtensions(sup, router, server);
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); };
});
afterAll(async () => { await close(); });

describe('pipeline helpers', () => {
  it('extracts JSON from fenced or chatty replies', () => {
    expect(jsonFrom<{ a: number }>('Sure!\n```json\n{"a": 1}\n```')).toEqual({ a: 1 });
    expect(jsonFrom<{ q: string[] }>('Here: {"q":["x"]} hope that helps')).toEqual({ q: ['x'] });
    expect(() => jsonFrom('no json here')).toThrow();
  });
  it('weights the five critique roles like Open Design', () => {
    expect(compositeScore({ designer: 2, critic: 10, brand: 10, a11y: 10, copy: 10 })).toBe(10);
    expect(compositeScore({ designer: 10, critic: 5, brand: 10, a11y: 10, copy: 10 })).toBe(8);
  });
});

describe('pipeline end to end (R-15)', () => {
  let W = '', P = '';
  it('lists the harness default plus Open Design’s design systems', async () => {
    W = (await ok('POST', '/api/workspaces', { name: 'Pipe', kind: 'personal' })).id;
    P = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Castform', parent: null })).id;
    // Free models have rolling per-model limits; HARNESS_TEST_MODEL picks another.
    await ok('PATCH', `/api/ws/${W}`, { defaultModel: process.env.HARNESS_TEST_MODEL || 'opencode/deepseek-v4.1-flash-free' });
    const { systems } = await ok('GET', `/api/ws/${W}/design-systems`);
    expect(systems[0].id).toBe('claude-warm-editorial');
    const md = (await ok('GET', `/api/ws/${W}/design-systems/claude-warm-editorial`)).body;
    expect(md).toContain('#cc785c');
    if (process.env.HARNESS_OD_DIR === OD) expect(systems.length).toBeGreaterThan(100);
  });

  it('brief → questions → direction → generate → critique → fix → handoff → memory', async () => {
    try {
      const brief = { project: P, type: 'Doc', title: 'Mold wall guide', brief: 'A one-page guide to choosing silicone mold wall thickness for 3D-printed masters.' };
      const { chat } = await ok('POST', `/api/ws/${W}/pipeline`, brief);
      const { questions } = await ok('POST', `/api/ws/${W}/pipeline/${chat}/questions`, brief);
      expect(questions.length).toBeGreaterThan(0);
      const { directions } = await ok('POST', `/api/ws/${W}/pipeline/${chat}/directions`, { type: 'Doc', answers: questions.map((q: string) => ({ q, a: 'Hobbyists; metric units; keep it short.' })) });
      expect(directions.length).toBeGreaterThan(0);
      const art = await ok('POST', `/api/ws/${W}/pipeline/${chat}/generate`, { type: 'Doc', title: 'Mold wall guide', direction: directions[0], designSystem: 'claude-warm-editorial', designSystemTitle: 'Claude Warm Editorial' });
      expect(art.project).toBe(P);
      expect(art.chat).toBe(chat);
      expect(art.designSystem).toBe('Claude Warm Editorial');
      if (process.env.HARNESS_OD_DIR === OD) expect(art.versions[0].od).toBeTruthy();
      const crit = await ok('POST', `/api/ws/${W}/pipeline/${chat}/critique`, { artifact: art.id });
      expect(Object.keys(crit.scores)).toEqual(expect.arrayContaining(['designer', 'critic', 'brand', 'a11y', 'copy']));
      expect(typeof crit.composite).toBe('number');
      const issues = crit.issues?.length ? crit.issues.slice(0, 2) : [{ role: 'copy', issue: 'Title could be clearer', fix: 'Add a one-line subtitle' }];
      const fixed = await ok('POST', `/api/ws/${W}/pipeline/${chat}/fix`, { artifact: art.id, issues });
      expect(fixed.version).toBe(art.version + 1);
      const ho = await ok('POST', `/api/ws/${W}/pipeline/${chat}/handoff`, { artifact: art.id });
      expect(existsSync(join(ho.folder, 'README.md'))).toBe(true);
      expect(readFileSync(join(ho.folder, 'README.md'), 'utf8').length).toBeGreaterThan(20);
      const mem = await ok('POST', `/api/ws/${W}/pipeline/${chat}/remember`, { artifact: art.id, direction: directions[0], designSystemTitle: 'Claude Warm Editorial' });
      expect(mem.facts.every((f: any) => f.status === 'suggested' && f.scope === P)).toBe(true);
    } catch (e) {
      if (quota(e)) { console.warn('Skipped: the free model hit its usage limit —', String(e).slice(0, 160)); return; }
      throw e;
    }
  }, 900000);
});
