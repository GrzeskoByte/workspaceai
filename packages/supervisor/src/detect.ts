// First-run detection: is opencode installed, and which sessions does it already have (R-17, R-18)?
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { ManagedServer, baseEnv, type OpencodeClient, type OcSession } from './opencode.ts';
import type { WorkspaceRuntime } from './runtime.ts';

const run = promisify(execFile);

export async function findOpencode(binary = 'opencode'): Promise<{ found: boolean; path?: string; version?: string }> {
  try {
    const { stdout: path } = await run('sh', ['-c', `command -v ${binary.replace(/[^\w./-]/g, '')}`]);
    const { stdout: v } = await run(path.trim(), ['--version'], { timeout: 10000 });
    return { found: true, path: path.trim(), version: v.trim().replace(/^opencode\s+/i, '') };
  } catch { return { found: false }; }
}

/**
 * Sessions in this computer's own opencode data. A temporary server is started with the user's normal
 * environment (their XDG dirs), read through the API, and stopped again. Nothing is written to it.
 */
export class LocalOpencodeData {
  private server: ManagedServer | null = null;
  private client: OpencodeClient | null = null;
  private stopTimer: NodeJS.Timeout | null = null;

  async open(binary = 'opencode'): Promise<OpencodeClient> {
    if (this.stopTimer) clearTimeout(this.stopTimer);
    this.stopTimer = setTimeout(() => this.close(), 5 * 60000);
    this.stopTimer.unref();
    if (this.client && this.server?.proc) return this.client;
    const env = { ...baseEnv() };
    for (const k of ['XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_STATE_HOME', 'XDG_CACHE_HOME']) if (process.env[k]) env[k] = process.env[k] as string;
    this.server = new ManagedServer({ binary, cwd: process.env.HOME || '/', env, logDir: mkdtempSync(join(tmpdir(), 'harness-scan-')) });
    this.client = await this.server.start();
    return this.client;
  }

  async sessions(binary?: string): Promise<OcSession[]> {
    const c = await this.open(binary);
    const all = await c.listSessions({ limit: 2000 });
    return all.filter((s) => !s.parentID && !s.time.archived);
  }

  async close(): Promise<void> { const s = this.server; this.server = null; this.client = null; if (s) await s.stop(); }
}

/** Copy sessions with full history from a source server into a workspace's own server, then map them to chats. */
export async function copySessions(source: OpencodeClient, rt: WorkspaceRuntime, ids: string[]): Promise<{ chats: number; projects: number; failed: string[] }> {
  return rt.owning(() => copyInner(source, rt, ids));
}

async function copyInner(source: OpencodeClient, rt: WorkspaceRuntime, ids: string[]): Promise<{ chats: number; projects: number; failed: string[] }> {
  const target = await rt.client();
  const imported: OcSession[] = [];
  const failed: string[] = [];
  for (const id of ids) {
    try {
      const exp = await source.exportSession(id) as Record<string, unknown>;
      const payload = (exp.data ?? exp) as { info: OcSession; messages: unknown[] };
      const dir = payload.info?.location?.directory;
      const r = await target.importSession(payload.info, payload.messages || [], dir);
      rt.markOwn(r.data.id);
      imported.push(r.data);
    } catch { failed.push(id); }
  }
  const res = rt.importSessions(imported);
  return { ...res, failed };
}
