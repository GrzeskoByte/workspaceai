// Facts are short statements kept as Markdown files (R-22): workspace scope in ~/.harness/ws/<id>/memory/facts.md,
// project scope in <project>/.harness/memory/facts.md. The supervisor and the memory MCP server share this format.
import type { Fact } from './types.ts';

const LINE = /^- (.*?)\s*<!--\s*(.*?)\s*-->\s*$/;

export function parseFacts(md: string, scope: string): Fact[] {
  const out: Fact[] = [];
  for (const raw of md.split('\n')) {
    const m = LINE.exec(raw.trim());
    if (!m) continue;
    const meta: Record<string, string> = {};
    for (const kv of m[2].split(/\s+/)) { const i = kv.indexOf(':'); if (i > 0) meta[kv.slice(0, i)] = kv.slice(i + 1); }
    out.push({ id: meta.id || 'f_' + out.length, text: m[1], scope, status: meta.status === 'suggested' ? 'suggested' : 'approved', source: meta.source ? decodeURIComponent(meta.source) : undefined, createdAt: Number(meta.at) || 0 });
  }
  return out;
}

export function serializeFacts(facts: Fact[], title: string): string {
  const lines = [`# ${title}`, '', '<!-- Managed by WorkspaceAI. Each line is one fact; suggestions wait for approval. -->', ''];
  for (const f of facts) {
    const meta = [`id:${f.id}`, `status:${f.status}`, `at:${f.createdAt}`];
    if (f.source) meta.push(`source:${encodeURIComponent(f.source)}`);
    lines.push(`- ${f.text.replace(/\n+/g, ' ').replace(/<!--|-->/g, '')} <!-- ${meta.join(' ')} -->`);
  }
  return lines.join('\n') + '\n';
}

/** Simple relevance search shared by the UI filter and the memory MCP `memory.search` tool. */
export function searchFacts(facts: Fact[], query: string): Fact[] {
  const terms = query.toLowerCase().split(/\W+/).filter((t) => t.length > 1);
  if (!terms.length) return facts.filter((f) => f.status === 'approved');
  return facts
    .filter((f) => f.status === 'approved')
    .map((f) => ({ f, s: terms.reduce((a, t) => a + (f.text.toLowerCase().includes(t) ? 1 : 0), 0) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s)
    .map((x) => x.f);
}
