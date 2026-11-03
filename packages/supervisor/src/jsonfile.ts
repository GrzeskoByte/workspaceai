import { readFileSync, writeFileSync, renameSync, mkdirSync, existsSync, appendFileSync } from 'node:fs';
import { dirname } from 'node:path';

export function readJson<T>(file: string, fallback: T): T {
  try { return JSON.parse(readFileSync(file, 'utf8')) as T; } catch { return fallback; }
}

/** Atomic write: temp file then rename, so a crash never leaves half a file. */
export function writeJson(file: string, value: unknown): void {
  mkdirSync(dirname(file), { recursive: true });
  const tmp = file + '.' + process.pid + '.tmp';
  writeFileSync(tmp, JSON.stringify(value, null, 2) + '\n', { mode: 0o600 });
  renameSync(tmp, file);
}

export function writeText(file: string, text: string): void {
  mkdirSync(dirname(file), { recursive: true });
  const tmp = file + '.' + process.pid + '.tmp';
  writeFileSync(tmp, text);
  renameSync(tmp, file);
}

export function readText(file: string, fallback = ''): string {
  try { return readFileSync(file, 'utf8'); } catch { return fallback; }
}

export function appendLine(file: string, line: string): void {
  mkdirSync(dirname(file), { recursive: true });
  appendFileSync(file, line + '\n', { mode: 0o600 });
}

export { existsSync };
