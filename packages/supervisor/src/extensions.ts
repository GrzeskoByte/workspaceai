// Registers the harness's own MCP servers (memory, artifacts) in every workspace and the routes they call back into.
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Supervisor } from './supervisor.ts';
import type { Router, StartedServer } from './server.ts';
import type { WorkspaceRuntime } from './runtime.ts';
import { projectPathName } from '../../shared/src/util.ts';
import { exportsFor } from './artifacts.ts';
import { openDesign } from './opendesign.ts';
import { installLeanCtx, leanCtxBin, leanCtxDir, leanCtxGain, leanCtxMcp, leanCtxOptions, leanCtxVersion, setLeanCtxOption, setLeanCtxToolProfile } from './leanctx.ts';
import { artifactGuide, isGuided, markGuided, type GuideType } from './design-guide.ts';
import { odSettings } from '../../shared/src/defaults.ts';
import { readText } from './jsonfile.ts';
import { installGraphRoutes } from './graph-tools.ts';
import { installVaultRoutes } from './vault.ts';
import { applyOps, resolveMap } from './maps.ts';
import { refreshGitlab, ticketDetail, tokenName } from './gitlab.ts';
import { toOutline } from '../../shared/src/mindmap.ts';
import { detectDevCommand, devStatus, framePolicy, openExternal, serversIn, startDev, stopDev } from './devserver.ts';
import { expandHome } from './paths.ts';
import { keychain } from './keychain.ts';
import { searchFiles } from './files.ts';
import { randomBytes } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';

const here = dirname(fileURLToPath(import.meta.url));
export const MEMORY_MCP = join(here, '../../memory-mcp/src/server.ts');
export const ARTIFACTS_MCP = join(here, '../../memory-mcp/src/artifacts-server.ts');
export const VAULT_MCP = join(here, '../../memory-mcp/src/vault-server.ts');

/** The chat whose session most recently called an artifact tool (within two minutes). */
/** The harness tool type for each artifact kind. */
const TOOL_TYPE: Record<string, string> = { Doc: 'doc', Deck: 'deck', Prototype: 'prototype', Sheet: 'sheet', LiveArtifact: 'prototype', Image: 'prototype', Video: 'doc', DesignSystem: 'design-system' };

function callingChat(rt: WorkspaceRuntime): string | null {
  const recent = rt.artifactCalls.filter((c) => Date.now() - c.at < 120000).sort((a, b) => b.at - a.at)[0];
  if (recent) return rt.state.chats.find((c) => c.sid === recent.sid)?.id ?? null;
  // Fall back to the only chat that is currently streaming.
  const streaming = rt.state.chats.filter((c) => rt.isStreaming(c.id));
  return streaming.length === 1 ? streaming[0].id : null;
}

