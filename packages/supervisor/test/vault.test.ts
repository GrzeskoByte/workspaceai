// Obsidian vaults: notes can't be reached outside the vault, agents edit only attached folders, edits never clobber a
// changed note, and a save leaves every byte it wasn't asked to change.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, statSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { buildOpencodeJson } from '../../shared/src/opencode-config.ts';

const home = mkdtempSync(join(tmpdir(), 'harness-vault-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';

let base = '', token = '', sup: any, close = async () => {};
const call = (m: string, p: string, b?: unknown) => fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
const ok = async (m: string, p: string, b?: unknown) => { const r = await call(m, p, b); const t = await r.text(); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${t}`); return t ? JSON.parse(t) : {}; };
const q = (o: Record<string, string>) => new URLSearchParams(o).toString();

let W = '', V = '', V2 = '', P = '', Q = '', vault = '', other = '', folderP = '', outside = '';

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

  vault = realpathSync(mkdtempSync(join(tmpdir(), 'vault-a-')));
  other = realpathSync(mkdtempSync(join(tmpdir(), 'vault-b-')));
  outside = realpathSync(mkdtempSync(join(tmpdir(), 'outside-')));
  for (const d of ['Work', 'Private', '.obsidian']) mkdirSync(join(vault, d));
  writeFileSync(join(vault, 'Work/plan.md'), '# Plan\nalpha\n');
  writeFileSync(join(vault, 'Private/diary.md'), 'secret entry\n');
  writeFileSync(join(vault, '.obsidian/config.md'), 'hidden');
  writeFileSync(join(other, 'elsewhere.md'), 'hello');
  writeFileSync(join(outside, 'loot.md'), 'loot');
  symlinkSync(join(outside, 'loot.md'), join(vault, 'Work/escape.md'));
  symlinkSync(outside, join(vault, 'Work/outdir'));
  symlinkSync(join(vault, '.obsidian/config.md'), join(vault, 'Work/hidden.md'));
  symlinkSync(join(vault, 'gone.md'), join(vault, 'Work/dangling.md'));
  symlinkSync(join(vault, 'Private/diary.md'), join(vault, 'Work/diary-link.md'));

  W = (await ok('POST', '/api/workspaces', { name: 'Notes', kind: 'personal' })).id;
  const snap = await ok('PATCH', `/api/ws/${W}`, { vaults: [{ name: 'Main', path: vault }, { name: 'Other', path: other }] });
  [V, V2] = snap.workspace.vaults.map((v: any) => v.id);
  P = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Attached', parent: null })).id;
  Q = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Bare', parent: null })).id;
  await ok('PATCH', `/api/ws/${W}/projects/${P}`, { vaults: [{ vaultId: V, folder: 'Work' }] });
  folderP = sup.rt(W).project(P).folder;
}, 60000);
afterAll(async () => { await close(); });

const file = (v: string, path: string) => `/api/ws/${W}/vaults/${v}/file?${q({ path })}`;
const agent = (action: string, body: Record<string, unknown>) => call('POST', `/api/ws/${W}/agent/vault/${action}`, { vault: V, folder: folderP, ...body });

describe('vault paths', () => {
  it('rejects anything outside the vault or hidden, for reads and writes', async () => {
    for (const path of ['../x.md', '/etc/passwd.md', 'Work/../../x.md', '.obsidian/config.md', 'Work/plan.txt', 'a\0b.md', 'Work\\plan.md', 'Work/escape.md', 'Work/hidden.md', 'Work/dangling.md', 'Work/outdir/new.md']) {
      expect((await call('GET', file(V, path))).status, 'read ' + path).toBeGreaterThanOrEqual(400);
      expect((await call('PUT', `/api/ws/${W}/vaults/${V}/file`, { path, text: 'x', hash: null })).status, 'write ' + path).toBeGreaterThanOrEqual(400);
    }
    expect(existsSync(join(outside, 'new.md'))).toBe(false);
    expect(readFileSync(join(outside, 'loot.md'), 'utf8')).toBe('loot');
  });

  it('lists notes only, and no hidden folders or symlinks', async () => {
    const t = await ok('GET', `/api/ws/${W}/vaults/${V}/tree`);
    expect(t.files.map((f: any) => f.path)).toEqual(['Private/diary.md', 'Work/plan.md']);
    expect(t.dirs).toEqual(['Private', 'Work']);
  });

  it('refuses vaults that aren’t folders and attachments that don’t exist', async () => {
    expect((await call('PATCH', `/api/ws/${W}`, { vaults: [{ name: 'Nope', path: join(home, 'missing') }] })).status).toBe(400);
    for (const folder of ['Nope', '.obsidian', '../x', 'Work/outdir']) expect((await call('PATCH', `/api/ws/${W}/projects/${Q}`, { vaults: [{ vaultId: V, folder }] })).status, folder).toBeGreaterThanOrEqual(400);
  });
});

describe('what agents can reach', () => {
  it('edits in the attached folder, and nowhere else', async () => {
    expect((await agent('write', { path: 'Work/new.md', content: 'made by agent' })).status).toBe(200);
    expect(readFileSync(join(vault, 'Work/new.md'), 'utf8')).toBe('made by agent');
    for (const [v, path] of [[V, 'Private/diary.md'], [V, 'Private/new.md'], [V, 'root.md'], [V2, 'elsewhere.md'], [V, 'Work/diary-link.md']]) {
      const r = await agent('write', { vault: v, path, content: 'x', hash: 'whatever' });
      expect(r.status, path).toBe(403);
    }
    expect(readFileSync(join(vault, 'Private/diary.md'), 'utf8')).toBe('secret entry\n');
    expect(existsSync(join(vault, 'root.md'))).toBe(false);
  });

  it('reads everywhere in the workspace’s vaults', async () => {
    const r = await ok('GET', `/api/ws/${W}/agent/vault/read?${q({ vault: 'Other', path: 'elsewhere.md' })}`);
    expect(r.text).toBe('hello');
    const s = await ok('GET', `/api/ws/${W}/agent/vault/search?${q({ q: 'SECRET' })}`);
    expect(s.hits.map((h: any) => h.path)).toEqual(['Private/diary.md']);
  });

  it('an unknown caller can’t write, unless one project alone has a chat running', async () => {
    const rt = sup.rt(W);
    const unknown = { folder: join(home, 'nowhere'), path: 'Work/who.md', content: 'x' };
    expect((await agent('write', unknown)).status).toBe(403);
    const chatP = await ok('POST', `/api/ws/${W}/chats`, { project: P, title: 'A' });
    const chatQ = await ok('POST', `/api/ws/${W}/chats`, { project: Q, title: 'B' });
    rt.live.set(chatP.id, {});
    expect((await agent('write', unknown)).status).toBe(200);
    rt.live.set(chatQ.id, {});
    expect((await agent('write', { ...unknown, path: 'Work/who2.md' })).status).toBe(403);
    rt.live.clear();
  });

  it('General’s folder alone doesn’t count as General, because opencode’s engine runs there', async () => {
    const rt = sup.rt(W);
    const general = rt.general();
    await ok('PATCH', `/api/ws/${W}/projects/${general.id}`, { vaults: [{ vaultId: V, folder: 'Private' }] });
    const body = { folder: general.folder, path: 'Private/general.md', content: 'x' };
    const chatP = await ok('POST', `/api/ws/${W}/chats`, { project: P, title: 'C' });
    rt.live.set(chatP.id, {});
    expect((await agent('write', body)).status).toBe(403);
    rt.live.clear();
    expect((await agent('write', body)).status).toBe(403);
    await ok('PATCH', `/api/ws/${W}/projects/${general.id}`, { vaults: [] });
  });

  it('the rule tells each project what it can edit', () => {
    const rt = sup.rt(W);
    expect(rt.inheritedPrefix(P)).toContain('You can edit notes in: Main/Work.');
    expect(rt.inheritedPrefix(Q)).toContain('read-only for this project');
  });

  it('an edit must match exactly once', async () => {
    writeFileSync(join(vault, 'Work/edit.md'), 'one two two\n');
    expect((await agent('edit', { path: 'Work/edit.md', old: 'three', new: 'x' })).status).toBe(409);
    expect((await agent('edit', { path: 'Work/edit.md', old: 'two', new: 'x' })).status).toBe(409);
    expect((await agent('edit', { path: 'Work/edit.md', old: 'one', new: '$& 1' })).status).toBe(200);
    expect(readFileSync(join(vault, 'Work/edit.md'), 'utf8')).toBe('$& 1 two two\n');
  });

  it('asks before vault writes when the workspace asks first', () => {
    const { json } = buildOpencodeJson(sup.rt(W).ws, null, {});
    expect((json.permission as any)['harness-vault_vault_write']).toBe('ask');
    expect((json.permission as any)['harness-vault_vault_edit']).toBe('ask');
  });
});

describe('saving', () => {
  const put = (path: string, b: Record<string, unknown>) => call('PUT', `/api/ws/${W}/vaults/${V}/file`, { path, ...b });

  it('won’t overwrite a note that changed or was deleted', async () => {
    const path = 'Private/conflict.md', real = join(vault, path);
    expect((await put(path, { text: 'v1', hash: null })).status).toBe(200);
    expect((await put(path, { text: 'again', hash: null })).status).toBe(409);
    const { hash } = await ok('GET', file(V, path));
    writeFileSync(real, 'changed in Obsidian');
    expect((await put(path, { text: 'mine', hash })).status).toBe(409);
    expect(readFileSync(real, 'utf8')).toBe('changed in Obsidian');
    expect((await put(path, { text: 'mine', hash, force: true })).status).toBe(200);
    expect(readFileSync(real, 'utf8')).toBe('mine');
    const fresh = await ok('GET', file(V, path));
    await ok('DELETE', file(V, path));
    expect(existsSync(real)).toBe(false);
    expect(existsSync(join(vault, '.trash/conflict.md'))).toBe(true);
    expect((await put(path, { text: 'back', hash: fresh.hash })).status).toBe(409);
  });

  it('keeps CRLF, BOM and file mode, and leaves no temp file', async () => {
    const path = 'Private/bytes.md', real = join(vault, path);
    const bom = Buffer.from([0xef, 0xbb, 0xbf]);
    writeFileSync(real, Buffer.concat([bom, Buffer.from('a\r\nb\r\n')]));
    chmodSync(real, 0o600);
    const note = await ok('GET', file(V, path));
    expect(note.text).toBe('a\r\nb\r\n');
    await ok('PUT', `/api/ws/${W}/vaults/${V}/file`, { path, text: note.text + 'c\r\n', hash: note.hash });
    expect(readFileSync(real).equals(Buffer.concat([bom, Buffer.from('a\r\nb\r\nc\r\n')]))).toBe(true);
    expect(statSync(real).mode & 0o777).toBe(0o600);
    expect(readdirSync(join(vault, 'Private')).filter((f) => f.endsWith('.tmp'))).toEqual([]);
  });

  it('an agent’s newlines become CRLF in a CRLF note', async () => {
    writeFileSync(join(vault, 'Work/crlf.md'), 'a\r\nb\r\n');
    expect((await agent('edit', { path: 'Work/crlf.md', old: 'a\nb', new: 'x\ny' })).status).toBe(200);
    expect(readFileSync(join(vault, 'Work/crlf.md'), 'utf8')).toBe('x\r\ny\r\n');
  });

  it('won’t edit what it can’t round-trip', async () => {
    writeFileSync(join(vault, 'Work/latin.md'), Buffer.from([0x63, 0x61, 0x66, 0xe9]));
    const note = await ok('GET', file(V, 'Work/latin.md'));
    expect(note.readOnly).toBe(true);
    expect((await agent('edit', { path: 'Work/latin.md', old: 'c', new: 'x' })).status).toBe(409);
  });

  it('moves refuse to overwrite; removing a vault detaches it and leaves its files', async () => {
    await ok('POST', `/api/ws/${W}/vaults/${V}/move`, { path: 'Work/new.md', to: 'Work/sub/moved.md' });
    expect(existsSync(join(vault, 'Work/sub/moved.md'))).toBe(true);
    expect((await call('POST', `/api/ws/${W}/vaults/${V}/move`, { path: 'Work/sub/moved.md', to: 'Work/plan.md' })).status).toBe(409);
    await ok('PATCH', `/api/ws/${W}`, { vaults: [{ id: V2, name: 'Other', path: other }] });
    expect(sup.rt(W).project(P).vaults).toEqual([]);
    expect(existsSync(join(vault, 'Work/plan.md'))).toBe(true);
  });
});
