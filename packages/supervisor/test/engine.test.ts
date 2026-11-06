// R-03: each workspace runs an unmodified opencode — the one on PATH or an exact pinned release — and an org can
// hold its workspaces on one version.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, mkdirSync, symlinkSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';

const home = mkdtempSync(join(tmpdir(), 'harness-engine-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'none');

let base = '', token = '', sup: any, close = async () => {};
const call = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); return { status: r.status, data: t ? JSON.parse(t) : {} };
};
const ok = async (m: string, p: string, b?: unknown) => { const r = await call(m, p, b); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${JSON.stringify(r.data)}`); return r.data; };

beforeAll(async () => {
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  sup = new Supervisor();
  const server = await startServer(sup, buildRouter(), {});
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); };
});
afterAll(async () => { await close(); });

describe('opencode version pin (R-03)', () => {
  it('refuses a pin that is not installed, then runs the pinned binary', async () => {
    const { pinnedOpencodeBin, opencodeBinary } = await import('../src/opencode.ts');
    const W = (await ok('POST', '/api/workspaces', { name: 'Pin', kind: 'personal' })).id;
    expect(opencodeBinary(sup.rt(W).ws.engine)).toBe('opencode');
    const version = execFileSync('opencode', ['--version']).toString().trim().split(/\s+/).pop()!.replace(/^v/, '');
    expect((await call('POST', `/api/ws/${W}/engine/version`, { version })).status).toBe(409);

    // Stand in for `npm install opencode-ai@<version>`: the same unmodified binary at the pinned path.
    const bin = pinnedOpencodeBin(version);
    mkdirSync(dirname(bin), { recursive: true });
    symlinkSync(execFileSync('sh', ['-c', 'command -v opencode']).toString().trim(), bin);
    const r = await ok('POST', `/api/ws/${W}/engine/version`, { version });
    expect(r.version).toBe(version);
    expect(opencodeBinary(sup.rt(W).ws.engine)).toBe(bin);
    const st = await ok('POST', `/api/ws/${W}/engine/start`);
    expect(st.state).toBe('running');
    expect(st.version).toBe(version);

    await ok('POST', `/api/ws/${W}/engine/version`, { version: 'system' });
    expect(opencodeBinary(sup.rt(W).ws.engine)).toBe('opencode');
  });

  it('no harness code calls a model API itself; only the connection test asks for a short reply', () => {
    const src = execFileSync('sh', ['-c', `grep -rnE "chat/completions|/v1/messages|/v1/responses|generateText|streamText|from 'openai'|from '@anthropic-ai" ${join(__dirname, '../../')}*/src ${join(__dirname, '../../../apps/ui/src')} || true`]).toString().trim().split('\n').filter((l) => !(l.includes('supervisor/src/routes.ts') && l.includes("await fetch(base + '/chat/completions'"))).join('\n');
    expect(src).toBe('');
  });
});
