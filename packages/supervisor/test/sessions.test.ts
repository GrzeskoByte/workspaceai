// R-17 start or connect, R-18 import, R-19 two-way sync — against real opencode servers.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-sess-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'no-open-design');

let base = '', token = '', sup: any, close = async () => {};
let user: any; // a separately running opencode server ("the user's own")
let userUrl = '', userPw = '';
const dirs = { cf: join(home, 'code/castform'), sim: join(home, 'code/castform/sim'), dot: join(home, 'dotfiles') };

async function call(method: string, path: string, body?: unknown) {
  const r = await fetch(base + path, { method, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  const t = await r.text();
  return { status: r.status, data: t ? JSON.parse(t) : {} };
}
const ok = async (m: string, p: string, b?: unknown) => { const r = await call(m, p, b); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${JSON.stringify(r.data)}`); return r.data; };
const wait = async (fn: () => Promise<boolean>, ms = 15000) => { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return true; await new Promise((r) => setTimeout(r, 300)); } return false; };

beforeAll(async () => {
  for (const d of Object.values(dirs)) mkdirSync(d, { recursive: true });
  const { ManagedServer, baseEnv } = await import('../src/opencode.ts');
  const x = join(home, 'user-opencode');
  user = new ManagedServer({ binary: 'opencode', cwd: home, env: { ...baseEnv(), XDG_DATA_HOME: x + '/data', XDG_CONFIG_HOME: x + '/config', XDG_STATE_HOME: x + '/state', XDG_CACHE_HOME: x + '/cache' }, logDir: (mkdirSync(x, { recursive: true }), x) });
  const c = await user.start();
  userUrl = user.url; userPw = user.password;
  await c.createSession({ title: 'Fix STL import scaling', directory: dirs.cf });
  await c.createSession({ title: 'Wall thickness solver tests', directory: dirs.sim });
  await c.createSession({ title: 'Neovim config cleanup', directory: dirs.dot });
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  const { installExtensions } = await import('../src/extensions.ts');
  sup = new Supervisor();
  const router = buildRouter();
  const server = await startServer(sup, router, {});
  installExtensions(sup, router, server);
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); await user.stop(); };
});
afterAll(async () => { await close(); });

describe('starting or connecting opencode (R-17)', () => {
  it('a connection must pass its test first', async () => {
    const good = await ok('POST', '/api/onboarding/test-remote', { url: userUrl, username: 'opencode', password: userPw });
    expect(good.ok).toBe(true);
    expect(good.sessions).toBe(3);
    expect(good.folders).toBe(3);
    const bad = await ok('POST', '/api/onboarding/test-remote', { url: userUrl, username: 'opencode', password: 'wrong' });
    expect(bad.ok).toBe(false);
    expect(bad.error).toMatch(/password/);
    // Remote servers must be HTTPS or reached through a tunnel on loopback (R-35).
    const plain = await ok('POST', '/api/onboarding/test-remote', { url: 'http://dev-box.example.test:4096', username: 'opencode', password: 'x' });
    expect(plain.ok).toBe(false);
    expect(plain.error).toMatch(/HTTPS|tunnel/);
  });

  let R = '';
  it('a workspace connected to a running server imports its sessions; folders become nested projects (R-18)', async () => {
    R = (await ok('POST', '/api/workspaces', { name: 'Remote', kind: 'personal' })).id;
    expect((await call('POST', `/api/ws/${R}/engine/mode`, { mode: 'remote', url: userUrl, username: 'opencode', password: 'wrong' })).status).toBe(400);
    await ok('POST', `/api/ws/${R}/engine/mode`, { mode: 'remote', url: userUrl, username: 'opencode', password: userPw });
    expect(sup.rt(R).status.state).toBe('connected');
    const scan = (await ok('GET', `/api/ws/${R}/scan-sessions`)).sessions;
    expect(scan.length).toBe(3);
    // Skip one session.
    const keep = scan.filter((s: any) => s.title !== 'Neovim config cleanup').map((s: any) => s.id);
    await ok('POST', `/api/ws/${R}/import-sessions`, { ids: keep });
    const snap = await ok('GET', `/api/ws/${R}/snapshot`);
    const byName = (n: string) => snap.projects.find((p: any) => p.name === n);
    expect(byName('Castform')).toBeTruthy();
    expect(byName('Sim').parent).toBe(byName('Castform').id);
    expect(byName('Dotfiles')).toBeUndefined();
    const solver = snap.chats.find((c: any) => c.title === 'Wall thickness solver tests');
    expect(solver.project).toBe(byName('Sim').id);
    expect(solver.imported).toBe(true);
  });

  it('a session started in the TUI appears in the right project (R-19)', async () => {
    const c = await sup.rt(R).client();
    const s = await c.createSession({ title: 'Started in the TUI', directory: dirs.sim });
    const seen = await wait(async () => (await ok('GET', `/api/ws/${R}/snapshot`)).chats.some((x: any) => x.sid === s.id));
    expect(seen).toBe(true);
    const snap = await ok('GET', `/api/ws/${R}/snapshot`);
    expect(snap.chats.find((x: any) => x.sid === s.id).project).toBe(snap.projects.find((p: any) => p.name === 'Sim').id);
    // …and a chat started here is a session on that same server, so the TUI sees it.
    const chat = await ok('POST', `/api/ws/${R}/chats`, { project: snap.projects.find((p: any) => p.name === 'Castform').id, title: 'Started in harness' });
    const onServer = await (await (await import('../src/opencode.ts')).OpencodeClient.prototype.listSessions.call(new (await import('../src/opencode.ts')).OpencodeClient(userUrl, 'opencode', userPw)));
    expect(onServer.some((x: any) => x.id === chat.sid)).toBe(true);
    expect((await ok('GET', `/api/ws/${R}/chats/${chat.id}/attach`)).command).toBe(`opencode --server ${userUrl} --session ${chat.sid} ${dirs.cf}`);
  });

  it('a managed workspace copies sessions from another server with their history (R-18)', async () => {
    const M = (await ok('POST', '/api/workspaces', { name: 'Managed', kind: 'personal' })).id;
    const { OpencodeClient } = await import('../src/opencode.ts');
    const { copySessions } = await import('../src/detect.ts');
    const src = new OpencodeClient(userUrl, 'opencode', userPw);
    const all = await src.listSessions();
    const r = await copySessions(src, sup.rt(M), all.filter((s: any) => s.title === 'Fix STL import scaling').map((s: any) => s.id));
    expect(r.failed).toEqual([]);
    expect(r.chats).toBe(1);
    const snap = await ok('GET', `/api/ws/${M}/snapshot`);
    const chat = snap.chats.find((c: any) => c.title === 'Fix STL import scaling');
    expect(snap.projects.find((p: any) => p.id === chat.project).name).toBe('Castform');
    const mine = await (await sup.rt(M).client()).getSession(chat.sid);
    expect(mine.id).toBe(chat.sid);
    expect(sup.rt(M).status.url).not.toBe(userUrl);
  });
});
