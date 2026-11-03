// R-34, R-35: secrets go to the real OS keychain (Secret Service / Keychain / Credential Manager) and never to disk;
// remote opencode servers must be HTTPS or loopback; nothing listens beyond 127.0.0.1.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-sec-'));
process.env.HARNESS_HOME = home;
delete process.env.HARNESS_KEYCHAIN; // the OS keychain, not the in-memory one
process.env.HARNESS_OD_DIR = join(home, 'none');

let base = '', token = '', sup: any, keychain: any, osAvailable = false, close = async () => {};
const call = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); return { status: r.status, data: t ? JSON.parse(t) : {} };
};
const allFiles = (dir: string): string[] => readdirSync(dir).flatMap((n) => { const p = join(dir, n); return statSync(p).isDirectory() ? allFiles(p) : [p]; });

beforeAll(async () => {
  ({ keychain } = await import('../src/keychain.ts'));
  try { keychain.set('probe', 'X', 'y'); osAvailable = keychain.get('probe', 'X') === 'y'; keychain.purge('probe'); } catch { osAvailable = false; }
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  sup = new Supervisor();
  const server = await startServer(sup, buildRouter(), {});
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); };
});
afterAll(async () => { await close(); });

describe('secrets and network (R-34, R-35)', () => {
  it('a provider key lives in the OS keychain, per workspace, and nowhere on disk', async (ctx) => {
    if (!osAvailable) ctx.skip();
    expect(keychain.backendName).toBe('os');
    const W = (await call('POST', '/api/workspaces', { name: 'Sec', kind: 'personal' })).data.id;
    const other = (await call('POST', '/api/workspaces', { name: 'Other', kind: 'personal' })).data.id;
    const secret = 'sk-harness-os-keychain-' + Date.now();
    expect((await call('POST', `/api/ws/${W}/providers`, { id: 'gw', url: 'http://127.0.0.1:9/v1', key: secret, model: 'm' })).status).toBe(200);
    // Read back through the OS keychain API directly, not through harness.
    const { Entry } = (await import('node:module')).createRequire(import.meta.url)('@napi-rs/keyring');
    expect(new Entry('harness', `${W}/GW_API_KEY`).getPassword()).toBe(secret);
    expect(keychain.get(other, 'GW_API_KEY')).toBeNull();
    for (const f of allFiles(home)) expect(readFileSync(f, 'utf8').includes(secret), f).toBe(false);
    keychain.purge(W); keychain.purge(other);
    expect(new Entry('harness', `${W}/GW_API_KEY`).getPassword() ?? null).toBeNull();
  });

  it('remote opencode must be HTTPS or loopback', async () => {
    const { remoteUrlAllowed } = await import('../src/opencode.ts');
    expect(remoteUrlAllowed('http://10.0.0.5:4096').ok).toBe(false);
    expect(remoteUrlAllowed('http://example.com').ok).toBe(false);
    expect(remoteUrlAllowed('ftp://example.com').ok).toBe(false);
    expect(remoteUrlAllowed('https://oc.example.com').ok).toBe(true);
    expect(remoteUrlAllowed('http://127.0.0.1:4096').ok).toBe(true);
    expect(remoteUrlAllowed('http://localhost:4096').ok).toBe(true);
    const W = (await call('POST', '/api/workspaces', { name: 'Remote', kind: 'personal' })).data.id;
    const r = await call('POST', `/api/ws/${W}/engine/mode`, { mode: 'remote', url: 'http://10.0.0.5:4096', username: 'opencode', password: 'x' });
    expect(r.status).toBeGreaterThanOrEqual(400);
    expect(JSON.stringify(r.data)).toMatch(/HTTPS|SSH tunnel/);
    expect(sup.rt(W).ws.engine.mode).toBe('managed');
  });

  it('an org workspace uploads its audit log to the org, without content unless policy allows (R-36)', async () => {
    const { createServer } = await import('node:http');
    const got: any[] = [];
    const collector = createServer((req, res) => { let b = ''; req.on('data', (c) => { b += c; }); req.on('end', () => { got.push(JSON.parse(b)); res.end('{}'); }); });
    await new Promise<void>((r) => collector.listen(0, '127.0.0.1', () => r()));
    const url = `http://127.0.0.1:${(collector.address() as any).port}/audit`;
    try {
      const ORG = (await call('POST', '/api/workspaces/join-org', { name: 'Audit Org', email: 'me@org.test', auditUrl: url })).data.id;
      const rt = sup.rt(ORG);
      rt.audit('chat.prompt', 'Secret merger plan · opencode');
      rt.audit('provider.call', 'org-gateway/llama · in 120 · out 40', 'harness');
      expect((await call('POST', `/api/ws/${ORG}/audit/upload`)).data.sent).toBeGreaterThan(2);
      const events = got[0].events;
      expect(got[0]).toMatchObject({ org: 'Audit Org', workspace: ORG, subject: 'me@org.test' });
      expect(events.map((e: any) => e.action)).toEqual(expect.arrayContaining(['sso.signin', 'policy.accept', 'chat.prompt', 'provider.call']));
      expect(events.find((e: any) => e.action === 'chat.prompt').detail).toBe('');
      expect(events.find((e: any) => e.action === 'provider.call').detail).toMatch(/in 120 · out 40/);
      expect(JSON.stringify(got)).not.toContain('Secret merger plan');
      // Only new entries go next time; a personal workspace never uploads.
      expect((await call('POST', `/api/ws/${ORG}/audit/upload`)).data.sent).toBe(0);
      rt.audit('export', 'workspace.tar.gz');
      expect((await call('POST', `/api/ws/${ORG}/audit/upload`)).data.sent).toBe(1);
      const P = (await call('POST', '/api/workspaces', { name: 'Mine', kind: 'personal' })).data.id;
      expect((await call('POST', `/api/ws/${P}/audit/upload`)).data.sent).toBe(0);
      expect(got).toHaveLength(2);
    } finally { collector.close(); }
  });

  it('the supervisor API only listens on loopback and needs its token', async () => {
    expect(new URL(base).hostname).toBe('127.0.0.1');
    expect((await fetch(base + '/api/app')).status).toBe(401);
  });
});
