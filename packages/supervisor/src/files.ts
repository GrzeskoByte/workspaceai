// Finds files and folders in a project for `@` references in the composer. Paths only, relative to the project.
import { execFile } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fuzzyScore } from '../../shared/src/util.ts';
import { SKIP } from './graphify.ts';

/** Tracked and untracked files, honouring .gitignore; null when the folder isn't in a git repository. */
function gitFiles(folder: string): Promise<string[] | null> {
  return new Promise((done) => {
    execFile('git', ['-C', folder, 'ls-files', '-co', '--exclude-standard', '-z'], { maxBuffer: 64 * 1024 * 1024, timeout: 5000 }, (err, out) => done(err ? null : out.split('\0').filter(Boolean)));
  });
}

// ponytail: walks at most 20000 entries without git; ranking only sees those.
function walkFiles(folder: string): string[] {
  const out: string[] = [];
  let budget = 20000;
  const walk = (rel: string) => {
    let entries;
    try { entries = readdirSync(join(folder, rel), { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      if (budget-- <= 0) return;
      const path = rel ? `${rel}/${e.name}` : e.name;
      if (e.isDirectory()) { if (!SKIP.has(e.name)) walk(path); } else if (e.isFile()) out.push(path);
    }
  };
  walk('');
  return out;
}

/** Best matches for `query`, a folder being a path ending in `/`. Names beat paths, then shorter paths win. */
export async function searchFiles(folder: string, query: string, limit = 30): Promise<string[]> {
  const files = ((await gitFiles(folder)) ?? walkFiles(folder)).filter((f) => !f.split('/').some((seg) => SKIP.has(seg)));
  const dirs = new Set<string>();
  for (const f of files) for (let i = f.indexOf('/'); i !== -1; i = f.indexOf('/', i + 1)) dirs.add(f.slice(0, i + 1));
  const name = (p: string) => p.replace(/\/$/, '').split('/').pop() as string;
  return [...dirs, ...files]
    .filter((p) => p !== query) // a folder already typed in full: offer what is inside it
    .map((p) => ({ p, score: fuzzyScore(query, name(p), p) }))
    .filter((x) => x.score > 0)
    .sort((a, b) => b.score - a.score || a.p.length - b.p.length || a.p.localeCompare(b.p))
    .slice(0, limit)
    .map((x) => x.p);
}
