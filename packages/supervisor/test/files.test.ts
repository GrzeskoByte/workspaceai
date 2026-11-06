// `@` references: which files and folders the composer offers.
import { describe, it, expect } from 'vitest';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { searchFiles } from '../src/files.ts';

function tree(): string {
  const dir = mkdtempSync(join(tmpdir(), 'wsai-files-'));
  for (const d of ['src/views', 'node_modules/x', '.harness/memory']) mkdirSync(join(dir, d), { recursive: true });
  for (const f of ['main.ts', 'src/main.ts', 'src/views/Chat.tsx', 'my notes.md', 'node_modules/x/main.ts', '.harness/memory/facts.md', 'ignored.log']) writeFileSync(join(dir, f), '');
  return dir;
}

describe.each([['without git', false], ['in a git repository', true]])('searchFiles %s', (_name, git) => {
  const dir = tree();
  if (git) {
    writeFileSync(join(dir, '.gitignore'), 'ignored.log\nnode_modules\n');
    execFileSync('git', ['init', '-q'], { cwd: dir });
  }

  it('offers files and folders, best match first, and never dependency or Harness folders', async () => {
    expect((await searchFiles(dir, 'main')).slice(0, 2)).toEqual(['main.ts', 'src/main.ts']);
    expect(await searchFiles(dir, 'view')).toContain('src/views/');
    const all = await searchFiles(dir, '', 100);
    expect(all).toContain('my notes.md');
    expect(all.some((p) => p.startsWith('node_modules') || p.startsWith('.harness'))).toBe(false);
  });

  it('completes inside a folder already typed in full', async () => {
    const inside = await searchFiles(dir, 'src/');
    expect(inside).not.toContain('src/');
    expect(inside).toContain('src/main.ts');
  });

  it('caps the results', async () => {
    expect(await searchFiles(dir, '', 2)).toHaveLength(2);
  });
});

describe('searchFiles', () => {
  it('honours .gitignore in a repository', async () => {
    const dir = tree();
    writeFileSync(join(dir, '.gitignore'), 'ignored.log\n');
    execFileSync('git', ['init', '-q'], { cwd: dir });
    expect(await searchFiles(dir, 'ignored')).toEqual([]);
  });
  it('returns nothing for a folder that does not exist', async () => {
    expect(await searchFiles(join(tmpdir(), 'wsai-no-such-folder'), '')).toEqual([]);
  });
});
