// Points Pi at the workspace's models. Pi reads one agent folder (PI_CODING_AGENT_DIR), so each workspace gets its
// own: models.json and settings.json are generated, everything else (login, sessions, packages) links to the user's
// real folder, so Pi keeps working as before and their own config is never written to.
import { lstatSync, mkdirSync, readdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { effectiveProviders, NATIVE_PROVIDERS } from '../../shared/src/opencode-config.ts';
import type { Provider } from '../../shared/src/types.ts';
import { keychain } from './keychain.ts';
import type { WorkspaceRuntime } from './runtime.ts';

const API: Record<string, string> = { '@ai-sdk/anthropic': 'anthropic-messages', '@ai-sdk/openai': 'openai-responses', '@ai-sdk/google': 'google-generative-ai' };

/** A workspace's providers as Pi's models.json `providers`. Keys stay `$NAME` references, never values. */
export function piProviders(providers: Provider[], hasKey: (env: string) => boolean): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const p of providers) {
    if (p.fromOpencode) continue;
    // Pi must be able to resolve a key even for an endpoint that ignores it, and an unset $NAME hides the models.
    const apiKey = p.keyEnv && hasKey(p.keyEnv) ? '$' + p.keyEnv : 'none';
    if (NATIVE_PROVIDERS.has(p.id)) {
      // Pi knows these and reads their usual key variable itself; it only needs telling about a proxy.
      if (p.baseURL) out[p.id] = { baseUrl: p.baseURL, ...(p.keyEnv ? { apiKey } : {}) };
      continue;
    }
    if (!p.baseURL || !p.models.length) continue;
    out[p.id] = {
      baseUrl: p.baseURL, api: API[p.npm || ''] || 'openai-completions', apiKey,
      models: p.models.map((m) => ({ id: m.id, ...(m.name ? { name: m.name } : {}), ...(m.context ? { contextWindow: m.context } : {}), ...(m.output ? { maxTokens: m.output } : {}) })),
    };
  }
  return out;
}

const readObject = (file: string): Record<string, any> => { try { return JSON.parse(readFileSync(file, 'utf8')); } catch { return {}; } };

/** Writes this workspace's Pi folder. `signature` changes when a running Pi has to restart to see the new config. */
export function piAgentDir(rt: WorkspaceRuntime): { env: Record<string, string>; signature: string } {
  const dir = join(rt.paths.data, 'agents', 'pi');
  const real = process.env.PI_CODING_AGENT_DIR || join(homedir(), '.pi', 'agent');
  mkdirSync(dir, { recursive: true });
  let names: string[] = [];
  try { names = readdirSync(real); } catch { /* Pi has never run here */ }
  for (const name of names) {
    if (name === 'models.json' || name === 'settings.json') continue;
    try { lstatSync(join(dir, name)); } catch { symlinkSync(join(real, name), join(dir, name)); }
  }
  const providers = effectiveProviders(rt.ws, rt.policy);
  const env: Record<string, string> = { PI_CODING_AGENT_DIR: dir };
  for (const p of providers) { const v = p.keyEnv && keychain.get(rt.id, p.keyEnv); if (v) env[p.keyEnv!] = v; }
  const own = readObject(join(real, 'models.json'));
  const models = JSON.stringify({ ...own, providers: { ...own.providers, ...piProviders(providers, (e) => e in env) } }, null, 2);
  // 0600: the user's own models.json may hold literal keys.
  writeFileSync(join(dir, 'models.json'), models, { mode: 0o600 });
  // A copy, because Pi saves the last model picked into settings; that must not change the user's own default.
  const [provider, ...id] = rt.ws.defaultModel.split('/');
  writeFileSync(join(dir, 'settings.json'), JSON.stringify({ ...readObject(join(real, 'settings.json')), ...(id.length ? { defaultProvider: provider, defaultModel: id.join('/') } : {}) }, null, 2), { mode: 0o600 });
  return { env, signature: JSON.stringify([models, env]) };
}
