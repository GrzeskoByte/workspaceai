import { Router } from './server.ts';
import type { Supervisor } from './supervisor.ts';
import type { Provider, Workspace, McpServer, ModelGroup, ModelOption } from '../../shared/src/types.ts';
import { uid } from '../../shared/src/util.ts';
import { webConfigPreview, buildOpencodeJson, WEB_PROVIDERS } from '../../shared/src/opencode-config.ts';
import { AGENTS_TEMPLATES } from '../../shared/src/defaults.ts';
import { keychain } from './keychain.ts';
import { findOpencode, LocalOpencodeData, copySessions } from './detect.ts';
import { existsSync } from 'node:fs';
import { OpencodeClient, installOpencode, pinnedOpencodeBin } from './opencode.ts';
import { readText } from './jsonfile.ts';
import { join } from 'node:path';
import { registerScreens } from './screens.ts';
import { cleanVaults, pruneAttachments } from './vault.ts';
import { registerPipeline } from './pipeline.ts';
import { canSignIn, cancelSignIn, signInCode, signInState, startSignIn } from './signin.ts';
import { SUBSCRIPTIONS, connectSubscription, detectedRows, disconnectSubscription, subscriptionStatus } from './subscriptions.ts';
import { getMemoryGraph, graphifyStatus, graphifyVersion, memoryNodeDetail, queryMemoryGraph, reviewLink, rebuildMemoryGraph, installGraphify } from './graphify.ts';
import type { SubscriptionId } from '../../shared/src/types.ts';

const local = new LocalOpencodeData();

function err(message: string, status = 400): never { throw Object.assign(new Error(message), { status }); }

/**
 * Test an OpenAI-compatible endpoint (R-20 "connections can be tested"). With a model it asks for a short reply, the same
 * request opencode sends, because listing models can succeed where chat fails. Without one it can only list models.
 */
export async function testEndpoint(baseURL: string, key?: string, model?: string): Promise<{ ok: boolean; models?: string[]; ms?: number; error?: string }> {
  if (!/^(https:\/\/|http:\/\/(localhost|127\.0\.0\.1|\[::1\])[:/])/.test(baseURL || '')) return { ok: false, error: 'Use https://, or http:// only for localhost' };
  const t0 = Date.now();
  const base = baseURL.replace(/\/$/, '');
  const auth: Record<string, string> = key ? { authorization: 'Bearer ' + key } : {};
  const rejected = (status: number) => ({ ok: false, error: 'The key was rejected (' + status + ')' });
  const notApi = { ok: false, error: 'That URL does not serve the chat API. Check the Base URL (Open WebUI uses https://your-host/api).' };
  try {
    const r = await fetch(base + '/models', { headers: auth, signal: AbortSignal.timeout(8000) });
    if (r.status === 401 || r.status === 403) return rejected(r.status);
    const j = r.ok ? await r.json().catch(() => null) as { data?: { id: string }[] } | null : null;
    const models = (j?.data || []).map((m) => m.id).slice(0, 50);
    if (!model) {
      if (!r.ok) return { ok: false, error: 'Endpoint answered ' + r.status };
      return j ? { ok: true, models, ms: Date.now() - t0 } : notApi;
    }
    const c = await fetch(base + '/chat/completions', {
      method: 'POST',
      headers: { ...auth, 'content-type': 'application/json' },
      body: JSON.stringify({ model, messages: [{ role: 'user', content: 'Reply with OK.' }], max_tokens: 8 }),
      signal: AbortSignal.timeout(60000), // a local model may have to load first
    });
    if (c.status === 401 || c.status === 403) return rejected(c.status);
    if (c.status === 405) return notApi;
    const reply = await c.json().catch(() => null) as { choices?: unknown[]; error?: string | { message?: string }; detail?: unknown } | null;
    if (!c.ok) {
      const why = typeof reply?.error === 'string' ? reply.error : reply?.error?.message || (typeof reply?.detail === 'string' ? reply.detail : '');
      return { ok: false, error: `Model ${model} failed: endpoint answered ${c.status}${why ? ' · ' + why.slice(0, 160) : ''}` };
    }
    if (!Array.isArray(reply?.choices) || !reply.choices.length) return notApi;
    return { ok: true, models, ms: Date.now() - t0 };
  } catch (e) {
    return { ok: false, error: 'No answer from ' + baseURL + ' (' + String((e as Error).message).slice(0, 80) + ')' };
  }
}

