// Memory recall on every message: the harness looks the message up in the workspace's memory — the Graphify graph
// and approved facts — and hands the agent what matched, so memory is used whether or not the agent thinks to ask.
// The agents' instructions (MEMORY_RULE) tell them to go further with the memory tools when that is not enough.
import { projectPath } from '../../shared/src/util.ts';
import type { MemoryGraphNode } from '../../shared/src/types.ts';
import { cachedMemoryGraph, getMemoryGraph, queryTerms, rankNodes } from './graphify.ts';
import type { WorkspaceRuntime } from './runtime.ts';

const OPEN = '<harness-memory>', CLOSE = '</harness-memory>';

/** Strip the recall block from a stored user message (the transcript shows only what the user typed). */
export function stripRecall(text: string): string {
  return text.replace(/(\n*<harness-(memory|graph)>[\s\S]*?<\/harness-\2>)+\s*$/, '');
}

/** What memory has on this message, as a block appended to the prompt; empty when nothing matches. */
export function memoryRecall(rt: WorkspaceRuntime, pid: string, text: string): string {
  const mode = rt.ws.memory;
  if (mode === 'mcp' || rt.ws.useMemory === false || rt.ws.memoryRecall === false) return '';
  const terms = queryTerms(text);
  if (!terms.length) return '';
  const lines: string[] = [];
  if (mode === 'facts' || mode === 'both') {
    const scopes = ['ws', ...projectPath(rt.state.projects, pid).map((p) => p.id)];
    const facts = scopes.flatMap((s) => rt.scopeFacts(s)).filter((f) => f.status === 'approved');
    const hits = facts.map((f) => ({ f, s: terms.filter((t) => f.text.toLowerCase().includes(t)).length })).filter((x) => x.s > 0).sort((a, b) => b.s - a.s).slice(0, 6);
    if (hits.length) lines.push('Facts:', ...hits.map((h) => `- ${h.f.text}`));
  }
  if (mode === 'graph' || mode === 'both') {
    const graph = recallGraph(rt);
    const nodes = graph ? rankNodes(graph, terms, 8) : [];
    if (graph && nodes.length) {
      const byId = new Map(graph.nodes.map((n) => [n.id, n] as [string, MemoryGraphNode]));
      const rel = (n: MemoryGraphNode) => graph.edges.filter((e) => e.from === n.id || e.to === n.id).slice(0, 4)
        .map((e) => { const o = byId.get(e.from === n.id ? e.to : e.from); return o ? `${e.kind} ${o.label}` : ''; }).filter(Boolean);
      lines.push(`Graph (${graph.source === 'graphify' ? 'Graphify' : 'workspace index'}):`, ...nodes.map((n) => { const r = rel(n); return `- ${n.label} (${n.kind}${n.id !== n.label ? ', ' + n.id : ''})${r.length ? ' — ' + r.join('; ') : ''}`; }));
    }
  }
  if (!lines.length) return '';
  const tools = mode === 'facts' ? 'memory_search' : mode === 'graph' ? 'graphify_query' : 'graphify_query and memory_search';
  return `\n\n${OPEN}\nFrom this workspace's memory, matched to the message above (use what is relevant; call ${tools} for more):\n${lines.join('\n').slice(0, 2400)}\n${CLOSE}`;
}

let building = 0;
/** The built graph; when there is none yet it is built in the background, so the next message has it. */
function recallGraph(rt: WorkspaceRuntime) {
  const graph = cachedMemoryGraph(rt);
  if (!graph && Date.now() - building > 60000) { building = Date.now(); setTimeout(() => { try { getMemoryGraph(rt); } catch { /* no folders yet */ } }, 0); }
  return graph;
}

/**
 * What memory holds, for the start of every session: the approved facts for this project (and the workspace) and
 * what the memory graph covers. Part of the harness instructions, so each new chat starts from memory.
 */
export function memoryBrief(rt: WorkspaceRuntime, pid: string): string {
  const mode = rt.ws.memory;
  if (mode === 'mcp' || rt.ws.useMemory === false) return '';
  const out: string[] = [];
  if (mode === 'facts' || mode === 'both') {
    const scopes = [...projectPath(rt.state.projects, pid).map((p) => p.id).reverse(), 'ws'];
    const facts = scopes.flatMap((s) => rt.scopeFacts(s)).filter((f) => f.status === 'approved').slice(0, 15);
    if (facts.length) out.push('Facts you already know:', ...facts.map((f) => `- ${f.text}`));
  }
  if (mode === 'graph' || mode === 'both') {
    const graph = recallGraph(rt);
    if (graph?.nodes.length) {
      const areas = graph.nodes.filter((n) => n.kind === 'community').sort((a, b) => (b.size || 0) - (a.size || 0)).slice(0, 8).map((n) => n.label);
      out.push(`Memory graph (${graph.source === 'graphify' ? 'Graphify' : 'workspace index'}): ${graph.nodes.length} nodes, ${graph.edges.length} links over ${graph.folders.join(', ') || 'this workspace'}${areas.length ? '; main areas: ' + areas.join(', ') : ''}. Look things up with \`graphify_query\`.`);
    }
  }
  return out.length ? '### What memory holds now\n' + out.join('\n').slice(0, 2400) : '';
}
