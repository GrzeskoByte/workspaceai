// The side panel lists what a chat's tools read from memory and the vaults.
import { describe, it, expect } from 'vitest';
import { readsFromTools } from '../src/messages.ts';
import type { ToolStep } from '../src/types.ts';

const vaults = [{ id: 'v1', name: 'Notes', path: '/home/me/Notes' }];
const step = (name: string, input: unknown, status: ToolStep['status'] = 'ok'): ToolStep => ({ name, status, input: JSON.stringify(input) });

describe('readsFromTools', () => {
  it('finds vault and memory tools, deduped', () => {
    const r = readsFromTools([
      step('harness-vault_vault_read a/b.md', { vault: 'Notes', path: 'a/b.md' }),
      step('harness-vault_vault_read a/b.md', { vault: 'v1', path: 'a/b.md' }),
      step('harness-memory_memory_search', { query: 'auth' }),
      step('harness-memory_memory_read', {}),
    ], vaults);
    expect(r.map((x) => [x.kind, x.label])).toEqual([['wiki', 'a/b.md'], ['memory', 'Search · auth'], ['memory', 'Facts · workspace']]);
    expect(r[0].note).toEqual({ vault: 'v1', path: 'a/b.md' });
  });
  it('reads calls out of code-mode execute', () => {
    const r = readsFromTools([step('harness-vault.vault_read', { code: 'await tools["harness-vault"].vault_read({ vault: "Notes", path: "x.md" })' })], vaults);
    expect(r[0]).toMatchObject({ kind: 'wiki', label: 'x.md', note: { vault: 'v1', path: 'x.md' } });
  });
  it('classifies file reads, and only reads', () => {
    const r = readsFromTools([
      step('Read', { file_path: '/home/me/Notes/server/auth.md' }),
      step('mcp__lean-ctx__ctx_read', { path: '/home/me/.claude/projects/-x/memory/MEMORY.md' }),
      step('read CLAUDE.md', { filePath: '/repo/CLAUDE.md' }),
      step('Read', { file_path: '/repo/README.md' }),
      step('Read', { file_path: '/repo/main.ts' }),
      step('Edit', { file_path: '/home/me/Notes/server/edited.md' }),
      step('Read', { file_path: '/home/me/Notes/failed.md' }, 'error'),
    ], vaults);
    expect(r.map((x) => x.label)).toEqual(['server/auth.md', 'memory/MEMORY.md', '/repo/CLAUDE.md']);
    expect(r[0].note).toEqual({ vault: 'v1', path: 'server/auth.md' });
  });
});
