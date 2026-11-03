// Maps opencode v2 session messages to the harness chat model.
import type { ChatMessage, ToolStep } from './types.ts';

interface OcPart { type: string; text?: string; name?: string; id?: string; state?: { status?: string; input?: unknown; content?: { type: string; text?: string }[]; error?: string } }

export function toolLabel(name: string, input: unknown): string {
  const i = (input && typeof input === 'object' ? input : {}) as Record<string, unknown>;
  // opencode 2.x code mode: one `execute` call runs code that calls tools as tools["server"].name(...)
  if (typeof i.code === 'string') {
    const calls = Array.from(new Set(Array.from(i.code.matchAll(/tools(?:\[["'`]([\w.-]+)["'`]\]|\.([\w-]+))\s*(?:\.\s*([\w-]+)|\[["'`]([\w.-]+)["'`]\])/g)).map((m) => `${m[1] || m[2]} · ${m[3] || m[4]}`)));
    if (calls.length) return calls.join(', ');
  }
  const arg = (i.path || i.filePath || i.url || i.query || i.command || i.pattern || '') as string;
  const short = typeof arg === 'string' ? arg.split('/').slice(-2).join('/') : '';
  return short ? `${name} ${short}` : name;
}

export function mapOcMessages(raw: Record<string, unknown>[]): ChatMessage[] {
  const chrono = raw.slice().sort((a, b) => Number((a.time as { created?: number })?.created ?? 0) - Number((b.time as { created?: number })?.created ?? 0));
  const out: ChatMessage[] = [];
  for (const m of chrono) {
    const created = Number((m.time as { created?: number })?.created ?? Date.now());
    if (m.type === 'user') {
      out.push({ id: String(m.id), role: 'user', text: String(m.text ?? (m.payload as { text?: string })?.text ?? ''), createdAt: created });
    } else if (m.type === 'assistant') {
      const parts = (m.content as OcPart[]) || [];
      const text = parts.filter((p) => p.type === 'text').map((p) => p.text || '').join('\n\n');
      const tools: ToolStep[] = parts.filter((p) => p.type === 'tool').map((p) => ({
        name: toolLabel(p.name || 'tool', p.state?.input),
        status: p.state?.status === 'error' || p.state?.status === 'failed' ? 'error' : p.state?.status === 'completed' ? 'ok' : 'running',
        input: p.state?.input ? JSON.stringify(p.state.input) : undefined,
        output: (p.state?.content || []).map((c) => c.text || '').join('\n').slice(0, 2000) || p.state?.error,
      }));
      const model = m.model as { id?: string; providerID?: string } | undefined;
      // Merge consecutive assistant steps of one turn into one bubble, like the TUI does.
      const prev = out[out.length - 1];
      if (prev && prev.role === 'assistant' && !prev.error) {
        prev.text = [prev.text, text].filter(Boolean).join('\n\n');
        prev.tools = [...(prev.tools || []), ...tools];
      } else {
        out.push({ id: String(m.id), role: 'assistant', text, tools, createdAt: created, model: model ? `${model.providerID}/${model.id}` : undefined, error: m.error ? String((m.error as { message?: string }).message ?? m.error) : undefined });
      }
    }
  }
  return out;
}

/** Pages the agent read through web tools, listed under the reply (R-21). */
export function sourcesFromTools(tools: ToolStep[] = []): { title: string; url: string }[] {
  const out: { title: string; url: string }[] = [];
  for (const t of tools) {
    if (!/^(webfetch|websearch|\w+_search|\w+_fetch)/.test(t.name)) continue;
    for (const m of (t.output || '').matchAll(/https?:\/\/[^\s)"'\]]+/g)) if (!out.some((o) => o.url === m[0])) out.push({ title: m[0].replace(/^https?:\/\//, '').slice(0, 60), url: m[0] });
    try { const i = JSON.parse(t.input || '{}'); if (typeof i.url === 'string' && !out.some((o) => o.url === i.url)) out.push({ title: i.url.replace(/^https?:\/\//, '').slice(0, 60), url: i.url }); } catch { /* not json */ }
  }
  return out.slice(0, 12);
}
