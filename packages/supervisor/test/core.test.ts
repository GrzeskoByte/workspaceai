// Integration tests for the supervisor API: workspaces, isolation, org policy, projects, instructions, secrets, sessions.
// Runs against a real `opencode serve` (opencode must be on PATH) in a throwaway HARNESS_HOME.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-test-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'no-open-design');

let base = '';
let token = '';
let close: () => Promise<void> = async () => {};
let sup: any;

async function call(method: string, path: string, body?: unknown): Promise<{ status: number; data: any }> {
  const r = await fetch(base + path, { method, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  const t = await r.text();
  return { status: r.status, data: t ? JSON.parse(t) : {} };
}
const ok = async (method: string, path: string, body?: unknown) => { const r = await call(method, path, body); if (r.status >= 300) throw new Error(`${method} ${path} → ${r.status} ${JSON.stringify(r.data)}`); return r.data; };

beforeAll(async () => {
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  const { installExtensions } = await import('../src/extensions.ts');
  sup = new Supervisor();
  const router = buildRouter();
  const server = await startServer(sup, router, {});
  installExtensions(sup, router, server);
  base = `http://127.0.0.1:${server.port}`;
  token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); };
});
afterAll(async () => { await close(); });

describe('supervisor API', () => {
  let A = '', B = '', ORG = '';

  it('rejects requests without the per-launch token (R-35)', async () => {
    const r = await fetch(base + '/api/state');
    expect(r.status).toBe(401);
  });

  it('creates workspaces; organizations only join through SSO (R-08)', async () => {
    A = (await ok('POST', '/api/workspaces', { name: 'Personal', kind: 'personal' })).id;
    B = (await ok('POST', '/api/workspaces', { name: 'Castform Labs', kind: 'team' })).id;
    const r = await call('POST', '/api/workspaces', { name: 'Evil Org', kind: 'org' });
    expect(r.status).toBe(400);
    expect(r.data.error).toMatch(/SSO/);
    ORG = (await ok('POST', '/api/workspaces/join-org', { name: 'BSC AI Factory', email: 'me@org.test' })).id;
    const state = await ok('GET', '/api/state');
    expect(state.workspaces.map((w: any) => w.kind)).toEqual(['personal', 'org', 'team']);
    // Onboarding has no organization step (R-08).
    const onb = readFileSync(join(__dirname, '../../../apps/ui/src/onboarding/Onboarding.tsx'), 'utf8');
    const steps = /const STEPS = \[(.*?)\] as const;/s.exec(onb)?.[1] || '';
    expect(steps).not.toMatch(/org|sso|organization/i);
  });

  it('keeps everything per workspace (R-06)', async () => {
    const p = await ok('POST', `/api/ws/${A}/projects`, { name: 'Secret project', parent: null });
    await ok('POST', `/api/ws/${A}/facts`, { text: 'Only A knows this', scope: 'ws' });
    await ok('PUT', `/api/ws/${A}/secrets/MY_KEY`, { value: 'sk-test-aaaaaaaaaaaaaaaa' });
    await ok('POST', `/api/ws/${A}/mcp`, { command: 'uvx blender-mcp' });
    const a = await ok('GET', `/api/ws/${A}/snapshot`);
    const b = await ok('GET', `/api/ws/${B}/snapshot`);
    expect(a.projects.some((x: any) => x.id === p.id)).toBe(true);
    expect(b.projects.some((x: any) => x.name === 'Secret project')).toBe(false);
    expect(b.facts.some((f: any) => f.text === 'Only A knows this')).toBe(false);
    expect(b.workspace.mcp.some((m: any) => /blender/.test(m.name))).toBe(false);
    expect((await ok('GET', `/api/ws/${B}/secrets`)).names).not.toContain('MY_KEY');
    expect((await ok('GET', `/api/ws/${A}/secrets`)).names).toContain('MY_KEY');
    // Separate config and data folders.
    expect(sup.rt(A).paths.root).not.toBe(sup.rt(B).paths.root);
    expect(sup.rt(A).paths.xdgData.startsWith(sup.rt(A).paths.root)).toBe(true);
  });

  it('writes custom providers to opencode.json with key references only (R-20, R-34)', async () => {
    const r = await ok('POST', `/api/ws/${A}/providers`, { id: 'ai-factory', name: 'AI Factory inference', url: 'https://inference.example.test/v1', env: 'AI_FACTORY_TOKEN', model: 'llama-4-maverick', key: 'sk-live-supersecretvalue123' });
    const json = JSON.parse(r.opencodeJson);
    expect(json.provider['ai-factory']).toEqual({ npm: '@ai-sdk/openai-compatible', name: 'AI Factory inference', options: { baseURL: 'https://inference.example.test/v1', apiKey: '{env:AI_FACTORY_TOKEN}' }, models: { 'llama-4-maverick': { name: 'llama-4-maverick' } } });
    for (const id of [A, B, ORG]) {
      const f = sup.rt(id).paths.opencodeJson;
      if (existsSync(f)) expect(readFileSync(f, 'utf8')).not.toMatch(/supersecret|sk-test|sk-live/);
    }
    expect(sup.rt(A).engineEnv().AI_FACTORY_TOKEN).toBe('sk-live-supersecretvalue123');
    expect(sup.rt(B).engineEnv().AI_FACTORY_TOKEN).toBeUndefined();
  });

  it('org workspace offers only policy providers and locks changes (R-07, R-36)', async () => {
    const s = await ok('GET', `/api/ws/${ORG}/snapshot`);
    expect(s.policy.allowedProviders.map((p: any) => p.id)).toEqual(['ai-factory', 'azure']);
    const models = (await ok('GET', `/api/ws/${ORG}/models`)).models;
    expect(models.every((m: string) => s.policy.allowedModels.includes(m))).toBe(true);
    expect((await call('POST', `/api/ws/${ORG}/providers`, { id: 'anthropic', url: 'https://api.anthropic.com/v1' })).status).toBe(403);
    expect((await call('PATCH', `/api/ws/${ORG}`, { defaultModel: 'anthropic/claude-opus-5' })).status).toBe(403);
    expect((await call('PUT', `/api/ws/${ORG}/secrets/ANTHROPIC_API_KEY`, { value: 'sk-personal' })).status).toBe(403);
    expect((await call('PATCH', `/api/ws/${ORG}`, { web: { mode: 'exa' } })).status).toBe(403);
    const audit = (await ok('GET', `/api/ws/${ORG}/audit`)).events.map((e: any) => e.action);
    expect(audit).toContain('policy.block');
    expect(audit).toContain('policy.accept');
    expect(audit).toContain('sso.signin');
  });

  it('a policy that fails verification blocks sending (R-36)', async () => {
    const r = await ok('POST', `/api/ws/${ORG}/policy/refresh`, { tamper: true });
    expect(r.ok).toBe(false);
    const send = await call('POST', `/api/ws/${ORG}/chats`, { title: 'x' });
    expect(send.status).toBe(423);
    expect((await ok('POST', `/api/ws/${ORG}/policy/refresh`, {})).ok).toBe(true);
  });

  let root = '', sub = '', subsub = '', leaf = '';
  it('nests sub-projects to any depth (R-09, R-11)', async () => {
    root = (await ok('POST', `/api/ws/${A}/projects`, { name: 'Castform', parent: null })).id;
    sub = (await ok('POST', `/api/ws/${A}/projects`, { name: 'Simulation engine', parent: root })).id;
    subsub = (await ok('POST', `/api/ws/${A}/projects`, { name: 'Solver', parent: sub })).id;
    leaf = (await ok('POST', `/api/ws/${A}/projects`, { name: 'Kernels', parent: subsub })).id;
    const snap = await ok('GET', `/api/ws/${A}/snapshot`);
    const leafP = snap.projects.find((p: any) => p.id === leaf);
    expect(leafP.folder).toMatch(/castform\/simulation-engine\/solver\/kernels$/);
    // A project cannot move inside its own descendant.
    expect((await call('PATCH', `/api/ws/${A}/projects/${root}`, { parent: leaf })).status).toBe(400);
  });

  it('layers instructions broad to narrow, and inheritance can be turned off (R-12)', async () => {
    await ok('PUT', `/api/ws/${A}/projects/${root}/agents`, { text: '# Castform\n- millimetres' });
    await ok('PUT', `/api/ws/${A}/projects/${sub}/agents`, { text: '# Sim\n- seed randomness' });
    const layers = (await ok('GET', `/api/ws/${A}/projects/${leaf}/layers`)).layers;
    expect(layers.map((l: any) => l.kind)).toEqual(['workspace', 'parent', 'parent', 'parent', 'project']);
    expect(layers[1].text).toMatch(/millimetres/);
    await ok('PATCH', `/api/ws/${A}/projects/${leaf}`, { inherit: false });
    const off = (await ok('GET', `/api/ws/${A}/projects/${leaf}/layers`)).layers;
    expect(off.map((l: any) => l.kind)).toEqual(['workspace', 'project']);
    await ok('PATCH', `/api/ws/${A}/projects/${leaf}`, { inherit: true });
  });

  let chat: any;
  it('chats are opencode sessions; rename, fork and delete use the session API (R-16)', async () => {
    chat = await ok('POST', `/api/ws/${A}/chats`, { project: subsub, title: 'Solver questions' });
    expect(chat.sid).toMatch(/^ses_/);
    expect(chat.project).toBe(subsub);
    // Two-way sync must not adopt the harness's own new session as a second chat.
    await new Promise((r) => setTimeout(r, 1500));
    expect((await ok('GET', `/api/ws/${A}/snapshot`)).chats.filter((x: any) => x.sid === chat.sid).length).toBe(1);
    const c = await sup.rt(A).client();
    expect((await c.getSession(chat.sid)).location.directory).toMatch(/solver$/);
    // Inherited layers ride along as a session instruction entry, not as a message (R-12).
    const entries = (await c.req('GET', `/api/experimental/session/${chat.sid}/instructions/entries`)).data;
    const harness = entries.find((e: any) => e.key === 'harness');
    expect(harness.value).toMatch(/millimetres/);
    expect(harness.value).toMatch(/seed randomness/);
    expect((await c.messages(chat.sid)).length).toBe(0);
    await ok('PATCH', `/api/ws/${A}/chats/${chat.id}`, { title: 'Renamed solver chat' });
    expect((await c.getSession(chat.sid)).title).toBe('Renamed solver chat');
    const fork = await ok('POST', `/api/ws/${A}/chats/${chat.id}/fork`);
    expect(fork.sid).not.toBe(chat.sid);
    await new Promise((r) => setTimeout(r, 1500));
    expect((await ok('GET', `/api/ws/${A}/snapshot`)).chats.filter((x: any) => x.sid === fork.sid).length).toBe(1);
    expect((await c.getSession(fork.sid)).id).toBe(fork.sid);
    await ok('DELETE', `/api/ws/${A}/chats/${fork.id}`);
    await expect(c.getSession(fork.sid)).rejects.toThrow();
    const attach = (await ok('GET', `/api/ws/${A}/chats/${chat.id}/attach`)).command;
    expect(attach).toContain('--session ' + chat.sid);
  });

  it('a chat is never stranded (R-10)', async () => {
    const quick = await ok('POST', `/api/ws/${A}/chats`, { title: 'Quick question' });
    const snap = await ok('GET', `/api/ws/${A}/snapshot`);
    expect(snap.projects.find((p: any) => p.id === quick.project).isGeneral).toBe(true);
    expect((await call('POST', `/api/ws/${A}/chats/${chat.id}/move`, { project: 'nope' })).status).toBe(400);
    const general = snap.projects.find((p: any) => p.isGeneral).id;
    expect((await call('POST', `/api/ws/${A}/projects/${general}/archive`)).status).toBe(400);
    expect((await call('DELETE', `/api/ws/${A}/projects/${general}`)).status).toBe(400);
    // Archive hides the sub-tree and its chats; nothing is deleted.
    expect((await ok('POST', `/api/ws/${A}/projects/${sub}/archive`)).hiddenSubprojects).toBe(2);
    const after = await ok('GET', `/api/ws/${A}/snapshot`);
    expect(after.projects.some((p: any) => p.id === subsub)).toBe(false);
    expect(after.chats.some((c: any) => c.id === chat.id)).toBe(false);
    for (const c of after.chats) expect(after.projects.some((p: any) => p.id === c.project)).toBe(true);
    await ok('POST', `/api/ws/${A}/projects/${sub}/restore`);
    expect((await ok('GET', `/api/ws/${A}/snapshot`)).chats.some((c: any) => c.id === chat.id)).toBe(true);
  });

  it('deleting a project removes its whole sub-tree and everything in it', async () => {
    const top = (await ok('POST', `/api/ws/${A}/projects`, { name: 'Doomed', parent: null })).id;
    const kid = (await ok('POST', `/api/ws/${A}/projects`, { name: 'Doomed kid', parent: top })).id;
    const grandkid = (await ok('POST', `/api/ws/${A}/projects`, { name: 'Doomed grandkid', parent: kid })).id;
    const live = await ok('POST', `/api/ws/${A}/chats`, { title: 'Live chat', project: kid });
    const old = await ok('POST', `/api/ws/${A}/chats`, { title: 'Archived chat', project: grandkid });
    const g = await ok('POST', `/api/ws/${A}/graphs`, { project: kid, name: 'Doomed graph' });
    const rt = sup.rt(A);
    const art = rt.artifacts.create({ project: top, type: 'Doc', title: 'Doomed doc', content: '# x' });
    rt.addFact('Doomed fact', kid);
    const folder = rt.project(grandkid).folder;
    const artDir = rt.artifacts.dir(art);
    const facts = rt.factsFile(kid);
    const graphFile = g.file;
    const before = rt.ws.defaultProject;
    rt.ws.defaultProject = kid;
    expect([artDir, facts, graphFile].map((f) => existsSync(f))).toEqual([true, true, true]);
    // An archived sub-project goes too, though descendants() can't see it.
    await ok('POST', `/api/ws/${A}/projects/${grandkid}/archive`);
    await ok('DELETE', `/api/ws/${A}/projects/${top}`);
    expect(rt.state.projects.filter((p: any) => [top, kid, grandkid].includes(p.id))).toEqual([]);
    expect(rt.state.chats.filter((x: any) => [live.id, old.id].includes(x.id))).toEqual([]);
    expect(rt.state.graphs.some((x: any) => x.id === g.id)).toBe(false);
    expect(rt.state.artifacts.some((x: any) => x.id === art.id)).toBe(false);
    expect([artDir, facts, graphFile].map((f) => existsSync(f))).toEqual([false, false, false]);
    expect(rt.ws.defaultProject).toBe(rt.general().id);
    expect(existsSync(folder)).toBe(true);
    rt.ws.defaultProject = before;
  });

  it('exports a workspace without secrets', async () => {
    const r = await fetch(base + `/api/ws/${A}/export`, { headers: { 'x-harness-token': token } });
    expect(r.status).toBe(200);
    const buf = Buffer.from(await r.arrayBuffer());
    const { execFileSync } = await import('node:child_process');
    const { writeFileSync } = await import('node:fs');
    const f = join(home, 'export.tar.gz'); writeFileSync(f, buf);
    const list = execFileSync('tar', ['-tzf', f]).toString();
    expect(list).toMatch(/config\/opencode\.json/);
    expect(list).toMatch(new RegExp(`projects/${root}/AGENTS\\.md`));
    const all = execFileSync('sh', ['-c', `tar -xzOf ${f} 2>/dev/null | strings`]).toString();
    expect(all).not.toMatch(/sk-live-supersecretvalue123|sk-test-aaaaaaaa/);
  });

  it('switching workspaces keeps the other workspace running (R-05)', async () => {
    expect(sup.rt(A).status.state).toBe('running');
    const portA = sup.rt(A).status.port;
    await ok('POST', `/api/workspaces/${B}/activate`);
    await sup.rt(B).client();
    expect(sup.rt(A).status.state).toBe('running');
    expect(sup.rt(B).status.port).not.toBe(portA);
    // Each workspace's server only sees its own sessions.
    const sessB = await (await sup.rt(B).client()).listSessions();
    expect(sessB.some((s: any) => s.id === chat.sid)).toBe(false);
  });

  it('managed servers bind to loopback and require a password (R-35)', async () => {
    const url = sup.rt(A).status.url;
    expect(url).toMatch(/^http:\/\/127\.0\.0\.1:/);
    expect((await fetch(url + '/api/session')).status).toBe(401);
  });
});
