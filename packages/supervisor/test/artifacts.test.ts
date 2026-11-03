// R-04, R-13, R-14: every artifact type is made from a chat and from the library, lands in the chat's (sub-)project,
// moves with the chat, rolls up into parents, is versioned by Open Design and exported through it — HTML, and PDF
// and PPTX rendered by the harness host's headless Chromium in place of Open Design's desktop app.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { existsSync, mkdtempSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';

const OD = join(homedir(), '.harness/vendor/open-design');
const home = mkdtempSync(join(tmpdir(), 'harness-art-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = OD;
const haveOd = existsSync(join(OD, 'apps/daemon/dist/daemon-startup.js'));

let base = '', token = '', sup: any, close = async () => {};
const call = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  return r;
};
const ok = async (m: string, p: string, b?: unknown) => { const r = await call(m, p, b); const t = await r.text(); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${t}`); return t ? JSON.parse(t) : {}; };

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
}, 60000);
afterAll(async () => { await close(); });

const DECK = `<!doctype html><html><head><style>body{margin:0;font-family:sans-serif}section.slide{width:1920px;height:1080px;display:flex;align-items:center;justify-content:center;font-size:96px}section.slide:nth-child(1){background:#cc785c}section.slide:nth-child(2){background:#5db8a6}</style></head><body><section class="slide">Castform beta</section><section class="slide">8 mm walls</section></body></html>`;
const CONTENT: Record<string, string> = {
  Doc: '# Mold notes\n\nMinimum wall is 8 mm.\n\n- Mold Star 30\n- Degas twice',
  Deck: DECK,
  Prototype: '<!doctype html><html><body><h1>Beta landing page</h1><button>Join</button></body></html>',
  Sheet: 'part,wall_mm\nbase,8\nlid,6\n',
  Image: '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><circle cx="20" cy="20" r="18" fill="#cc785c"/></svg>',
};

describe('artifacts (R-04, R-13, R-14)', () => {
  let W = '', P = '', S = '', Q = '';
  it('artifacts follow their chat: sub-project placement, move with the chat, roll up to parents; library needs a project', async (ctx) => {
    W = (await ok('POST', '/api/workspaces', { name: 'Art', kind: 'personal' })).id;
    P = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Castform', parent: null })).id;
    S = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Simulation engine', parent: P })).id;
    Q = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Marketing', parent: null })).id;
    const rt = sup.rt(W);
    const chat = await ok('POST', `/api/ws/${W}/chats`, { project: S, title: 'Mold math' });
    const a = await ok('POST', `/api/ws/${W}/artifacts`, { chat: chat.id, type: 'Doc', title: 'Mold notes', content: CONTENT.Doc });
    expect(a.project).toBe(S);
    const dir = (id: string) => join(rt.project(id).folder, '.harness/artifacts', a.id);
    expect(existsSync(dir(S))).toBe(true);
    // Rolls up: Castform's Artifacts tab lists what its sub-projects made.
    const { descendants } = await import('../../shared/src/util.ts');
    const inCastform = rt.state.artifacts.filter((x: any) => [P, ...descendants(rt.state.projects, P).map((p: any) => p.id ?? p)].includes(x.project));
    expect(inCastform.map((x: any) => x.id)).toContain(a.id);
    // Moves with the chat.
    expect((await ok('POST', `/api/ws/${W}/chats/${chat.id}/move`, { project: Q })).movedArtifacts).toBe(1);
    expect(rt.artifacts.get(a.id).project).toBe(Q);
    expect(existsSync(dir(Q))).toBe(true);
    expect(existsSync(dir(S))).toBe(false);
    // The library must pick a project.
    expect((await call('POST', `/api/ws/${W}/artifacts`, { type: 'Doc', title: 'Loose', content: 'x' })).status).toBeGreaterThanOrEqual(400);
    if (!haveOd) ctx.skip();
  }, 120000);

  it('each type is made from a chat and the library, versioned by Open Design, and exported', async (ctx) => {
    if (!haveOd) ctx.skip();
    const chat = await ok('POST', `/api/ws/${W}/chats`, { project: S, title: 'Types' });
    for (const [type, content] of Object.entries(CONTENT)) {
      for (const from of ['chat', 'library']) {
        const a = await ok('POST', `/api/ws/${W}/artifacts`, { ...(from === 'chat' ? { chat: chat.id } : { project: P }), type, title: `${type} from ${from}`, content });
        expect(a.type, type).toBe(type);
        expect(a.versions[0].od?.file, `${type} saved by Open Design`).toBeTruthy();
        const v2 = await ok('POST', `/api/ws/${W}/artifacts/${a.id}/versions`, { content: content.replace(/8/g, '9'), note: 'edit' });
        expect(v2.version).toBe(2);
        expect(v2.versions.at(-1).od?.file).toBeTruthy();
        const { formats } = await ok('GET', `/api/ws/${W}/artifacts/${a.id}/exports`);
        for (const f of formats) {
          const r = await call('GET', `/api/ws/${W}/artifacts/${a.id}/export?format=${f.format}`);
          expect(r.status, `${type} → ${f.format}`).toBe(200);
          const buf = Buffer.from(await r.arrayBuffer());
          expect(buf.length, `${type} → ${f.format}`).toBeGreaterThan(20);
          if (f.format === 'pdf' && ['Doc', 'Deck', 'Prototype'].includes(type)) expect(buf.subarray(0, 5).toString(), `${type} PDF`).toBe('%PDF-');
          if (f.format === 'pptx') expect(buf.subarray(0, 2).toString(), 'PPTX is a zip').toBe('PK');
        }
      }
    }
    // Sharing publishes a static export at an unguessable link that needs no token; off means no link.
    const doc = sup.rt(W).state.artifacts.find((x: any) => x.type === 'Prototype');
    await ok('PATCH', `/api/ws/${W}`, { publicLinks: true });
    const shared = await ok('POST', `/api/ws/${W}/artifacts/${doc.id}/share`);
    expect(shared.scope).toBe('link');
    const page = await fetch(shared.url);
    expect(page.status).toBe(200);
    expect(page.headers.get('content-security-policy')).toMatch(/sandbox/);
    expect(await page.text()).toContain('Beta landing page');
    expect((await fetch(shared.url.replace(/[^/]+$/, 'nope'))).status).toBe(404);
    await ok('PATCH', `/api/ws/${W}`, { publicLinks: false });
    expect((await ok('POST', `/api/ws/${W}/artifacts/${doc.id}/share`)).url).toBeUndefined();
    const audit = (await ok('GET', `/api/ws/${W}/audit`)) as any;
    const lines = JSON.stringify(audit);
    expect(lines).toMatch(/\.pptx via open-design/);
    expect(lines).toMatch(/\.pdf via open-design/);
  }, 600000);
});
