// lean-ctx (https://leanctx.com) as a native feature: installed into the harness's own folder, connected to every
// agent as an MCP server (compressed reads, searches and shell output), its options edited through its own CLI and
// config schema, and its token savings read from `lean-ctx gain --json` — the same numbers its Cockpit shows.
import { execFile } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { harnessHome } from './paths.ts';
import { baseEnv } from './opencode.ts';

const run = promisify(execFile);

export function leanCtxDir(): string { return process.env.HARNESS_LEANCTX_DIR || join(harnessHome(), 'vendor', 'lean-ctx'); }
const pkgDir = () => join(leanCtxDir(), 'node_modules', 'lean-ctx-bin');

/** The native binary (the npm package's postinstall downloads and verifies it). */
export function leanCtxBin(): string | null {
  const candidates = [process.env.HARNESS_LEANCTX_BIN, join(pkgDir(), 'bin', 'lean-ctx')].filter(Boolean) as string[];
  for (const c of candidates) if (existsSync(c)) return c;
  return null;
}

// lean-ctx never rewires the user's other AI tools from here, and never auto-updates behind the harness's back.
const env = () => ({ ...baseEnv(), LEAN_CTX_NO_ONBOARD: '1', NO_COLOR: '1' });

async function cli(args: string[], timeout = 20000): Promise<string> {
  const bin = leanCtxBin();
  if (!bin) throw Object.assign(new Error('lean-ctx is not installed'), { status: 409 });
  const r = await run(bin, args, { env: env(), timeout, maxBuffer: 16 << 20 });
  return String(r.stdout);
}

let versionCache: { bin: string; version: string } | null = null;
export async function leanCtxVersion(): Promise<string> {
  const bin = leanCtxBin();
  if (!bin) return '';
  if (versionCache?.bin === bin) return versionCache.version;
  const v = (await cli(['--version']).catch(() => '')).match(/lean-ctx\s+([\w.-]+)/)?.[1] || '';
  versionCache = { bin, version: v };
  return v;
}

/**
 * Install or update from npm. Its preinstall script stops running lean-ctx processes with `pkill -f lean-ctx`,
 * which would also kill whatever started the install, so scripts are skipped and the postinstall (download the
 * release binary and verify its SHA-256) is run on its own, without `lean-ctx onboard`.
 */
