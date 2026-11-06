import { describe, it, expect } from 'vitest';
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, renameSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe('dev supervisor watcher', () => {
  it('restarts on every atomic save, and exits when the app closes its pipe', async () => {
    const d = mkdtempSync(join(tmpdir(), 'watch-'));
    const src = join(d, 'packages/supervisor/src');
    mkdirSync(src, { recursive: true });
    mkdirSync(join(d, 'packages/shared/src'), { recursive: true });
    writeFileSync(join(src, 'main.mjs'), `console.log('START'); process.on('SIGTERM', () => process.exit(0)); setInterval(() => {}, 1000);`);
    writeFileSync(join(src, 'a.ts'), '// 0');
    const p = spawn(process.execPath, [resolve('scripts/watch-supervisor.mjs'), 'packages/supervisor/src/main.mjs'], { cwd: d, stdio: ['pipe', 'pipe', 'inherit'] });
    let starts = 0;
    p.stdout.on('data', (b) => { starts += (String(b).match(/START/g) || []).length; });
    await sleep(600);
    expect(starts).toBe(1);
    for (let i = 2; i <= 4; i++) {
      writeFileSync(join(src, 'a.ts.tmp'), '// ' + i);
      renameSync(join(src, 'a.ts.tmp'), join(src, 'a.ts'));
      await sleep(800);
      expect(starts).toBe(i);
    }
    const gone = new Promise((r) => p.once('exit', r));
    p.stdin.end();
    await gone;
    expect(p.exitCode).toBe(0);
  }, 15000);
});
