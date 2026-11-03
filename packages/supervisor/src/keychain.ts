// Secrets live in the OS keychain under a per-workspace prefix (R-34). Config files only reference them.
// HARNESS_KEYCHAIN=memory keeps secrets in process memory (tests, CI without a Secret Service).
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const SERVICE = 'harness';

interface Backend { get(account: string): string | null; set(account: string, value: string): void; del(account: string): void }

function osBackend(): Backend {
  const { Entry } = require('@napi-rs/keyring') as { Entry: new (s: string, a: string) => { getPassword(): string | null; setPassword(v: string): void; deletePassword(): boolean } };
  return {
    get: (a) => { try { return new Entry(SERVICE, a).getPassword() ?? null; } catch { return null; } },
    set: (a, v) => new Entry(SERVICE, a).setPassword(v),
    del: (a) => { try { new Entry(SERVICE, a).deletePassword(); } catch { /* absent */ } },
  };
}

function memoryBackend(): Backend {
  const m = new Map<string, string>();
  return { get: (a) => m.get(a) ?? null, set: (a, v) => { m.set(a, v); }, del: (a) => { m.delete(a); } };
}

const backend: Backend = process.env.HARNESS_KEYCHAIN === 'memory' ? memoryBackend() : osBackend();

/** Index of secret names per workspace, so we can list and delete without enumerating the OS keychain. */
const indexAccount = (ws: string) => `${ws}/__index__`;

function readIndex(ws: string): string[] {
  try { return JSON.parse(backend.get(indexAccount(ws)) || '[]'); } catch { return []; }
}

export const keychain = {
  backendName: process.env.HARNESS_KEYCHAIN === 'memory' ? 'memory' : 'os',
  get(ws: string, name: string): string | null { return backend.get(`${ws}/${name}`); },
  set(ws: string, name: string, value: string): void {
    backend.set(`${ws}/${name}`, value);
    const idx = readIndex(ws);
    if (!idx.includes(name)) backend.set(indexAccount(ws), JSON.stringify([...idx, name]));
  },
  has(ws: string, name: string): boolean { return backend.get(`${ws}/${name}`) != null; },
  delete(ws: string, name: string): void {
    backend.del(`${ws}/${name}`);
    backend.set(indexAccount(ws), JSON.stringify(readIndex(ws).filter((n) => n !== name)));
  },
  list(ws: string): string[] { return readIndex(ws); },
  /** Removes every secret a workspace owns (workspace delete / leave org). */
  purge(ws: string): void { for (const n of readIndex(ws)) backend.del(`${ws}/${n}`); backend.del(indexAccount(ws)); },
};
