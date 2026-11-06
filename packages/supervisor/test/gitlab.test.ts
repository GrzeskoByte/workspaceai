import { describe, it, expect, afterAll } from 'vitest';
import { execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { parseRemote, repoNodes, tokenName } from '../src/gitlab.ts';
import { openExternal } from '../src/devserver.ts';
import { piProviders } from '../src/pi-config.ts';

describe('parseRemote', () => {
  it('reads ssh, scp-style and https remotes, without credentials', () => {
    expect(parseRemote('git@gitlab.example.com:group/sub/app.git\n')).toEqual({ origin: 'https://gitlab.example.com', host: 'gitlab.example.com', path: 'group/sub/app' });
    expect(parseRemote('ssh://git@gitlab.example.com:2222/group/app.git')?.origin).toBe('https://gitlab.example.com');
    expect(parseRemote('https://me:secret@gitlab.example.com:8443/group/app/')).toEqual({ origin: 'https://gitlab.example.com:8443', host: 'gitlab.example.com', path: 'group/app' });
  });
  it('refuses what is not a hosted repository, or a host that could break git’s credential protocol', () => {
    for (const u of ['', '/srv/git/app.git', '../app', 'git@host:app.git', 'git@ho\nst=evil:group/app.git']) expect(parseRemote(u)).toBeNull();
  });
  it('keeps one token name per host', () => { expect(tokenName('gitlab.a.com')).not.toBe(tokenName('gitlab.b.com')); });
});

describe('tickets on the map', () => {
  const gitlab = createServer((req, res) => {
    const u = new URL(req.url!, 'http://x');
    res.setHeader('content-type', 'application/json');
    if (!u.pathname.startsWith('/api/v4/projects/group%2Fapp/')) { res.statusCode = 404; res.end('[]'); return; }
    // Two pages of tickets, so paging is covered.
    if (u.pathname.endsWith('/issues') && u.searchParams.get('page') === '1') res.setHeader('x-next-page', '2');
    if (u.pathname.endsWith('/milestones')) res.end(JSON.stringify([{ id: 7, title: 'v1', due_date: '2026-12-01', web_url: 'http://m/7' }, { id: 8, title: 'Empty' }]));
    else if (u.searchParams.get('page') === '1') res.end(JSON.stringify([{ id: 1, iid: 11, title: 'In v1', web_url: 'http://t/11', milestone: { id: 7, title: 'v1' }, assignee: { name: 'Ana' }, labels: ['bug'] }]));
    else res.end(JSON.stringify([{ id: 2, iid: 12, title: 'Loose', web_url: 'http://t/12', milestone: null }]));
  });
  afterAll(() => gitlab.close());

  const project = (remote: string) => {
    const folder = mkdtempSync(join(tmpdir(), 'gl-repo-'));
    execFileSync('git', ['init', '-q', folder]);
    execFileSync('git', ['-C', folder, 'remote', 'add', 'origin', remote]);
    return folder;
  };
  const refreshed = (folder: string) => new Promise<any[]>((done) => {
    const rt: any = { id: 'w', paths: { data: mkdtempSync(join(tmpdir(), 'gl-data-')) }, emit: () => done(repoNodes(rt, 'p', folder, 'project:p')) };
    expect(repoNodes(rt, 'p', folder, 'project:p')).toEqual([]);
  });

  it('hangs tickets off their milestone, under the repository', async () => {
    await new Promise<void>((r) => gitlab.listen(0, '127.0.0.1', r));
    const port = (gitlab.address() as { port: number }).port;
    const nodes = await refreshed(project(`http://127.0.0.1:${port}/group/app.git`));
    const at = (id: string) => nodes.find((n) => n.id === id);
    expect(at('repo:group/app')).toMatchObject({ kind: 'repo', parent: 'project:p', meta: '2 open tickets' });
    expect(at('milestone:7')).toMatchObject({ title: 'v1', parent: 'repo:group/app', meta: '1 open · due 2026-12-01', folded: true });
    expect(at('milestone:8')).toMatchObject({ title: 'Empty', folded: false });
    expect(at('ticket:1')).toMatchObject({ title: '#11 In v1', parent: 'milestone:7', meta: 'Ana · bug', url: 'http://t/11' });
    expect(at('ticket:2').parent).toBe('milestone:none:group/app');
    expect(at('milestone:none:group/app')).toMatchObject({ title: 'No milestone', meta: '1 open' });
  });

  it('says a token is needed when GitLab refuses, and shows no tickets for GitHub', async () => {
    const port = (gitlab.address() as { port: number }).port;
    const denied = await refreshed(project(`http://127.0.0.1:${port}/group/private.git`));
    expect(denied).toHaveLength(1);
    expect(denied[0]).toMatchObject({ meta: 'needs a GitLab token', needsToken: true });
    const hub = await refreshed(project('git@github.com:me/app.git'));
    expect(hub.map((n) => n.id)).toEqual(['repo:me/app']);
  });
});

describe('openExternal', () => {
  it('only hands web pages to the system', () => {
    for (const u of ['file:///etc/passwd', 'javascript:alert(1)', '--help', '']) expect(() => openExternal(u)).toThrow('Only http and https');
  });
});

describe('piProviders', () => {
  const base = { name: 'X', note: '', models: [{ id: 'm1', name: 'Model', context: 1000, output: 10 }] };
  it('describes a custom endpoint, by key reference only', () => {
    const out: any = piProviders([{ ...base, id: 'lite', baseURL: 'http://h/v1', keyEnv: 'LITE_KEY', npm: '@ai-sdk/openai-compatible' }], () => true);
    expect(out.lite).toEqual({ baseUrl: 'http://h/v1', api: 'openai-completions', apiKey: '$LITE_KEY', models: [{ id: 'm1', name: 'Model', contextWindow: 1000, maxTokens: 10 }] });
  });
  it('gives a keyless local endpoint a placeholder, or Pi hides its models', () => {
    expect((piProviders([{ ...base, id: 'ollama', baseURL: 'http://h/v1', keyEnv: 'OLLAMA_API_KEY' }], () => false) as any).ollama.apiKey).toBe('none');
  });
  it('leaves known providers to Pi unless they go through a proxy', () => {
    expect(piProviders([{ ...base, id: 'anthropic', keyEnv: 'ANTHROPIC_API_KEY' }, { ...base, id: 'fromoc', baseURL: 'http://h', fromOpencode: true }], () => true)).toEqual({});
    expect(piProviders([{ ...base, id: 'anthropic', keyEnv: 'K', baseURL: 'http://p' }], () => true)).toEqual({ anthropic: { baseUrl: 'http://p', apiKey: '$K' } });
  });
});
