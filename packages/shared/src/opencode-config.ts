// Builds the opencode.json a workspace's server reads. Keys never enter the file: every secret is an
// `{env:NAME}` reference and the supervisor injects the value from the keychain into that workspace's
// process environment (R-20, R-34). Org workspaces are written from policy only (R-07, R-36).
import type { Workspace, Provider, McpServer, OrgPolicy, WebProvider } from './types.ts';

export const WEB_PROVIDERS: Record<WebProvider, { pkg: string; env: string }> = {
  Tavily: { pkg: 'tavily-mcp', env: 'TAVILY_API_KEY' },
  Brave: { pkg: '@brave/brave-search-mcp-server', env: 'BRAVE_API_KEY' },
  Firecrawl: { pkg: 'firecrawl-mcp', env: 'FIRECRAWL_API_KEY' },
};

/** Providers opencode resolves itself; only custom ones need a `provider` block. */
export const NATIVE_PROVIDERS = new Set(['anthropic', 'openrouter', 'openai', 'opencode', 'google', 'mistral']);

export interface OpencodeJson {
  $schema?: string;
  provider?: Record<string, unknown>;
  model?: string;
  mcp?: Record<string, unknown>;
  permission?: Record<string, string>;
  instructions?: string[];
  [k: string]: unknown;
}

export function providerBlock(p: Provider): Record<string, unknown> | null {
  const models: Record<string, unknown> = {};
  for (const m of p.models) {
    const entry: Record<string, unknown> = { name: m.name || m.id };
    if (m.context || m.output) entry.limit = { context: m.context ?? 128000, output: m.output ?? 8192 };
    models[m.id] = entry;
  }
  if (NATIVE_PROVIDERS.has(p.id)) {
    // Native provider: opencode already knows its SDK and models; point it at the key, and at a proxy if one is set.
    const options: Record<string, unknown> = {};
    if (p.baseURL) options.baseURL = p.baseURL;
    if (p.keyEnv) options.apiKey = `{env:${p.keyEnv}}`;
    return Object.keys(options).length ? { options } : null;
  }
  const options: Record<string, unknown> = {};
  if (p.baseURL) options.baseURL = p.baseURL;
  if (p.keyEnv) options.apiKey = `{env:${p.keyEnv}}`;
  return { npm: p.npm || '@ai-sdk/openai-compatible', name: p.name, options, models };
}

export function mcpBlock(m: McpServer): Record<string, unknown> | null {
  if (m.type === 'connector') return null;
  const environment: Record<string, string> = {};
  for (const [name] of Object.entries(m.env || {})) environment[name] = `{env:${name}}`;
  if (m.type === 'remote') return { type: 'remote', url: m.url, enabled: m.on, ...(Object.keys(environment).length ? { headers: environment } : {}) };
  return { type: 'local', command: m.command || [], enabled: m.on, ...(Object.keys(environment).length ? { environment } : {}) };
}

export function webSearchConfig(ws: Workspace): { mcp: Record<string, unknown>; permission: Record<string, string>; env: Record<string, string> } {
  const w = ws.web;
  const level = w.ask ? 'ask' : 'allow';
  const permission: Record<string, string> = { webfetch: w.fetch ? level : 'deny' };
  const mcp: Record<string, unknown> = {};
  const env: Record<string, string> = {};
  if (w.mode === 'off') permission.websearch = 'deny';
  else if (w.mode === 'exa') { permission.websearch = level; env.OPENCODE_ENABLE_EXA = 'true'; }
  else if (w.mode === 'mcp') {
    const p = WEB_PROVIDERS[w.provider];
    const id = w.provider.toLowerCase();
    mcp[id] = { type: 'local', command: ['npx', '-y', p.pkg], environment: { [p.env]: `{env:${p.env}}` }, enabled: true };
    permission.websearch = 'deny';
    permission[id + '_*'] = level;
  } else if (w.mode === 'searxng') {
    mcp.searxng = { type: 'local', command: ['npx', '-y', 'mcp-searxng'], environment: { SEARXNG_URL: w.searxngUrl }, enabled: true };
    permission.websearch = 'deny';
    permission['searxng_*'] = level;
  }
  return { mcp, permission, env };
}

