import { homedir } from 'node:os';
import { join } from 'node:path';
import { mkdirSync } from 'node:fs';

/** ~/.harness, overridable for tests and dev (HARNESS_HOME). */
export function harnessHome(): string {
  return process.env.HARNESS_HOME || join(homedir(), '.harness');
}

export function expandHome(p: string): string {
  if (p === '~') return homedir();
  if (p.startsWith('~/')) return join(homedir(), p.slice(2));
  return p;
}

export function contractHome(p: string): string {
  const h = homedir();
  return p.startsWith(h + '/') ? '~' + p.slice(h.length) : p;
}

export interface WsPaths {
  root: string;
  config: string;
  opencodeJson: string;
  harnessJson: string;
  policyJson: string;
  data: string;
  state: string;
  xdgData: string;
  xdgConfig: string;
  xdgState: string;
  xdgCache: string;
  openDesign: string;
  memory: string;
  audit: string;
  runs: string;
  browserProfile: string;
  logs: string;
}

export function wsPaths(id: string): WsPaths {
  const root = join(harnessHome(), 'ws', id);
  const config = join(root, 'config');
  const data = join(root, 'data');
  return {
    root, config, data,
    opencodeJson: join(config, 'opencode.json'),
    harnessJson: join(config, 'harness.json'),
    policyJson: join(config, 'policy.json'),
    state: join(data, 'state.json'),
    xdgData: join(data, 'opencode', 'xdg-data'),
    xdgConfig: join(config, 'xdg'),
    xdgState: join(data, 'opencode', 'xdg-state'),
    xdgCache: join(data, 'opencode', 'xdg-cache'),
    openDesign: join(data, 'open-design'),
    memory: join(root, 'memory'),
    audit: join(data, 'audit.jsonl'),
    runs: join(data, 'runs'),
    browserProfile: join(data, 'browser-profile'),
    logs: join(data, 'logs'),
  };
}

export function ensureWsDirs(id: string): WsPaths {
  const p = wsPaths(id);
  for (const d of [p.config, p.data, p.xdgData, p.xdgConfig, p.xdgState, p.xdgCache, p.openDesign, p.memory, p.runs, p.browserProfile, p.logs]) mkdirSync(d, { recursive: true });
  return p;
}

export function appJsonPath(): string { return join(harnessHome(), 'app.json'); }
export function computersJsonPath(): string { return join(harnessHome(), 'computers.json'); }
