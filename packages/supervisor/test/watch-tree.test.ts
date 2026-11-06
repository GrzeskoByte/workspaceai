// watchTree must not put inotify watches on node_modules, and a failed watch must not take the supervisor down.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { mkdtempSync, mkdirSync, writeFileSync, watch, type FSWatcher } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

vi.mock('node:fs', async (orig) => {
  const fs = await orig<typeof import('node:fs')>();
  return { ...fs, watch: vi.fn(fs.watch) };
});

process.env.HARNESS_HOME = mkdtempSync(join(tmpdir(), 'harness-wt-'));
const { watchTree } = await import('../src/graphify.ts');

const until = async (fn: () => boolean) => { const end = Date.now() + 5000; while (Date.now() < end && !fn()) await new Promise((r) => setTimeout(r, 50)); return fn(); };
const tree = () => {
  const root = mkdtempSync(join(tmpdir(), 'wt-'));
  mkdirSync(join(root, 'src', 'deep'), { recursive: true });
  mkdirSync(join(root, 'node_modules', 'pkg'), { recursive: true });
  mkdirSync(join(root, '.git'));
  return root;
};

describe('watchTree', () => {
  let stop = () => {};
  afterEach(() => { stop(); vi.mocked(watch).mockClear(); });

  it('watches directories one by one, leaving out skipped ones', () => {
    const root = tree();
    stop = () => handle.close();
    const handle = watchTree(root, () => {});
    expect(vi.mocked(watch).mock.calls.map((c) => c[0]).sort()).toEqual([root, join(root, 'src'), join(root, 'src', 'deep')]);
  });

  it('reports changes as paths under the root, including in a folder made after it started', async () => {
    const root = tree();
    const seen: string[] = [];
    const handle = watchTree(root, (rel) => seen.push(rel));
    stop = () => handle.close();
    writeFileSync(join(root, 'src', 'deep', 'a.ts'), '1');
    expect(await until(() => seen.includes(join('src', 'deep', 'a.ts')))).toBe(true);
    mkdirSync(join(root, 'src', 'fresh'));
    expect(await until(() => vi.mocked(watch).mock.calls.some((c) => c[0] === join(root, 'src', 'fresh')))).toBe(true);
    writeFileSync(join(root, 'src', 'fresh', 'b.ts'), '1');
    expect(await until(() => seen.includes(join('src', 'fresh', 'b.ts')))).toBe(true);
  });

  it('survives an error event from a watcher', () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    const handle = watchTree(tree(), () => {});
    stop = () => handle.close();
    const w = vi.mocked(watch).mock.results[0].value as FSWatcher;
    expect(() => w.emit('error', Object.assign(new Error('watch'), { code: 'ENOSPC' }))).not.toThrow();
    quiet.mockRestore();
  });
});
