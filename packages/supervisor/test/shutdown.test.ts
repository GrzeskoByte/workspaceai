// R-42: the desktop shell closes the supervisor's stdin on quit; every sidecar must stop with it.
import { describe, it, expect } from 'vitest';
import { spawn } from 'node:child_process';
import { mkdtempSync, existsSync } from 'node:fs';
import { tmpdir, homedir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

const alive = (pid: number) => { try { process.kill(pid, 0); return true; } catch { return false; } };

describe('quitting stops every sidecar (R-42)', () => {
  it('closing stdin stops opencode and the Open Design daemon', async () => {
    const home = mkdtempSync(join(tmpdir(), 'harness-quit-'));
    const od = join(homedir(), '.harness/vendor/open-design');
    const child = spawn(process.execPath, [join(__dirname, '../src/main.ts')], {
      env: { ...process.env, HARNESS_HOME: home, HARNESS_KEYCHAIN: 'memory', HARNESS_PARENT_STDIN: '1', HARNESS_OD_DIR: existsSync(od) ? od : join(home, 'none') },
      stdio: ['pipe', 'pipe', 'inherit'],
    });
    const ready: { port: number; token: string } = await new Promise((resolve) => {
      createInterface({ input: child.stdout! }).on('line', (l) => { if (l.startsWith('HARNESS_READY ')) resolve(JSON.parse(l.slice(14))); });
    });
    const h = { 'x-harness-token': ready.token, 'content-type': 'application/json' };
    const B = `http://127.0.0.1:${ready.port}/api`;
    const ws = await (await fetch(B + '/workspaces', { method: 'POST', headers: h, body: JSON.stringify({ name: 'Q', kind: 'personal' }) })).json();
    const status = await (await fetch(`${B}/ws/${ws.id}/engine/start`, { method: 'POST', headers: h, body: '{}' })).json();
    expect(status.state).toBe('running');
    const ocPid: number = status.pid;
    expect(alive(ocPid)).toBe(true);
    const odState = (await (await fetch(B + '/opendesign/status', { headers: h })).json()) as { running: { url: string }[] };
    const exited = new Promise<number | null>((r) => child.on('exit', (c) => r(c)));
    child.stdin!.end();
    expect(await exited).toBe(0);
    await new Promise((r) => setTimeout(r, 500));
    expect(alive(ocPid)).toBe(false);
    for (const d of odState.running || []) {
      const up = await fetch(d.url + '/api/health', { signal: AbortSignal.timeout(1000) }).then(() => true, () => false);
      expect(up).toBe(false);
    }
  });
});