export function buildRouter(): Router {
  const r = new Router();

  // ---------- app ----------
  r.get('/api/state', ({ sup }) => sup.appState());
  r.patch('/api/settings', ({ sup, body }) => {
    for (const k of ['theme', 'accent', 'reduceMotion', 'followSystem'] as const) if (body[k] !== undefined) (sup.settings as any)[k] = body[k];
    sup.saveSettings();
    return sup.appState();
  });

  // ---------- onboarding ----------
  r.get('/api/onboarding/detect', async ({ query }) => findOpencode(query.get('binary') || 'opencode'));
  r.post('/api/onboarding/scan', async ({ sup, body }) => {
    try {
      const sessions = body.mode === 'remote'
        ? await new OpencodeClient(body.url, body.username || 'opencode', body.password || '').listSessions({ limit: 2000 })
        : await local.sessions(body.binary);
      return { sessions: sessions.filter((s) => !s.parentID).map((s) => ({ id: s.id, title: s.title, dir: s.location?.directory || '', updated: s.time.updated, created: s.time.created })) };
    } catch (e) { return { sessions: [], error: String((e as Error).message || e) }; }
    void sup;
  });
  r.post('/api/onboarding/test-remote', ({ sup, body }) => sup.testRemote(body.url, body.username, body.password));
  r.post('/api/onboarding/test-provider', async ({ body }) => {
    if (body.kind === 'ollama') return testEndpoint('http://localhost:11434/v1');
    if (body.kind === 'custom') return testEndpoint(body.url, body.key, body.model);
    if (body.kind === 'anthropic') {
      if ((body.key || '').trim().length < 8) return { ok: false, error: 'That doesn’t look like an Anthropic key.' };
      try {
        const res = await fetch('https://api.anthropic.com/v1/models', { headers: { 'x-api-key': body.key, 'anthropic-version': '2023-06-01' }, signal: AbortSignal.timeout(8000) });
        return res.ok ? { ok: true } : { ok: false, error: 'The key was rejected (' + res.status + ')' };
      } catch { return { ok: false, error: 'Could not reach api.anthropic.com' }; }
    }
    if (body.kind === 'openrouter') return testEndpoint('https://openrouter.ai/api/v1', body.key);
    return { ok: true };
  });
  r.post('/api/onboarding/test-web', async ({ body }) => {
    if (body.mode === 'off') return { ok: true, note: 'Search is off' };
    if (body.mode === 'mcp') return (body.key || '').trim().length >= 8 ? { ok: true, note: 'Key saved to the keychain on finish; the MCP server starts with the workspace.' } : { ok: false, error: 'The ' + body.provider + ' key looks too short.' };
    if (body.mode === 'searxng') {
      try { const res = await fetch(body.url.replace(/\/$/, '') + '/search?q=opencode&format=json', { signal: AbortSignal.timeout(6000) }); return res.ok ? { ok: true, note: 'SearXNG answered' } : { ok: false, error: 'SearXNG answered ' + res.status }; } catch { return { ok: false, error: 'No answer from that URL.' }; }
    }
    return { ok: true, note: 'opencode built-in search (Exa) needs no key' };
  });
  r.get('/api/onboarding/templates', () => Object.fromEntries(Object.keys(AGENTS_TEMPLATES).map((k) => [k, AGENTS_TEMPLATES[k]('{name}')])));
  r.post('/api/onboarding/finish', async ({ sup, body }) => {
    const rt = await sup.finishOnboarding({ ...body, importSessions: [] });
    if (Array.isArray(body.subscriptions)) {
      for (const sid of body.subscriptions as SubscriptionId[]) {
        if (SUBSCRIPTIONS[sid]) await connectSubscription(rt, sid).catch(() => {});
      }
    }
    // "Claude subscription" as the model choice: new chats go to Claude Code once it is connected here.
    if (body.defaultAgent && rt.ws.agents.some((a) => a.id === body.defaultAgent && a.on)) { rt.ws.defaultAgent = body.defaultAgent; rt.save(); rt.changed(); }
    let imported = { chats: 0, projects: 0, failed: [] as string[] };
    const ids: string[] = body.importIds || [];
    if (ids.length) {
      if (rt.ws.engine.mode === 'remote') {
        const c = await rt.client();
        const all = await c.listSessions({ limit: 2000 });
        const r2 = rt.importSessions(all.filter((s) => ids.includes(s.id)));
        imported = { ...r2, failed: [] };
      } else {
        imported = await copySessions(await local.open(body.engine?.binary), rt, ids);
        local.close();
      }
    }
    if (body.skippedIds?.length) { rt.state.skippedSessions.push(...body.skippedIds); rt.save(); }
    return { workspace: rt.id, imported };
  });
  r.post('/api/onboarding/demo', async ({ sup }) => { await sup.loadDemo(); return sup.appState(); });

  // ---------- workspaces ----------
  r.post('/api/workspaces', ({ sup, body }) => {
    if (body.kind === 'org') err('Organizations are joined with SSO');
    const rt = sup.createWorkspace(body.name, body.kind === 'personal' ? 'personal' : 'team');
    sup.activate(rt.id);
    return { id: rt.id, port: 0 };
  });
  r.post('/api/workspaces/join-org', ({ sup, body }) => {
    const rt = sup.joinOrganization(body.name || 'BSC AI Factory', { issuer: body.issuer || 'https://login.[ORG-HOST]/realms/caim', subject: body.email || 'you@[ORG-HOST]', idp: body.idp || 'MyAccessID' }, { auditUrl: body.auditUrl, auditEveryMinutes: body.auditEveryMinutes, logContent: body.logContent, gateway: body.gateway });
    sup.activate(rt.id);
    return { id: rt.id };
  });
  r.post('/api/ws/:id/audit/upload', async ({ sup, params }) => ({ sent: await sup.rt(params.id).uploadAudit(Date.now(), true) }));
  r.post('/api/workspaces/:id/activate', ({ sup, params }) => { sup.activate(params.id); return sup.appState(); });
  r.del('/api/workspaces/:id', async ({ sup, params }) => { await sup.deleteWorkspace(params.id); return sup.appState(); });

  // ---------- subscriptions: opencode, Claude Code, Grok (R-06, R-39) ----------
  r.get('/api/subscriptions', ({ query }) => detectedRows(query.get('force') === '1'));
  r.get('/api/ws/:id/subscriptions', ({ sup, params, query }) => subscriptionStatus(sup.rt(params.id), query.get('force') === '1'));
  r.post('/api/ws/:id/subscriptions/:sid/connect', ({ sup, params }) => {
    const rt = sup.rt(params.id);
    const sid = params.sid as SubscriptionId;
    if (!SUBSCRIPTIONS[sid]) err('Unknown subscription', 404);
    return connectSubscription(rt, sid);
  });
  r.post('/api/ws/:id/subscriptions/:sid/disconnect', ({ sup, params }) => {
    const rt = sup.rt(params.id);
    const sid = params.sid as SubscriptionId;
    if (!SUBSCRIPTIONS[sid]) err('Unknown subscription', 404);
    return disconnectSubscription(rt, sid);
  });

  // Signing in again from a chat, when the workspace's copy of the login has expired.
  const subOf = (raw: string) => { const id = raw as SubscriptionId; if (!SUBSCRIPTIONS[id]) err('Unknown subscription', 404); return id; };
  r.get('/api/ws/:id/subscriptions/:sid/signin', async ({ sup, params }) => {
    const rt = sup.rt(params.id), id = subOf(params.sid);
    const here = (await detectedRows()).find((x) => x.id === id);
    return { ...signInState(rt, id), browser: canSignIn(id), computer: here?.signedIn ? { account: here.account || '' } : null };
  });
  r.post('/api/ws/:id/subscriptions/:sid/signin', ({ sup, params }) => {
    const rt = sup.rt(params.id), id = subOf(params.sid);
    return startSignIn(rt, id, (ok) => {
      if (!ok) return;
      // The agent restarts on its next message with the new login.
      void rt.stopAgents(rt.ws.agents.filter((a) => a.subscription === id).map((a) => a.id)).catch(() => {});
      rt.audit('subscription.signin', SUBSCRIPTIONS[id].name + ' · signed in again from a chat');
      sup.emit({ type: 'toast', text: `Signed in to ${SUBSCRIPTIONS[id].name}` });
    });
  });
  r.post('/api/ws/:id/subscriptions/:sid/signin/code', ({ sup, params, body }) => signInCode(sup.rt(params.id), subOf(params.sid), String(body.code || '')));
  r.post('/api/ws/:id/subscriptions/:sid/signin/cancel', ({ sup, params }) => cancelSignIn(sup.rt(params.id), subOf(params.sid)));

  // ---------- Graphify memory (R-23) ----------
  r.get('/api/ws/:id/graphify/status', async ({ sup, params }) => ({ ...graphifyStatus(sup.rt(params.id)), version: await graphifyVersion() }));
  r.post('/api/ws/:id/graphify/build', ({ sup, params }) => rebuildMemoryGraph(sup.rt(params.id)));
  r.post('/api/graphify/install', () => installGraphify());
  r.get('/api/ws/:id/graphify/graph', ({ sup, params, query }) => getMemoryGraph(sup.rt(params.id), query.get('rebuild') === '1'));
  r.get('/api/ws/:id/graphify/query', ({ sup, params, query }) => queryMemoryGraph(sup.rt(params.id), query.get('q') || ''));
  r.get('/api/ws/:id/graphify/node', ({ sup, params, query }) => memoryNodeDetail(sup.rt(params.id), query.get('id') || '') || err('That node is not in the graph', 404));
  r.post('/api/ws/:id/graphify/review/:rid', ({ sup, params, body }) => {
    const status = body.status === 'kept' ? 'kept' : body.status === 'dropped' ? 'dropped' : 'pending';
    return { review: reviewLink(sup.rt(params.id), params.rid, status) };
  });

  // ---------- workspace ----------
  r.get('/api/ws/:id/snapshot', ({ sup, params }) => sup.rt(params.id).snapshot());
  r.patch('/api/ws/:id', ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    const ws = rt.ws;
    const allowed: (keyof Workspace)[] = ['name', 'accent', 'instructions', 'defaultModel', 'defaultProject', 'memory', 'memoryMcpCommand', 'graphifyFolders', 'graphifyWatch', 'vaults', 'suggestMemories', 'designSystem', 'openDesign', 'useMemory', 'memoryRecall', 'leanCtx', 'web', 'agents', 'defaultAgent', 'acpAskFirst', 'members', 'skills', 'visibility', 'publicLinks', 'keepChats', 'routineAskFirst', 'routineIfAsleep', 'engine'];
    const lockedInOrg: (keyof Workspace)[] = ['name', 'web', 'agents', 'defaultAgent', 'engine', 'keepChats', 'publicLinks'];
    for (const k of Object.keys(body) as (keyof Workspace)[]) {
      if (!allowed.includes(k)) err('Unknown setting ' + String(k));
      if (ws.kind === 'org' && lockedInOrg.includes(k)) {
        // Per-chat toggles stay usable; only the workspace-level choice is locked (R-21, R-36).
        if (k === 'web' && body.web && body.web.mode === ws.web.mode && body.web.searxngUrl === ws.web.searxngUrl) { /* ask/showSources changes are fine */ }
        else { rt.audit('policy.block', 'change ' + String(k), 'workspaceai'); err('Locked by org policy', 403); }
      }
      if (k === 'defaultModel' && ws.kind === 'org' && rt.policy && !rt.policy.allowedModels.includes(body[k])) err('That model is not allowed by your organization', 403);
      if (k === 'openDesign' && body.openDesign?.engine && !['open-design', 'builtin'].includes(body.openDesign.engine)) err('Artifact engine must be open-design or builtin');
      if (k === 'vaults') { body.vaults = cleanVaults(body.vaults); pruneAttachments(rt, body.vaults); }
      (ws as any)[k] = k === 'web' || k === 'engine' || k === 'openDesign' || k === 'leanCtx' ? { ...(ws as any)[k], ...body[k] } : body[k];
      if (k === 'name') ws.initials = sup.initialsFor(body.name);
    }
    rt.audit('settings.update', Object.keys(body).join(', '));
    rt.changed();
    sup.saveSettings();
    // Open Design's MCP tools are written into opencode.json, so a change there restarts the engine too.
    const restart = ['web', 'mcp', 'providers', 'engine', 'defaultModel', 'memory', 'useMemory', 'openDesign', 'leanCtx'].some((k) => k in body);
    // A stopped engine reads the file when it starts; ACP agents read it for every new session.
    if (restart && rt.status.state === 'running') rt.restartEngine().catch(() => {});
    else if (restart) rt.writeConfig();
    return rt.snapshot();
  });
  r.get('/api/ws/:id/opencode-json', ({ sup, params }) => { const rt = sup.rt(params.id); return { path: rt.paths.opencodeJson, json: buildOpencodeJson(rt.ws, rt.policy, {}).json, written: readText(rt.paths.opencodeJson) }; });
  r.get('/api/ws/:id/web-preview', ({ sup, params }) => ({ text: webConfigPreview(sup.rt(params.id).ws) }));

  // engine
  r.get('/api/ws/:id/engine', ({ sup, params }) => sup.rt(params.id).status);
  r.post('/api/ws/:id/engine/start', async ({ sup, params }) => { const rt = sup.rt(params.id); await rt.client(); return rt.status; });
  // Pin this workspace to an exact opencode release (installed unmodified from npm), or go back to the system one.
  r.post('/api/ws/:id/engine/version', async ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    sup.assertAllowed(rt, 'engine');
    const version = String(body.version || 'system').trim().replace(/^v(?=\d)/, '') || 'system';
    if (version !== 'system' && !existsSync(pinnedOpencodeBin(version))) {
      if (!body.install) err(`opencode ${version} is not installed yet`, 409);
      await installOpencode(version);
    }
    rt.ws.engine = { ...rt.ws.engine, version };
    rt.audit('engine.version', version === 'system' ? 'opencode from PATH' : 'pinned to opencode ' + version);
    rt.changed();
    if (rt.status.state === 'running') await rt.restartEngine();
    return { version, status: rt.status };
  });
  r.post('/api/ws/:id/engine/restart', async ({ sup, params }) => { const rt = sup.rt(params.id); await rt.restartEngine(); return rt.status; });
  r.post('/api/ws/:id/engine/stop', async ({ sup, params }) => { const rt = sup.rt(params.id); await rt.stopEngine(); return rt.status; });
  r.post('/api/ws/:id/engine/mode', async ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    sup.assertAllowed(rt, 'engine');
    if (body.mode === 'remote') {
      const t = await sup.testRemote(body.url, body.username, body.password ?? keychain.get(rt.id, 'OPENCODE_SERVER_PASSWORD') ?? '');
      if (!t.ok) err(t.error || 'Connection test failed');
      if (body.password != null) keychain.set(rt.id, 'OPENCODE_SERVER_PASSWORD', body.password);
      rt.ws.engine = { ...rt.ws.engine, mode: 'remote', url: body.url, username: body.username || 'opencode', passwordSecret: 'OPENCODE_SERVER_PASSWORD' };
    } else rt.ws.engine = { ...rt.ws.engine, mode: 'managed' };
    rt.audit('engine.mode', rt.ws.engine.mode + (rt.ws.engine.mode === 'remote' ? ' ' + rt.ws.engine.url : ''));
    rt.save();
    await rt.restartEngine();
    return rt.snapshot();
  });
  r.get('/api/ws/:id/engine/logs', ({ sup, params }) => ({ text: readText(join(sup.rt(params.id).paths.logs, 'opencode.log')).split('\n').slice(-500).join('\n') }));
  r.get('/api/ws/:id/models', async ({ sup, params }) => {
    const rt = sup.rt(params.id);
    // opencode lists every provider it knows, with or without a key, so only providers this workspace has
    // actually connected count. Their models come from opencode itself (complete, with reasoning variants) plus
    // any the workspace configured by hand.
    const allowed = rt.ws.kind === 'org' && rt.policy ? rt.policy.allowedModels : null;
    const engine = await rt.engineModels();
    const groups: ModelGroup[] = [];
    for (const p of rt.connectedProviders()) {
      const seen = new Map<string, ModelOption>();
      for (const m of engine) if (m.providerID === p.id) seen.set(m.id, { id: p.id + '/' + m.id, name: m.name, ...(m.efforts ? { efforts: m.efforts } : {}) });
      for (const m of p.models) if (!seen.has(m.id)) seen.set(m.id, { id: p.id + '/' + m.id, name: m.name || m.id });
      const models = Array.from(seen.values()).filter((m) => !allowed || allowed.includes(m.id));
      if (models.length) groups.push({ provider: p.id, name: p.name, note: p.fromOpencode ? 'your opencode login' : p.keyEnv ? 'API key' : p.note || 'no key needed', models });
    }
    // ACP agents (Claude Code, Grok, …) bring their own models from the subscription they run on.
    const agents: Record<string, { models: ModelOption[]; current: string }> = {};
    for (const a of rt.ws.agents) {
      if (!a.on || a.id === 'opencode') continue;
      agents[a.id] = rt.agentModels(a.id);
      rt.refreshAgentModels(a.id);
    }
    return { models: groups.flatMap((g) => g.models.map((m) => m.id)), groups, agents };
  });
  r.get('/api/ws/:id/scan-sessions', async ({ sup, params }) => ({ sessions: await sup.rt(params.id).scanSessions() }));
  r.post('/api/ws/:id/import-sessions', async ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    const all = await rt.scanSessions();
    return rt.importSessions(all.filter((s) => !body.ids || body.ids.includes(s.id)));
  });

  // providers (R-20) and secrets (R-34)
  r.post('/api/ws/:id/providers', ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    sup.assertAllowed(rt, 'providers');
    const id = String(body.id || '').trim().toLowerCase().replace(/[^a-z0-9-]/g, '-');
    if (!id) err('Provider ID is required');
    if (rt.ws.providers.some((p) => p.id === id)) err('A provider with that ID already exists');
    const keyEnv = String(body.env || id.toUpperCase().replace(/-/g, '_') + '_API_KEY').toUpperCase().replace(/[^A-Z0-9_]/g, '_');
    const p: Provider = { id, name: body.name || id, note: (body.pkg || '@ai-sdk/openai-compatible') + ' · ' + (body.url || ''), npm: body.pkg || '@ai-sdk/openai-compatible', baseURL: body.url, keyEnv, models: (Array.isArray(body.models) ? body.models : [body.model || 'model']).map((m: string | { id: string }) => (typeof m === 'string' ? { id: m } : m)), preset: 'custom' };
    if (body.key) keychain.set(rt.id, keyEnv, body.key);
    rt.ws.providers.push(p);
    if (!rt.ws.defaultModel) rt.ws.defaultModel = id + '/' + p.models[0].id;
    rt.audit('provider.add', id + ' → opencode.json');
    rt.writeConfig();
    rt.changed();
    if (rt.status.state === 'running') rt.restartEngine().catch(() => {});
    return { provider: p, opencodeJson: readText(rt.paths.opencodeJson) };
  });
  r.post('/api/ws/:id/providers/test', async ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    const p = rt.ws.providers.find((x) => x.id === body.id);
    const url = body.url || p?.baseURL;
    const key = body.key || (p?.keyEnv ? keychain.get(rt.id, p.keyEnv) : null) || undefined;
    if (!url) return { ok: true, note: 'opencode resolves this provider itself' };
    // Only OpenAI-style SDKs have this chat route; a proxy for another SDK can only be listed.
    return testEndpoint(url, key, /openai/.test(body.pkg || p?.npm || '') ? body.model || p?.models[0]?.id : undefined);
  });
  r.patch('/api/ws/:id/providers/:pid', ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    sup.assertAllowed(rt, 'providers');
    const p = rt.ws.providers.find((x) => x.id === params.pid) || err('Provider not found', 404);
    if (p.locked) err('This provider is managed by org policy', 403);
    const models: string[] | null = Array.isArray(body.models) ? body.models.map((m: string) => String(m).trim()).filter(Boolean) : null;
    if (models && !models.length) err('At least one model is required');
    if (body.isDefault) { for (const x of rt.ws.providers) x.isDefault = x.id === p.id; rt.ws.defaultModel = p.id + '/' + p.models[0].id; }
    if (body.name) p.name = String(body.name);
    if (body.url) { p.baseURL = String(body.url); p.note = (p.npm || '') + ' · ' + p.baseURL; }
    if (models) {
      p.models = models.map((m) => p.models.find((x) => x.id === m) || { id: m });
      if (rt.ws.defaultModel?.startsWith(p.id + '/') && !models.includes(rt.ws.defaultModel.slice(p.id.length + 1))) rt.ws.defaultModel = p.id + '/' + models[0];
    }
    if (body.key && p.keyEnv) keychain.set(rt.id, p.keyEnv, body.key);
    if (body.url || models || body.key) rt.audit('provider.edit', p.id + ' → opencode.json');
    rt.writeConfig(); rt.changed();
    if ((body.url || models || body.key) && rt.status.state === 'running') rt.restartEngine().catch(() => {});
    return { ok: true };
  });
  r.del('/api/ws/:id/providers/:pid', ({ sup, params }) => {
    const rt = sup.rt(params.id);
    sup.assertAllowed(rt, 'providers');
    const p = rt.ws.providers.find((x) => x.id === params.pid) || err('Provider not found', 404);
    rt.ws.providers = rt.ws.providers.filter((x) => x.id !== p.id);
    if (p.keyEnv && !rt.ws.providers.some((x) => x.keyEnv === p.keyEnv)) keychain.delete(rt.id, p.keyEnv);
    if (rt.ws.defaultModel.startsWith(p.id + '/')) rt.ws.defaultModel = rt.ws.providers[0] ? rt.ws.providers[0].id + '/' + rt.ws.providers[0].models[0].id : '';
    if (rt.ws.providers[0] && !rt.ws.providers.some((x) => x.isDefault)) rt.ws.providers[0].isDefault = true;
    rt.audit('provider.remove', p.id);
    rt.writeConfig(); rt.changed();
    return { ok: true };
  });
  r.get('/api/ws/:id/secrets', ({ sup, params }) => ({ names: keychain.list(sup.rt(params.id).id), backend: keychain.backendName }));
  r.put('/api/ws/:id/secrets/:name', ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    if (rt.ws.kind === 'org' && rt.policy?.blockPersonalKeys && !body.org) err('Personal keys can’t be used in this organization', 403);
    keychain.set(rt.id, params.name.toUpperCase().replace(/[^A-Z0-9_]/g, '_'), String(body.value || ''));
    rt.audit('secret.set', params.name);
    return { ok: true };
  });
  r.del('/api/ws/:id/secrets/:name', ({ sup, params }) => { keychain.delete(sup.rt(params.id).id, params.name); return { ok: true }; });

  // connectors & MCP
  r.post('/api/ws/:id/mcp', ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    if (rt.ws.kind === 'org' && body.locked) err('Locked by org policy', 403);
    const raw = String(body.command || body.name || '').trim();
    if (!raw) err('Name or command required');
    const isUrl = /^https?:\/\//.test(raw);
    const m: McpServer = { id: (body.id || raw.split(/[\s/]+/).filter(Boolean).pop() || 'mcp').toLowerCase().replace(/[^a-z0-9-]/g, '-').slice(0, 40) + '-' + uid('m').slice(-4), name: body.name || raw, kind: isUrl ? 'MCP · http' : 'MCP · stdio', type: isUrl ? 'remote' : 'local', ...(isUrl ? { url: raw } : { command: raw.split(/\s+/) }), on: true };
    rt.ws.mcp.push(m);
    rt.audit('mcp.add', m.name + ' → opencode.json');
    rt.writeConfig(); rt.changed();
    return m;
  });
  r.patch('/api/ws/:id/mcp/:mid', ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    const m = rt.ws.mcp.find((x) => x.id === params.mid) || err('Connector not found', 404);
    if (m.locked) err('Required by your organization', 403);
    if (body.on !== undefined) m.on = !!body.on;
    rt.audit('mcp.toggle', m.name + (m.on ? ' on' : ' off'));
    rt.writeConfig(); rt.changed();
    return m;
  });
  r.del('/api/ws/:id/mcp/:mid', ({ sup, params }) => {
    const rt = sup.rt(params.id);
    const m = rt.ws.mcp.find((x) => x.id === params.mid) || err('Connector not found', 404);
    if (m.locked) err('Required by your organization', 403);
    rt.ws.mcp = rt.ws.mcp.filter((x) => x.id !== m.id);
    rt.writeConfig(); rt.changed();
    return { ok: true };
  });

  // ---------- projects ----------
  r.post('/api/ws/:id/projects', ({ sup, params, body }) => sup.rt(params.id).createProject(body.name, body.parent ?? null, body.folder));
  r.patch('/api/ws/:id/projects/:pid', ({ sup, params, body }) => sup.rt(params.id).updateProject(params.pid, body));
  r.post('/api/ws/:id/projects/:pid/archive', ({ sup, params }) => ({ hiddenSubprojects: sup.rt(params.id).archiveProject(params.pid) }));
  r.post('/api/ws/:id/projects/:pid/restore', ({ sup, params }) => { sup.rt(params.id).restoreProject(params.pid); return { ok: true }; });
  r.del('/api/ws/:id/projects/:pid', async ({ sup, params }) => { await sup.rt(params.id).deleteProject(params.pid); return { ok: true }; });
  r.get('/api/ws/:id/projects/:pid/agents', ({ sup, params }) => { const rt = sup.rt(params.id); return { file: rt.agentsFile(params.pid), text: rt.readAgents(params.pid) }; });
  r.put('/api/ws/:id/projects/:pid/agents', ({ sup, params, body }) => { sup.rt(params.id).writeAgents(params.pid, String(body.text ?? '')); return { ok: true }; });
  r.get('/api/ws/:id/projects/:pid/layers', ({ sup, params }) => ({ layers: sup.rt(params.id).instructionLayers(params.pid) }));
  // Export a workspace: config, memory, state, opencode sessions, audit log, and each project's AGENTS.md + .harness.
  // Secrets live in the keychain and are never part of the export.
  r.get('/api/ws/:id/export', async ({ sup, params, res }) => {
    const rt = sup.rt(params.id);
    const { existsSync, mkdtempSync, mkdirSync, symlinkSync, rmSync } = await import('node:fs');
    const { tmpdir } = await import('node:os');
    const { join } = await import('node:path');
    const { spawn } = await import('node:child_process');
    const { expandHome } = await import('./paths.ts');
    // Stage symlinks so the archive has one tidy layout; tar -h follows them.
    const stage = mkdtempSync(join(tmpdir(), 'harness-export-'));
    const root = join(stage, rt.ws.slug);
    mkdirSync(root);
    for (const rel of ['config', 'memory', 'data/state.json', 'data/audit.jsonl', 'data/opencode/xdg-data']) {
      if (!existsSync(join(rt.paths.root, rel))) continue;
      mkdirSync(join(root, rel, '..'), { recursive: true });
      symlinkSync(join(rt.paths.root, rel), join(root, rel));
    }
    for (const p of rt.state.projects) {
      const dir = expandHome(p.folder);
      for (const item of ['AGENTS.md', '.harness']) {
        if (!existsSync(join(dir, item))) continue;
        mkdirSync(join(root, 'projects', p.id), { recursive: true });
        symlinkSync(join(dir, item), join(root, 'projects', p.id, item));
      }
    }
    rt.audit('workspace.export', rt.ws.slug + '.tar.gz');
    res.writeHead(200, { 'content-type': 'application/gzip', 'content-disposition': `attachment; filename="${rt.ws.slug}.tar.gz"` });
    const tar = spawn('tar', ['-czhf', '-', '-C', stage, rt.ws.slug], { stdio: ['ignore', 'pipe', 'ignore'] });
    tar.stdout.pipe(res);
    await new Promise((r) => tar.on('close', r));
    rmSync(stage, { recursive: true, force: true });
  });
  r.get('/api/ws/:id/archived', ({ sup, params }) => ({ projects: sup.rt(params.id).state.projects.filter((p) => p.archived && !sup.rt(params.id).state.projects.find((q) => q.id === p.parent)?.archived) }));

  // ---------- chats ----------
  r.post('/api/ws/:id/chats', async ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    sup.sendAllowed(rt);
    const chat = await rt.createChat({ project: body.project, title: body.title || (body.text ? String(body.text).slice(0, 60) : 'New chat'), agent: body.agent, model: body.model, effort: body.effort ? String(body.effort) : undefined, web: body.web ?? false, instructions: body.instructions });
    if (body.text) await rt.send(chat.id, body.text);
    return chat;
  });
  r.get('/api/ws/:id/chats/:cid/messages', async ({ sup, params }) => ({ messages: await sup.rt(params.id).messages(params.cid), streaming: sup.rt(params.id).isStreaming(params.cid) }));
  r.post('/api/ws/:id/chats/:cid/send', async ({ sup, params, body }) => { const rt = sup.rt(params.id); sup.sendAllowed(rt); await rt.send(params.cid, String(body.text || '')); return { ok: true }; });
  r.get('/api/ws/:id/chats/:cid/commands', async ({ sup, params }) => ({ commands: await sup.rt(params.id).chatCommandList(params.cid) }));
  r.post('/api/ws/:id/chats/:cid/command', async ({ sup, params, body }) => { const rt = sup.rt(params.id); sup.sendAllowed(rt); return rt.runCommand(params.cid, String(body.text || '')); });
  r.post('/api/ws/:id/chats/:cid/stop', async ({ sup, params }) => { await sup.rt(params.id).stop(params.cid); return { ok: true }; });
  r.post('/api/ws/:id/chats/:cid/fork', async ({ sup, params }) => sup.rt(params.id).forkChat(params.cid));
  r.post('/api/ws/:id/chats/:cid/move', ({ sup, params, body }) => ({ movedArtifacts: sup.rt(params.id).moveChat(params.cid, body.project) }));
  r.patch('/api/ws/:id/chats/:cid', async ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    if (body.title !== undefined) await rt.renameChat(params.cid, body.title);
    if (body.yolo !== undefined) await rt.setChatYolo(params.cid, !!body.yolo);
    const { title: _t, yolo: _y, ...rest } = body;
    if (Object.keys(rest).length) rt.updateChat(params.cid, rest);
    return rt.chat(params.cid);
  });
  r.del('/api/ws/:id/chats/:cid', async ({ sup, params }) => { await sup.rt(params.id).deleteChat(params.cid); return { ok: true }; });
  r.get('/api/ws/:id/attach', ({ sup, params }) => ({ command: sup.rt(params.id).attachCommand() }));
  r.get('/api/ws/:id/chats/:cid/attach', ({ sup, params }) => ({ command: sup.rt(params.id).attachCommand(params.cid) }));
  r.get('/api/ws/:id/chats/:cid/permissions', async ({ sup, params }) => ({ permissions: await sup.rt(params.id).pendingPermissions(params.cid) }));
  r.post('/api/ws/:id/chats/:cid/permission/:rid', async ({ sup, params, body }) => {
    const rt = sup.rt(params.id);
    const chat = rt.chat(params.cid) || err('Chat not found', 404);
    const decision = (['once', 'always', 'reject'].includes(String(body.decision)) ? body.decision : 'once') as 'once' | 'always' | 'reject';
    const result = await rt.replyPermission(chat.id, params.rid, decision);
    if (result === 'expired') err('That permission request was already answered or has expired', 409);
    rt.audit('permission.' + decision, chat.title);
    return { ok: true };
  });

  // ---------- memory facts ----------
  r.post('/api/ws/:id/facts', ({ sup, params, body }) => sup.rt(params.id).addFact(body.text, body.scope || 'ws', body.status || 'approved', body.source));
  r.patch('/api/ws/:id/facts/:fid', ({ sup, params, body }) => { sup.rt(params.id).updateFact(params.fid, body); return { ok: true }; });
  r.del('/api/ws/:id/facts/:fid', ({ sup, params }) => { sup.rt(params.id).removeFact(params.fid); return { ok: true }; });
  r.del('/api/ws/:id/facts', ({ sup, params }) => { sup.rt(params.id).clearFacts(); return { ok: true }; });
  r.post('/api/ws/:id/memory-changed', ({ sup, params }) => { const rt = sup.rt(params.id); rt.audit('memory.suggest', 'from agent', 'agent'); rt.changed(); return { ok: true }; });

  // ---------- org policy ----------
  r.post('/api/ws/:id/policy/refresh', ({ sup, params, body }) => sup.refreshPolicy(params.id, !!body.tamper));
  r.get('/api/ws/:id/audit', ({ sup, params, query }) => ({ events: sup.rt(params.id).auditLog(Number(query.get('limit') || 300)) }));

  r.get('/api/web-providers', () => WEB_PROVIDERS);
  registerScreens(r);
  registerPipeline(r);
  return r;
}