/** Effective providers: org workspaces only ever get what the policy allows (R-07). */
export function effectiveProviders(ws: Workspace, policy: OrgPolicy | null): Provider[] {
  if (ws.kind === 'org' && policy) {
    // With an org gateway (LiteLLM, MIT edition) every allowed model is reached through that one URL, with the
    // gateway's key; the gateway maps model names to backends (R-40).
    const gw = policy.gateway?.replace(/\/+$/, '');
    const gwKey = policy.allowedProviders.find((p) => p.baseURL?.replace(/\/+$/, '') === gw)?.keyEnv || policy.allowedProviders[0]?.keyEnv;
    return policy.allowedProviders.map((p) => (gw ? { ...p, npm: '@ai-sdk/openai-compatible', baseURL: gw, keyEnv: gwKey, locked: true } : { ...p, locked: true }));
  }
  return ws.providers;
}

export function buildOpencodeJson(ws: Workspace, policy: OrgPolicy | null, extraMcp: Record<string, unknown> = {}): { json: OpencodeJson; env: Record<string, string> } {
  const json: OpencodeJson = { $schema: 'https://opencode.ai/config.json' };
  const providers: Record<string, unknown> = {};
  for (const p of effectiveProviders(ws, policy)) {
    if (p.fromOpencode) continue;
    const b = providerBlock(p);
    if (b) providers[p.id] = b;
  }
  if (Object.keys(providers).length) json.provider = providers;
  if (ws.defaultModel) json.model = ws.defaultModel;
  const web = webSearchConfig(policy && ws.kind === 'org' ? { ...ws, web: { ...ws.web, mode: policy.web.mode, searxngUrl: policy.web.searxngUrl } } : ws);
  const mcp: Record<string, unknown> = { ...web.mcp, ...extraMcp };
  const servers = ws.kind === 'org' && policy ? [...policy.lockedMcp, ...ws.mcp.filter((m) => !policy.lockedMcp.some((l) => l.id === m.id))] : ws.mcp;
  for (const m of servers) { const b = mcpBlock(m); if (b) mcp[m.id] = b; }
  if (Object.keys(mcp).length) json.mcp = mcp;
  json.permission = { ...web.permission, ...(ws.acpAskFirst ? { edit: 'ask', bash: 'ask', 'harness-vault_vault_write': 'ask', 'harness-vault_vault_edit': 'ask' } : {}) };
  return { json, env: web.env };
}

/** Every env var name the config references; the supervisor fills these from the keychain. */
export function referencedEnv(json: OpencodeJson): string[] {
  const out = new Set<string>();
  const s = JSON.stringify(json);
  for (const m of s.matchAll(/\{env:([A-Z0-9_]+)\}/g)) out.add(m[1]);
  return Array.from(out);
}

/** Guard used in tests and before every write: a literal secret must never appear in opencode.json (R-34). */
export function containsLiteralSecret(json: OpencodeJson): boolean {
  const s = JSON.stringify(json);
  return /"(apiKey|token|password|secret)"\s*:\s*"(?!\{env:|\{file:)[^"]+"/i.test(s) || /sk-[A-Za-z0-9]{12,}/.test(s);
}

/** Text preview shown in onboarding and Settings › Web search. */
export function webConfigPreview(ws: Workspace): string {
  const { mcp, permission, env } = webSearchConfig(ws);
  const lines: string[] = [];
  if (Object.keys(env).length) lines.push('# env of this workspace\'s opencode serve', ...Object.entries(env).map(([k, v]) => `${k}=${v}`), '');
  lines.push('// opencode.json');
  if (Object.keys(mcp).length) lines.push('"mcp": ' + JSON.stringify(mcp, null, 2) + ',');
  lines.push('"permission": ' + JSON.stringify(permission));
  return lines.join('\n');
}