export function installExtensions(sup: Supervisor, router: Router, server: StartedServer): void {
  const env = (rt: WorkspaceRuntime) => ({ HARNESS_MEMORY_DIR: rt.paths.memory, HARNESS_MEMORY_BACKEND: rt.ws.memory, HARNESS_WS: rt.id, HARNESS_SUPERVISOR: '{env:HARNESS_SUPERVISOR}', HARNESS_SUPERVISOR_TOKEN: '{env:HARNESS_SUPERVISOR_TOKEN}' });
  sup.hooksExtra = {
    // Memory is an MCP server opencode starts, one store per workspace (R-22, R-23).
    // Artifacts: always the harness artifact tools, which save every version through Open Design (R-04).
    // Open Design's own MCP tools are added only when the workspace asks for them.
    extraMcp: (rt) => {
      const out: Record<string, unknown> = {};
      if (rt.ws.useMemory !== false) out['harness-memory'] = { type: 'local', command: [process.execPath, MEMORY_MCP], enabled: true, environment: env(rt) };
      const od = odSettings(rt.ws).agentTools ? openDesign.mcpCommand(rt) : null;
      if (od) out['open-design'] = { type: 'local', command: od.command, enabled: true, environment: od.environment };
      out['harness-artifacts'] = { type: 'local', command: [process.execPath, ARTIFACTS_MCP], enabled: true, environment: env(rt) };
      // Always registered, and scope is read on each call, so attaching a vault never has to restart running chats.
      out['harness-vault'] = { type: 'local', command: [process.execPath, VAULT_MCP], enabled: true, environment: env(rt) };
      // lean-ctx: compressed reads, searches and shell output for every agent, when the workspace turns it on.
      const lc = rt.ws.leanCtx?.enabled !== false ? leanCtxMcp() : null;
      if (lc) out['lean-ctx'] = { type: 'local', command: lc.command, enabled: true, environment: lc.environment };
      return out;
    },
    extraEnv: () => ({ HARNESS_SUPERVISOR: `http://127.0.0.1:${server.port}`, HARNESS_SUPERVISOR_TOKEN: server.token }),
    beforeStart: async (rt) => {
      const o = odSettings(rt.ws);
      if (o.engine === 'builtin') return [{ name: 'Open Design', state: 'off · built-in artifact store' }];
      // Not started with the workspace: it starts on the first artifact save instead.
      if (!o.autoStart && !o.agentTools) return [{ name: 'Open Design', state: openDesign.cliPath() ? 'starts with the first artifact' : 'not installed' }];
      const url = await openDesign.ensure(rt); return url ? [{ name: 'Open Design daemon', state: 'running · ' + url }] : [{ name: 'Open Design', state: openDesign.cliPath() ? 'error' : 'not installed · built-in artifact store' }]; },
    afterStop: (rt) => openDesign.stop(rt),
  };

  // ---------- lean-ctx (one install for the machine; on or off per workspace) ----------
  router.get('/api/leanctx', async () => ({ installed: !!leanCtxBin(), version: await leanCtxVersion(), bin: leanCtxBin(), dir: leanCtxDir() }));
  router.post('/api/leanctx/install', async () => {
    const r = await installLeanCtx();
    // Running workspaces pick up the new binary for their agents.
    if (r.ok) for (const rt of sup.runtimes.values()) if (rt.ws.leanCtx?.enabled !== false) void rt.restartEngine().catch(() => {});
    return r;
  });
  router.get('/api/leanctx/gain', ({ query }) => leanCtxGain(query.get('period') || undefined));
  router.get('/api/leanctx/options', () => leanCtxOptions());
  router.post('/api/leanctx/options', async ({ body }) => {
    const note = body.toolProfile ? await setLeanCtxToolProfile(String(body.toolProfile)) : await setLeanCtxOption(String(body.key || ''), body.value);
    return { note };
  });

  // ---------- the browser beside the chat: dev servers and pages ----------
  const devOf = (rt: WorkspaceRuntime, pid: string) => {
    const p = rt.project(pid);
    if (!p) throw Object.assign(new Error('Project not found'), { status: 404 });
    const folder = expandHome(p.folder || rt.general().folder);
    return { p, folder, key: rt.id + ':' + p.id, detected: detectDevCommand(folder) };
  };
  router.get('/api/ws/:id/projects/:pid/dev', async ({ params }) => {
    const rt = sup.rt(params.id);
    const { p, folder, key, detected } = devOf(rt, params.pid);
    return { ...devStatus(key, p.devCommand || detected), detected, folder, servers: await serversIn(folder) };
  });
  router.post('/api/ws/:id/projects/:pid/dev', ({ params, body }) => {
    const rt = sup.rt(params.id);
    const { p, folder, key, detected } = devOf(rt, params.pid);
    if (typeof body.command === 'string') { p.devCommand = body.command.trim() && body.command.trim() !== detected ? body.command.trim().slice(0, 500) : undefined; rt.save(); }
    if (body.action === 'stop') { stopDev(key); rt.audit('dev.stop', p.name); return devStatus(key, p.devCommand || detected); }
    if (body.action === 'start') { rt.audit('dev.start', `${p.name} · ${p.devCommand || detected}`); return startDev(key, folder, p.devCommand || detected); }
    return devStatus(key, p.devCommand || detected);
  });
  router.get('/api/ws/:id/projects/:pid/files', async ({ params, query }) => {
    const p = sup.rt(params.id).project(params.pid);
    if (!p) throw Object.assign(new Error('Project not found'), { status: 404 });
    return { files: await searchFiles(expandHome(p.folder), String(query.get('q') || '')) };
  });
  router.get('/api/preview/check',({ query }) => framePolicy(String(query.get('url') || '')));

  // ---------- mind maps: one per project, and the workspace overview ----------
  router.post('/api/open-external', async ({ body }) => { await openExternal(String(body.url || '')); return { ok: true }; });
  router.get('/api/ws/:id/projects/:pid/tickets/:iid', ({ params }) => {
    const rt = sup.rt(params.id);
    const t = ticketDetail(rt, rt.project(params.pid)?.folder || '', Number(params.iid));
    if (!t) throw Object.assign(new Error('That ticket is no longer open'), { status: 404 });
    return t;
  });
  router.post('/api/ws/:id/projects/:pid/gitlab-token', ({ params, body }) => {
    const rt = sup.rt(params.id);
    const p = rt.project(params.pid);
    if (!p) throw Object.assign(new Error('Project not found'), { status: 404 });
    if (!body.host || !String(body.token || '').trim()) throw new Error('Paste a GitLab token first');
    keychain.set(rt.id, tokenName(String(body.host)), String(body.token).trim());
    refreshGitlab(rt, expandHome(p.folder));
    rt.emit({ type: 'map', workspace: rt.id, scope: p.id });
    return { ok: true };
  });
  router.get('/api/ws/:id/map', ({ params }) => resolveMap(sup.rt(params.id), 'ws'));
  router.post('/api/ws/:id/map/ops', ({ params, body }) => applyOps(sup.rt(params.id), 'ws', body.ops || []));
  router.get('/api/ws/:id/projects/:pid/map', ({ params }) => resolveMap(sup.rt(params.id), params.pid));
  router.post('/api/ws/:id/projects/:pid/map/ops', ({ params, body }) => applyOps(sup.rt(params.id), params.pid, body.ops || []));
  // Agents work on the map of the project their chat is in.
  const agentScope = (rt: WorkspaceRuntime, project?: unknown) => {
    if (project && rt.project(String(project))) return String(project);
    const chat = callingChat(rt);
    return (chat && rt.chat(chat)?.project) || rt.ws.defaultProject || rt.general().id;
  };
  router.get('/api/ws/:id/agent/map', ({ params, query }) => {
    const rt = sup.rt(params.id);
    return { outline: toOutline(resolveMap(rt, agentScope(rt, query.get('project')))) };
  });
  router.post('/api/ws/:id/agent/map/ops', ({ params, body }) => {
    const rt = sup.rt(params.id);
    const ops = (body.ops || []).map((o: Record<string, unknown>) => (o.op === 'add' || o.op === 'import' ? { ...o, by: 'agent' } : o));
    const r = applyOps(rt, agentScope(rt, body.project), ops);
    return { added: r.added, outline: toOutline(r.map) };
  });

  // ---------- graphs as agent tools ----------
  installGraphRoutes(sup, router, callingChat);
  installVaultRoutes(sup, router);

  // ---------- artifact routes (agent tools and the UI) ----------
  // Open Design's reference (design system, craft, skills); agents read it before they save, so every artifact follows it.
  router.post('/api/ws/:id/agent/guide', async ({ params, body }) => {
    const rt = sup.rt(params.id);
    const type = (['doc', 'deck', 'prototype', 'sheet', 'design-system'].includes(body.type) ? body.type : 'doc') as GuideType;
    const guide = await artifactGuide(rt, type, body.skill || undefined);
    markGuided(body.chat || callingChat(rt), type);
    return { guide };
  });
  const needGuide = (chat: string | null, type: string) => {
    if (!isGuided(chat, type)) throw new Error(`Read Open Design's reference first: call artifact_guide with type "${type}", follow the design system it returns, then call this tool again.`);
  };
  router.post('/api/ws/:id/agent/artifacts', async ({ params, body }) => {
    const rt = sup.rt(params.id);
    const chat = body.chat || callingChat(rt);
    needGuide(chat, String(body.type || 'doc'));
    const a = await rt.artifacts.createVia({ chat, project: chat ? undefined : rt.ws.defaultProject, type: body.type, title: body.title, content: String(body.content ?? ''), note: body.note, format: body.format });
    if (a.chat) rt.emit({ type: 'artifact', workspace: rt.id, chat: a.chat, artifact: a.id, version: a.version });
    return { ...a, projectPath: projectPathName(rt.state.projects, a.project) };
  });
  router.post('/api/ws/:id/agent/artifacts/:aid', async ({ params, body }) => {
    const rt = sup.rt(params.id);
    const chat = callingChat(rt);
    needGuide(chat, TOOL_TYPE[rt.artifacts.get(params.aid).type] || 'doc');
    const a = await rt.artifacts.reviseVia(params.aid, String(body.content ?? ''), { chat, note: body.note, title: body.title, format: body.format });
    if (chat) rt.emit({ type: 'artifact', workspace: rt.id, chat, artifact: a.id, version: a.version });
    return a;
  });
  router.post('/api/ws/:id/artifacts', async ({ params, body }) => {
    const rt = sup.rt(params.id);
    if (!body.chat && !body.project) throw new Error('Artifacts made from the library must pick a project');
    return rt.artifacts.createVia({ chat: body.chat ?? null, project: body.project, type: body.type, title: body.title, content: String(body.content ?? ''), note: body.note, format: body.format });
  });
  router.get('/api/ws/:id/artifacts/:aid/content', ({ params, query }) => sup.rt(params.id).artifacts.view(params.aid, Number(query.get('v')) || undefined));
  router.post('/api/ws/:id/artifacts/:aid/versions', ({ params, body }) => sup.rt(params.id).artifacts.reviseVia(params.aid, String(body.content ?? ''), { chat: body.chat ?? null, note: body.note }, 'manual'));
  router.post('/api/ws/:id/artifacts/:aid/restore', ({ params, body }) => sup.rt(params.id).artifacts.restoreVia(params.aid, Number(body.v)));
  router.post('/api/ws/:id/artifacts/:aid/move', ({ params, body }) => sup.rt(params.id).artifacts.move(params.aid, body.project));
  router.del('/api/ws/:id/artifacts/:aid', ({ params }) => { sup.rt(params.id).artifacts.remove(params.aid); return { ok: true }; });
  router.get('/api/ws/:id/artifacts/:aid/exports', ({ params }) => ({ formats: exportsFor(sup.rt(params.id).artifacts.get(params.aid)) }));
  router.get('/api/ws/:id/artifacts/:aid/export', async ({ params, query, res }) => {
    const rt = sup.rt(params.id);
    const out = await rt.artifacts.exportVia(params.aid, query.get('format') || 'md', Number(query.get('v')) || undefined);
    rt.audit('artifact.export', `${out.filename} via ${out.via}${out.note ? ' · ' + out.note : ''}`);
    res.writeHead(200, { 'content-type': out.mime + (typeof out.body === 'string' ? '; charset=utf-8' : ''), 'content-disposition': `attachment; filename="${out.filename}"` }).end(out.body);
  });
  // Sharing publishes a static export (never the live daemon) at an unguessable link; org policy can forbid it.
  router.post('/api/ws/:id/artifacts/:aid/share', async ({ params }) => {
    const rt = sup.rt(params.id);
    if (rt.ws.kind === 'org' && rt.policy?.blockExternalSharing) { rt.audit('artifact.share', 'org-only link'); return { scope: 'organization', note: 'Shared with your organization (public links are off)' }; }
    if (!rt.ws.publicLinks) return { scope: 'private', note: 'Public links are off for this workspace — turn them on in Settings › Artifacts & sharing' };
    const out = await rt.artifacts.exportVia(params.aid, 'html');
    const key = randomBytes(18).toString('base64url');
    mkdirSync(join(rt.paths.data, 'shares'), { recursive: true });
    writeFileSync(join(rt.paths.data, 'shares', key + '.html'), out.body);
    const url = `http://127.0.0.1:${server.port}/api/shared/${rt.id}/${key}`;
    rt.audit('artifact.share', `${out.filename} · static link`);
    return { scope: 'link', url, note: 'Link copied — a static export, never the live daemon' };
  });
  router.get('/api/shared/:wsid/:key', ({ params, res }) => {
    const file = join(sup.rt(params.wsid).paths.data, 'shares', String(params.key).replace(/[^\w-]/g, '') + '.html');
    if (!existsSync(file)) { res.writeHead(404, { 'content-type': 'text/plain' }).end('This link was removed'); return; }
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'content-security-policy': "sandbox allow-scripts; default-src 'self' 'unsafe-inline' data: blob:", 'x-content-type-options': 'nosniff' }).end(readFileSync(file));
  });


  // ---------- Open Design ----------
  router.get('/api/opendesign/status', () => openDesign.status());
  router.post('/api/opendesign/install', () => openDesign.install());
  router.post('/api/opendesign/update', () => openDesign.update());
  // Settings › Open Design: everything about this workspace's artifact engine in one call.
  router.get('/api/ws/:id/opendesign', async ({ params }) => {
    const rt = sup.rt(params.id);
    const arts = rt.state.artifacts;
    const latest = arts.map((a) => a.versions[a.versions.length - 1]).filter(Boolean);
    return {
      install: openDesign.status(),
      daemon: await openDesign.wsStatus(rt),
      settings: odSettings(rt.ws),
      dataDir: rt.paths.openDesign,
      log: join(rt.paths.logs, 'open-design.log'),
      projects: Object.keys(rt.state.odProjects || {}).length,
      artifacts: { total: arts.length, inOpenDesign: latest.filter((v) => v.od).length, lintFindings: latest.reduce((n, v) => n + (v.od?.lint?.length || 0), 0) },
      agentTools: !!readText(rt.paths.opencodeJson).includes('"open-design"'),
    };
  });
  router.post('/api/ws/:id/opendesign/start', async ({ params }) => { const rt = sup.rt(params.id); await openDesign.require(rt); return openDesign.wsStatus(rt); });
  router.post('/api/ws/:id/opendesign/restart', async ({ params }) => { const rt = sup.rt(params.id); await openDesign.restart(rt); rt.audit('open-design.restart', 'from settings'); return openDesign.wsStatus(rt); });
  router.post('/api/ws/:id/opendesign/stop', async ({ params }) => { const rt = sup.rt(params.id); await openDesign.stop(rt); rt.audit('open-design.stop', 'from settings'); return openDesign.wsStatus(rt); });
  router.get('/api/ws/:id/opendesign/logs', ({ params }) => ({ text: readText(join(sup.rt(params.id).paths.logs, 'open-design.log')).split('\n').slice(-300).join('\n') }));
  router.post('/api/ws/:id/opendesign/resync', async ({ params }) => { const rt = sup.rt(params.id); const r = await rt.artifacts.resyncOpenDesign(); rt.audit('open-design.resync', `${r.synced} synced, ${r.failed.length} failed`); return r; });
}
