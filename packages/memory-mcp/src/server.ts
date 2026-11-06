#!/usr/bin/env node
// WorkspaceAI memory as an MCP server (stdio). One instance per workspace, started by that workspace's opencode.
// Facts live in Markdown files; new memories written by the agent arrive as suggestions the user approves (R-22).
// Tools: memory_search, memory_read, memory_write (exposed by opencode as harness-memory_*), plus graphify_query
// when the workspace's memory backend is Graphify (R-23). The graph is owned by the supervisor, so the tool
// asks it over loopback rather than reading or rebuilding graph.json here.
import { readFileSync, writeFileSync, mkdirSync, renameSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import { parseFacts, serializeFacts, searchFacts } from '../../shared/src/facts.ts';
import type { Fact, MemoryGraph, MemoryGraphEdge, MemoryGraphNode } from '../../shared/src/types.ts';

const dir = process.env.HARNESS_MEMORY_DIR;
if (!dir) { process.stderr.write('HARNESS_MEMORY_DIR is required\n'); process.exit(2); }

const SUP = process.env.HARNESS_SUPERVISOR;
const TOKEN = process.env.HARNESS_SUPERVISOR_TOKEN;
const WS = process.env.HARNESS_WS;

async function supervisor<T>(method: string, path: string): Promise<T> {
  if (!SUP || !TOKEN || !WS) throw new Error('WorkspaceAI supervisor is not reachable');
  const r = await fetch(`${SUP}/api/ws/${WS}${path}`, { method, headers: { 'x-harness-token': TOKEN, 'content-type': 'application/json' }, signal: AbortSignal.timeout(20000) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error((j as { error?: string }).error || 'request failed ' + r.status);
  return j as T;
}

interface Scope { id: string; name: string; folder?: string; parent?: string | null; file: string }

function scopes(): Scope[] {
  try { return (JSON.parse(readFileSync(join(dir as string, 'scopes.json'), 'utf8')) as { scopes: Scope[] }).scopes; }
  catch { return [{ id: 'ws', name: 'Workspace', file: join(dir as string, 'facts.md') }]; }
}

function read(scope: Scope): Fact[] { try { return parseFacts(readFileSync(scope.file, 'utf8'), scope.id); } catch { return []; } }

function write(scope: Scope, facts: Fact[]): void {
  mkdirSync(dirname(scope.file), { recursive: true });
  const tmp = scope.file + '.tmp';
  writeFileSync(tmp, serializeFacts(facts, scope.id === 'ws' ? 'Workspace memory' : scope.name + ' — project memory'));
  renameSync(tmp, scope.file);
}

/** Resolve a scope from a project name, path or folder; the narrowest folder containing `cwd` wins. */
function resolveScope(hint?: string): Scope {
  const all = scopes();
  if (!hint || /^(ws|workspace)$/i.test(hint)) return all.find((s) => s.id === 'ws') as Scope;
  const h = hint.toLowerCase();
  const byName = all.find((s) => s.name.toLowerCase() === h || s.name.toLowerCase().endsWith('› ' + h) || s.id === hint);
  if (byName) return byName;
  const byFolder = all.filter((s) => s.folder && (hint === s.folder || hint.startsWith(s.folder + '/'))).sort((a, b) => (b.folder as string).length - (a.folder as string).length)[0];
  return byFolder || (all.find((s) => s.id === 'ws') as Scope);
}

/** A project scope plus its parents, widening on request. */
function chain(scope: Scope): Scope[] {
  const all = scopes();
  const out: Scope[] = [scope];
  let cur = scope;
  for (let i = 0; i < 32 && cur.parent; i++) { const p = all.find((s) => s.id === cur.parent); if (!p) break; out.push(p); cur = p; }
  if (scope.id !== 'ws') out.push(all.find((s) => s.id === 'ws') as Scope);
  return out;
}

const server = new McpServer({ name: 'harness-memory', version: '0.1.0' });

server.registerTool('memory_search', {
  description: 'Search this workspace\'s approved memory facts. Pass `project` (name or folder) to search that project and its parents; omit it to search everything in the workspace.',
  inputSchema: { query: z.string().describe('What to look for'), project: z.string().optional().describe('Project name or folder; defaults to the whole workspace') },
}, async ({ query, project }) => {
  const sc = project ? chain(resolveScope(project)) : scopes();
  const facts = sc.flatMap((s) => read(s).map((f) => ({ ...f, scopeName: s.name })));
  const hits = searchFacts(facts, query).slice(0, 20) as (Fact & { scopeName: string })[];
  return { content: [{ type: 'text', text: hits.length ? hits.map((f) => `- ${f.text} (${f.scopeName}, ${f.id})`).join('\n') : 'No matching facts.' }] };
});

server.registerTool('memory_read', {
  description: 'Read every approved fact for a scope (the workspace, or a project and its parents).',
  inputSchema: { project: z.string().optional() },
}, async ({ project }) => {
  const sc = chain(resolveScope(project));
  const text = sc.map((s) => `## ${s.name}\n` + (read(s).filter((f) => f.status === 'approved').map((f) => `- ${f.text}`).join('\n') || '(none)')).join('\n\n');
  return { content: [{ type: 'text', text }] };
});

server.registerTool('memory_write', {
  description: 'Suggest a fact to remember. It is saved as a suggestion; the user approves it on the Memory tab before it is used.',
  inputSchema: { text: z.string().max(400), project: z.string().optional().describe('Project name or folder; omit for workspace scope'), source: z.string().optional() },
}, async ({ text, project, source }) => {
  const scope = resolveScope(project);
  const facts = read(scope);
  if (facts.some((f) => f.text.toLowerCase() === text.toLowerCase())) return { content: [{ type: 'text', text: 'Already remembered.' }] };
  const f: Fact = { id: 'f_' + Date.now().toString(36) + Math.random().toString(36).slice(2, 5), text, scope: scope.id, status: 'suggested', source: source || 'agent', createdAt: Date.now() };
  write(scope, [...facts, f]);
  // Tell the supervisor so the UI refreshes; failures are fine (the file is the source of truth).
  if (process.env.HARNESS_SUPERVISOR && process.env.HARNESS_SUPERVISOR_TOKEN) {
    fetch(`${process.env.HARNESS_SUPERVISOR}/api/ws/${process.env.HARNESS_WS}/memory-changed`, { method: 'POST', headers: { 'x-harness-token': process.env.HARNESS_SUPERVISOR_TOKEN, 'content-type': 'application/json' }, body: '{}' }).catch(() => {});
  }
  return { content: [{ type: 'text', text: `Suggested for ${scope.name}: "${text}". The user approves it before it is used.` }] };
});

/** Render a graph query as compact text: matching nodes, then the links that touch them. */
function formatGraph(r: { graph: MemoryGraph; matches: MemoryGraphNode[]; edges: MemoryGraphEdge[] }, q: string, limit: number): string {
  const labels = new Map(r.graph.nodes.map((n) => [n.id, n.label]));
  const communities = new Map(r.graph.communities.map((c) => [c.id, c.name]));
  const label = (id: string): string => labels.get(id) || id;
  const shown = r.matches.slice(0, limit);
  const head = `Graphify (${r.graph.source === 'graphify' ? 'graphify-out/graph.json' : 'workspace index'}) · ${r.graph.nodes.length} nodes · ${r.graph.edges.length} edges · ${r.graph.communities.length} communities`;
  if (!shown.length) return `${head}\n\nNo nodes match “${q}”. Try a shorter word, or call again with rebuild: true after files changed.`;
  const lines = [head, '', q ? `Matches for “${q}”:` : 'Largest nodes:'];
  for (const n of shown) {
    const links = r.edges.filter((e) => e.from === n.id || e.to === n.id).slice(0, 6);
    lines.push(`- ${n.label} [${n.kind}] · ${communities.get(n.community) || n.community} · ${n.id}`);
    for (const e of links) lines.push(`  ${e.from === n.id ? '→' : '←'} ${label(e.from === n.id ? e.to : e.from)} (${e.kind}${e.ambiguous ? ', ambiguous — pending review' : ''})`);
  }
  if (r.matches.length > shown.length) lines.push('', `…and ${r.matches.length - shown.length} more matches; narrow the query or raise limit.`);
  if (r.graph.truncated) lines.push('', 'The graph is truncated: it covers only the first files of the configured folders.');
  return lines.join('\n');
}

// Graphify is the backend only in `graph` and `graph` + facts workspaces (R-23). Facts-only and custom
// memory MCP workspaces must not see a graph tool that would build an index nobody asked for.
const backend = process.env.HARNESS_MEMORY_BACKEND || 'both';
if (backend === 'graph' || backend === 'both') {
  server.registerTool('graphify_query', {
    description: 'Search this workspace\'s memory graph (Graphify): files, folders, symbols and concepts, with their links and communities. Use it to find where something lives before opening files, or to see what a change would touch. Read-only unless rebuild is set.',
    inputSchema: {
      query: z.string().optional().describe('Words to look for in node names or paths; omit for the largest nodes'),
      limit: z.number().int().min(1).max(40).optional().describe('Maximum matches to show (default 12)'),
      rebuild: z.boolean().optional().describe('Rebuild the graph first; use after adding, moving or deleting many files'),
    },
  }, async ({ query, limit, rebuild }) => {
    try {
      if (rebuild) await supervisor('POST', '/graphify/build');
      const r = await supervisor<{ graph: MemoryGraph; matches: MemoryGraphNode[]; edges: MemoryGraphEdge[] }>('GET', '/graphify/query?q=' + encodeURIComponent(query || ''));
      return { content: [{ type: 'text' as const, text: formatGraph(r, (query || '').trim(), limit || 12) }] };
    } catch (e) {
      return { content: [{ type: 'text' as const, text: `Graphify query failed: ${(e as Error).message}` }] };
    }
  });
}

await server.connect(new StdioServerTransport());
