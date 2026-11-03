// Action sheets: info commands (/status, /usage, /context, /help) answer in a sheet over the chat instead of in the
// conversation, like Claude's own apps. The agent's output is read for meters (plan usage, context window) and
// breakdowns; whatever isn't recognised is shown as written.
import { useEffect, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { Markdown } from '../ui/Markdown.tsx';
import { StatusBody, type ChatStatus } from './Commands.tsx';
import type { ChatCommand } from '../../../../packages/shared/src/commands.ts';
import { Icon } from '../ui/Icon.tsx';

export interface SheetContent { command: string; title: string; status?: ChatStatus; text?: string; ran?: string; note?: string }
export type SheetState = { title: string; command: string; agentName: string } & ({ phase: 'loading' } | { phase: 'ready'; content: SheetContent } | { phase: 'error'; error: string } | { phase: 'help'; commands: ChatCommand[] } | { phase: 'graphs'; graphs: { id: string; name: string; file: string; project: string }[] });

interface Meter { label: string; pct: number; sub?: string }
interface Ranked { heading: string; rows: { pct: number; text: string }[] }

/** What can be drawn from an agent's text output; `rest` is the text that is left. */
export function readSheet(text: string): { meters: Meter[]; ranked: Ranked[]; categories: { name: string; value: string; pct: number }[]; model?: string; rest: string; intro: string[] } {
  const meters: Meter[] = [], ranked: Ranked[] = [], categories: { name: string; value: string; pct: number }[] = [], intro: string[] = [];
  let model: string | undefined;
  const keep: string[] = [];
  const lines = text.replace(/\r/g, '').split('\n');
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    // "Current session: 67% used · resets Oct 30, 9:20pm"
    const m = /^\s*([^:\n|]{2,60}):\s*(\d+(?:\.\d+)?)%\s*used(?:\s*[·•-]\s*(.+))?\s*$/i.exec(l);
    if (m) { meters.push({ label: m[1].trim(), pct: Number(m[2]), sub: m[3]?.trim() }); continue; }
    // "**Tokens:** 24.9k / 1m (2%)"
    const t = /^\s*\**Tokens:?\**\s*:?\s*([\d.,]+\s*[kmb]?)\s*\/\s*([\d.,]+\s*[kmb]?)\s*\((\d+(?:\.\d+)?)%\)/i.exec(l);
    if (t) { meters.push({ label: 'Context window', pct: Number(t[3]), sub: `${t[1].trim()} of ${t[2].trim()} tokens` }); continue; }
    const mo = /^\s*\**Model:?\**\s*:?\s*(.+?)\s*$/i.exec(l);
    if (mo && !model) { model = mo[1].replace(/\*/g, '').trim(); continue; }
    // "Last 24h · 870 requests" followed by "  90%  Start fresh when you switch tasks"
    if (/^\S.*·/.test(l) && /^\s{2,}\d+%\s{2,}/.test(lines[i + 1] || '')) {
      const r: Ranked = { heading: l.trim(), rows: [] };
      while (i + 1 < lines.length && /^\s{2,}/.test(lines[i + 1])) {
        const row = /^\s{2,}(\d+)%\s{2,}(.+)$/.exec(lines[++i]);
        if (row) r.rows.push({ pct: Number(row[1]), text: row[2].trim() }); else r.rows.push({ pct: -1, text: lines[i].trim() });
      }
      ranked.push(r); continue;
    }
    // The "| Category | Tokens | Percentage |" table of /context.
    if (/^\|\s*Category\s*\|/i.test(l) && /^\|[\s:-]+\|/.test(lines[i + 1] || '')) {
      i += 1;
      while (i + 1 < lines.length && /^\|/.test(lines[i + 1])) {
        const c = lines[++i].split('|').map((x) => x.trim()).filter(Boolean);
        if (c.length >= 3) categories.push({ name: c[0], value: c[1], pct: parseFloat(c[2]) || 0 });
      }
      continue;
    }
    keep.push(l);
  }
  // Short lines before anything recognised read as an intro ("You are using your subscription…").
  while (keep.length && !keep[0].trim()) keep.shift();
  if (meters.length || ranked.length) while (keep.length && keep[0].trim() && !/^#|\|/.test(keep[0]) && keep[0].length < 140) intro.push(keep.shift()!.trim());
  const rest = keep.join('\n').replace(/^##\s*Context Usage\s*$/im, '').replace(/^###\s*Estimated usage by category\s*$/im, '').replace(/\n{3,}/g, '\n\n').trim();
  return { meters, ranked, categories, model, rest, intro };
}

const tone = (pct: number) => (pct >= 90 ? 'var(--error-text)' : pct >= 70 ? 'var(--warning-text)' : 'var(--accent-p)');

function Bar({ pct, color }: { pct: number; color?: string }) {
  return <span className="sheet-bar" role="presentation"><span style={{ width: Math.max(0, Math.min(100, pct)) + '%', background: color || tone(pct) }} /></span>;
}

function Section({ title, children }: { title?: string; children: ReactNode }) {
  return <section className="sheet-sec">{title && <span className="lbl">{title}</span>}{children}</section>;
}

export function ActionSheet({ sheet, onClose, onRun, onGraph }: { sheet: SheetState; onClose: () => void; onRun: (command: string) => void; onGraph?: (id: string, dryRun: boolean) => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); onClose(); } };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  const body = (() => {
    if (sheet.phase === 'loading') return <Section><span className="muted pulse">Asking {sheet.agentName} for /{sheet.command}…</span><Bar pct={35} color="var(--bg3)" /><Bar pct={60} color="var(--bg3)" /></Section>;
    if (sheet.phase === 'error') return <Section><span style={{ color: 'var(--error-text)' }}>{sheet.error}</span></Section>;
    if (sheet.phase === 'help') return (
      <Section>
        <div className="sheet-cmds">{sheet.commands.map((c) => (
          <button key={c.source + c.name} type="button" className="navrow" onClick={() => onRun('/' + c.name)}>
            <span className="mono" style={{ fontSize: 12.5 }}>/{c.name}{c.hint && <span className="muted"> {c.hint}</span>}</span>
            <span className="muted ellipsis grow" style={{ fontSize: 12, minWidth: 0 }}>{c.description}</span>
            <span className="muted" style={{ fontSize: 10.5 }}>{c.sheet ? 'sheet' : c.source === 'agent' ? sheet.agentName : ''}</span>
          </button>
        ))}</div>
      </Section>
    );
    if (sheet.phase === 'graphs') return (
      <Section>
        {!sheet.graphs.length && <span className="muted" style={{ fontSize: 12.5 }}>No graphs yet. Add a .bpmn file to the project folder, make one in Graphs, or ask the agent to create one.</span>}
        <div className="sheet-cmds">{sheet.graphs.map((g) => (
          <div key={g.id} className="row" style={{ gap: 8, padding: '6px 8px' }}>
            <span className="grow" style={{ display: 'flex', flexDirection: 'column', minWidth: 0 }}>
              <b className="ellipsis" style={{ fontSize: 13 }}>{g.name}</b>
              <span className="muted mono ellipsis" style={{ fontSize: 11 }} title={g.file}>{g.project} · {g.file.split('/').slice(-2).join('/')}</span>
            </span>
            <button type="button" className="btn" style={{ height: 28 }} onClick={() => onGraph?.(g.id, true)}>Dry run</button>
            <button type="button" className="btn btn-p" style={{ height: 28 }} onClick={() => onGraph?.(g.id, false)}>Run</button>
          </div>
        ))}</div>
      </Section>
    );
    const c = sheet.content;
    const r = c.text ? readSheet(c.text) : null;
    return <>
      {c.note && <Section><span className="muted" style={{ fontSize: 12.5 }}>{c.note}</span></Section>}
      {r && r.intro.length > 0 && <Section>{r.intro.map((l) => <span key={l} style={{ fontSize: 13 }}>{l}</span>)}</Section>}
      {r && r.meters.length > 0 && <Section title={c.command === 'context' ? undefined : 'Plan usage'}>
        {r.model && <span className="muted mono" style={{ fontSize: 11.5 }}>{r.model}</span>}
        {r.meters.map((m) => (
          <div key={m.label} className="sheet-meter">
            <span className="row"><b className="grow" style={{ fontSize: 13 }}>{m.label}</b><span className="mono" style={{ fontSize: 12.5, color: tone(m.pct) }}>{m.pct}%</span></span>
            <Bar pct={m.pct} />
            {m.sub && <span className="muted" style={{ fontSize: 11.5 }}>{m.sub}</span>}
          </div>
        ))}
      </Section>}
      {r && r.categories.length > 0 && <Section title="What fills it">
        <span className="sheet-stack">{r.categories.filter((x) => !/free|buffer/i.test(x.name) && x.pct > 0).map((x, i) => <span key={x.name} title={`${x.name} · ${x.value}`} style={{ width: x.pct + '%', background: `color-mix(in srgb, var(--accent-p) ${90 - i * 9}%, var(--bg3))` }} />)}</span>
        <div className="sheet-grid">{r.categories.map((x) => <div key={x.name} style={{ display: 'contents' }}><span>{x.name}</span><span className="mono muted">{x.value}</span><span className="mono">{x.pct}%</span></div>)}</div>
      </Section>}
      {r && r.ranked.map((g) => <Section key={g.heading} title={g.heading}>
        {g.rows.map((row) => row.pct < 0 ? <span key={row.text} className="muted" style={{ fontSize: 12 }}>{row.text}</span> : (
          <div key={row.text} className="sheet-rank"><span className="mono" style={{ fontSize: 12 }}>{row.pct}%</span><Bar pct={row.pct} color="color-mix(in srgb, var(--accent-p) 70%, var(--bg3))" /><span style={{ fontSize: 12.5 }}>{row.text}</span></div>
        ))}
      </Section>)}
      {c.status && <Section title={c.text ? 'This chat' : undefined}><StatusBody s={c.status} /></Section>}
      {r && r.rest && (r.meters.length || r.ranked.length || r.categories.length
        ? <details className="sheet-sec"><summary className="muted" style={{ cursor: 'pointer', fontSize: 12 }}>Full output of {c.ran}</summary><div style={{ marginTop: 8, fontSize: 12.5 }}><Markdown text={r.rest} /></div></details>
        : <Section><Markdown text={r.rest} /></Section>)}
    </>;
  })();
  return createPortal(
    <div className="sheet-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="sheet" role="dialog" aria-modal="true" aria-label={sheet.title}>
        <div className="sheet-grab" aria-hidden />
        <header className="row">
          <b className="grow" style={{ fontSize: 15 }}>{sheet.title}</b>
          <span className="chip mono" style={{ height: 22, fontSize: 11 }}>/{sheet.command} · {sheet.agentName}</span>
          <button type="button" className="btn" onClick={onClose} aria-label="Close" style={{ height: 28, width: 28, padding: 0, justifyContent: 'center' }}><Icon name="x" size={15} /></button>
        </header>
        <div className="sheet-body">{body}</div>
      </div>
    </div>,
    document.querySelector('.hx') || document.body,
  );
}