export async function installLeanCtx(): Promise<{ ok: boolean; output: string; version?: string }> {
  const npm = await run('sh', ['-c', 'command -v npm']).then((r) => r.stdout.trim()).catch(() => '');
  if (!npm) return { ok: false, output: 'Installing lean-ctx needs npm (Node.js). Install Node.js, then try again.' };
  const opts = { env: env(), timeout: 10 * 60000, maxBuffer: 8 << 20 };
  try {
    const bin = leanCtxBin();
    if (bin) await run(bin, ['stop'], { env: env(), timeout: 10000 }).catch(() => {});
    const a = await run(npm, ['install', '--prefix', leanCtxDir(), '--ignore-scripts', '--no-fund', '--no-audit', 'lean-ctx-bin@latest'], opts);
    const b = await run(process.execPath, [join(pkgDir(), 'postinstall.js')], { ...opts, cwd: pkgDir(), env: { ...env(), LEAN_CTX_NO_COCKPIT: '1' } });
    versionCache = null;
    // Its first run would otherwise install SKILL.md and rule files into Claude Code, Cursor and others; here it
    // only serves the harness's agents, so it must not touch other tools' setups.
    const fresh = leanCtxBin();
    if (fresh) for (const k of ['setup.auto_inject_skills', 'setup.auto_inject_rules']) await run(fresh, ['config', 'set', k, 'false'], { env: env(), timeout: 20000 }).catch(() => {});
    const out = (a.stdout + a.stderr + b.stdout + b.stderr).replace(/\x1b\[[0-9;]*m/g, '').split('\n').filter((l) => l.trim() && !/[│┌└┐┘─]/.test(l)).join('\n');
    if (!leanCtxBin()) return { ok: false, output: 'The download did not leave a lean-ctx binary.\n' + out.slice(-1800) };
    return { ok: true, output: out.slice(-1800), version: await leanCtxVersion() };
  } catch (e) { return { ok: false, output: String((e as Error).message).slice(-2000) }; }
}

/** How agents start it: `lean-ctx` alone is its MCP server over stdio. */
export function leanCtxMcp(): { command: string[]; environment: Record<string, string> } | null {
  const bin = leanCtxBin();
  return bin ? { command: [bin], environment: { LEAN_CTX_NO_ONBOARD: '1' } } : null;
}

/** Told to every agent when lean-ctx is on for the workspace. */
export const LEANCTX_RULE = `## Context (lean-ctx)
lean-ctx is connected as MCP tools that return the same information in far fewer tokens. Prefer them: \`ctx_read\` to read files (mode=map or mode=sigs first for large files, mode=full when you will edit, mode=diff after editing), \`ctx_search\` instead of grep, \`ctx_shell\` for commands with long output, \`ctx_compose\` to find the files for a task. Use a native tool only when you need exact bytes or lean-ctx cannot do it.`;

// ---------- savings ----------

export interface LeanCtxGain {
  tokensSaved: number; netTokensSaved: number; inputTokens: number; outputTokens: number;
  reductionPct: number; commands: number; turns: number; avoidedUsd: number; overheadPerTurn: number;
  evidence: string; model: string;
  byTool: { name: string; calls: number; saved: number; pct: number }[];
}

const num = (v: unknown) => (typeof v === 'number' && isFinite(v) ? v : 0);

export async function leanCtxGain(period?: string): Promise<LeanCtxGain> {
  const p = period && ['week', 'month', 'all'].includes(period) ? [`--period=${period}`] : [];
  const g = JSON.parse(await cli(['gain', '--json', ...p], 30000)) as { summary?: Record<string, any> };
  const s = g.summary || {};
  let byTool: LeanCtxGain['byTool'] = [];
  try {
    const rows = JSON.parse(await cli(['gain', '--by-tool', '--json', ...p], 30000));
    const list: any[] = Array.isArray(rows) ? rows : rows?.tools || rows?.rows || [];
    byTool = list.map((r) => ({
      name: String(r.tool ?? r.name ?? r.key ?? '?'),
      calls: num(r.calls ?? r.count),
      saved: num(r.tokens_saved ?? r.saved_tokens ?? r.saved),
      pct: num(r.pct ?? r.percent ?? r.share_pct ?? r.reduction_pct),
    })).sort((a, b) => b.saved - a.saved).slice(0, 12);
  } catch { /* older lean-ctx: totals only */ }
  return {
    tokensSaved: num(s.tokens_saved), netTokensSaved: num(s.net_tokens_saved), inputTokens: num(s.input_tokens), outputTokens: num(s.output_tokens),
    reductionPct: num(s.total_reduction_pct), commands: num(s.total_commands), turns: num(s.turns), avoidedUsd: num(s.avoided_usd),
    overheadPerTurn: num(s.injected_overhead_tokens_per_turn), evidence: String(s.economic_evidence || ''), model: String(s.model?.model_key || ''),
    byTool,
  };
}

// ---------- options ----------

export interface LeanCtxOption { key: string; section: string; type: string; description: string; default: unknown; values?: string[]; value: unknown; set: boolean }

/** lean-ctx's own config schema, with the value each key has in its config file (or its default). */
export async function leanCtxOptions(): Promise<{ path: string; options: LeanCtxOption[]; toolProfile: string }> {
  const schema = JSON.parse(await cli(['config', 'schema'])) as { sections: Record<string, { keys: Record<string, { type: string; default: unknown; description?: string; values?: string[] }> }> };
  const path = (await cli(['config', 'path']).catch(() => '')).trim();
  const file = path && existsSync(path) ? parseToml(readFileSync(path, 'utf8')) : {};
  const options: LeanCtxOption[] = [];
  for (const [section, s] of Object.entries(schema.sections || {})) {
    for (const [k, d] of Object.entries(s.keys || {})) {
      const key = section === 'root' ? k : `${section}.${k}`;
      const set = key in file;
      options.push({ key, section, type: d.type, description: d.description || '', default: d.default, ...(d.values ? { values: d.values } : {}), value: set ? file[key] : d.default, set });
    }
  }
  const toolProfile = (await cli(['tools', 'show']).catch(() => '')).match(/Tool Profile:\s*(\w+)/)?.[1] || 'lean';
  return { path, options, toolProfile };
}

/** Flat `section.key → value` from lean-ctx's TOML (scalars and simple arrays; tables of tables are skipped). */
export function parseToml(text: string): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  let section = '';
  for (const raw of text.split('\n')) {
    const line = raw.replace(/\s+#.*$/, '').trim();
    if (!line || line.startsWith('#')) continue;
    const h = /^\[([^\]]+)\]$/.exec(line);
    if (h) { section = h[1].trim(); continue; }
    const m = /^([\w.-]+)\s*=\s*(.+)$/.exec(line);
    if (!m) continue;
    const v = m[2].trim();
    let val: unknown = v;
    if (v === 'true' || v === 'false') val = v === 'true';
    else if (/^-?\d+(\.\d+)?$/.test(v)) val = Number(v);
    else if (/^".*"$/.test(v)) val = v.slice(1, -1).replace(/\\"/g, '"');
    else if (/^\[.*\]$/.test(v)) { try { val = JSON.parse(v); } catch { val = v; } }
    out[section ? `${section}.${m[1]}` : m[1]] = val;
  }
  return out;
}

export async function setLeanCtxOption(key: string, value: unknown): Promise<string> {
  if (!/^[\w.-]+$/.test(key)) throw Object.assign(new Error('Unknown lean-ctx option'), { status: 400 });
  const v = Array.isArray(value) ? JSON.stringify(value) : String(value);
  return (await cli(['config', 'set', key, v])).trim();
}

export async function setLeanCtxToolProfile(profile: string): Promise<string> {
  if (!['lean', 'minimal', 'standard', 'power'].includes(profile)) throw Object.assign(new Error('Tool profile must be lean, minimal, standard or power'), { status: 400 });
  return (await cli(['tools', profile])).trim().split('\n')[0];
}
