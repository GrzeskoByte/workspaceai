// R-15: Open Design pipeline — helpers, the design-system library, and an end-to-end run with a real model.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir, homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { jsonFrom, compositeScore, claudeDesignSystems } from '../src/pipeline.ts';

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

describe('importing design systems from Claude', () => {
  const fake = (body: string) => { const f = join(mkdtempSync(join(tmpdir(), 'fake-claude-')), 'claude'); writeFileSync(f, `#!/bin/sh\n${body}\n`, { mode: 0o755 }); return f; };
  it('converts every system the CLI copies, and reports one it cannot read', async () => {
    const cli = fake(`mkdir -p 1/project/components/Button 2/project 3/project
echo '{"title":"Acme"}' > 1/project/design-system.json
echo '{"color":{"tokens":[{"name":"ink","value":"#111111"}]}}' > 1/project/tokens.json
echo '# Button' > 1/project/components/Button/README.md
echo x > 1/project/components/Button/preview.html
echo '# Second' > 2/project/README.md
echo '{oops' > 3/project/tokens.json
echo copied 3`);
    const { systems, failed } = await claudeDesignSystems(cli, {});
    expect(systems.map((s) => s.title).sort()).toEqual(['Acme', 'Second']);
    const acme = systems.find((s) => s.title === 'Acme')!;
    expect(acme.markdown).toContain('| ink | #111111 |');
    expect(acme.notCarried).toEqual(['1 component preview']);
    expect(failed).toEqual(['Claude design system 3']);
  });
  it('the route finds the CLI on PATH and returns the systems', async () => {
    const f = fake(`mkdir -p 1/project\necho '# Routed' > 1/project/README.md`);
    const path = process.env.PATH;
    process.env.PATH = dirname(f) + ':' + path;
    try {
      const w = (await ok('POST', '/api/workspaces', { name: 'Route DS', kind: 'personal' })).id;
      const { systems } = await ok('POST', `/api/ws/${w}/design-systems/claude`, {});
      expect(systems.map((s: any) => s.title)).toEqual(['Routed']);
    } finally { process.env.PATH = path; }
  });
  it('passes on why it found nothing, and why the CLI failed', async () => {
    await expect(claudeDesignSystems(fake('echo no artifact tool'), {})).rejects.toThrow('It said: no artifact tool');
    await expect(claudeDesignSystems(fake('echo not logged in >&2; exit 1'), {})).rejects.toThrow('not logged in');
  });
});

describe('workspace design systems', () => {
  it('a DesignSystem artifact is listed, readable, selectable and followed by the guide', async () => {
    const w = (await ok('POST', '/api/workspaces', { name: 'Own DS', kind: 'personal' })).id;
    const project = (await ok('POST', `/api/ws/${w}/projects`, { name: 'Acme', parent: null })).id;
    await ok('PATCH', `/api/ws/${w}`, { openDesign: { engine: 'builtin' } });
    const a = await ok('POST', `/api/ws/${w}/artifacts`, { project, type: 'DesignSystem', title: 'Acme', format: 'markdown', content: '# Acme\n\n> Sharp and blue.\n\n| primary | #0055ff |\n| ink | #0055FF |\n| paper | #ffffff |' });
    const { systems } = await ok('GET', `/api/ws/${w}/design-systems`);
    expect(systems[0].id).toBe('claude-warm-editorial');
    expect(systems.find((d: any) => d.id === a.id)).toMatchObject({ title: 'Acme', summary: 'Sharp and blue.', source: 'workspace' });
    expect(systems.find((d: any) => d.id === a.id).swatches).toContain('#ffffff');
    expect((await ok('GET', `/api/ws/${w}/design-systems/${a.id}`)).body).toContain('#0055ff');
    await ok('PATCH', `/api/ws/${w}`, { designSystem: 'Acme' });
    const chat = await ok('POST', `/api/ws/${w}/chats`, { project, title: 'Deck' });
    const { guide } = await ok('POST', `/api/ws/${w}/agent/guide`, { chat: chat.id, type: 'deck' });
    expect(guide).toContain('## Design system: Acme');
    expect(guide).toContain('#0055ff');
  });

  it('an agent writes a new system as a DesignSystem artifact, without being told to follow the current one', async () => {
    const w = (await ok('POST', '/api/workspaces', { name: 'New DS', kind: 'personal' })).id;
    await ok('PATCH', `/api/ws/${w}`, { openDesign: { engine: 'builtin' } });
    const chat = await ok('POST', `/api/ws/${w}/chats`, { title: 'Make one' });
    const { guide } = await ok('POST', `/api/ws/${w}/agent/guide`, { chat: chat.id, type: 'design-system' });
    expect(guide).not.toContain('## Design system:');
    expect(guide).not.toContain('is authoritative');
    const a = await ok('POST', `/api/ws/${w}/agent/artifacts`, { chat: chat.id, type: 'design-system', title: 'Mine', format: 'markdown', content: '# Mine' });
    expect(a.type).toBe('DesignSystem');
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
