// Slash commands in the chat composer: the menu that completes them, and the cards some of them answer with.
import { useEffect, useState, type ReactNode } from 'react';
import type { ChatCommand } from '../../../../packages/shared/src/commands.ts';
import { effortLabel } from './Composer.tsx';
import { Icon } from '../ui/Icon.tsx';

/** What `/status` reports (the supervisor's `ChatStatus`). */
export interface ChatStatus {
  agent: string; agentName: string; model: string; effort: string; project: string; folder: string; session: string;
  web: boolean; fullAccess: boolean; connectors: number; engine: string;
  tokens?: { input: number; output: number; reasoning: number; cacheRead: number; cacheWrite: number }; cost?: number;
}

/** A card the harness adds under the conversation; it is not part of the session and goes away on reload. */
export type ChatNote = { id: string; at: number } & ({ kind: 'status'; status: ChatStatus; ran?: string } | { kind: 'help'; commands: ChatCommand[]; agentName: string });

/** Commands matching what is typed after `/`, harness ones first. */
export function matchCommands(all: ChatCommand[], draft: string): ChatCommand[] {
  const q = draft.slice(1).split(/\s/)[0].toLowerCase();
  const starts = all.filter((c) => c.name.toLowerCase().startsWith(q));
  // Descriptions only count once the query is long enough to mean something ("/st" should not match "cost").
  const contains = all.filter((c) => !starts.includes(c) && (c.name.toLowerCase().includes(q) || (q.length >= 3 && c.description.toLowerCase().includes(q))));
  return [...starts, ...contains].slice(0, 60);
}

export function CommandMenu({ items, active, agentName, onHover, onPick }: { items: ChatCommand[]; active: number; agentName: string; onHover: (i: number) => void; onPick: (c: ChatCommand) => void }) {
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  useEffect(() => { el?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' }); }, [active, el]);
  if (!items.length) return null;
  const firstAgent = items.findIndex((c) => c.source === 'agent');
  return (
    <div ref={setEl} className="menu rise cmd-menu" role="listbox" aria-label="Commands" style={{ left: 28, right: 28, bottom: '100%', marginBottom: 6, maxHeight: 'min(340px, 50vh)' }}>
      {items.map((c, i) => (
        <div key={c.source + c.name} style={{ display: 'contents' }}>
          {i === 0 && c.source === 'harness' && <span className="lbl" style={{ padding: '4px 10px' }}>Commands · run as {agentName}’s own</span>}
          {i === firstAgent && <span className="lbl" style={{ padding: '8px 10px 4px' }}>{agentName} commands</span>}
          <button type="button" role="option" aria-selected={i === active} data-active={i === active} className="navrow cmd-row" onMouseEnter={() => onHover(i)} onMouseDown={(e) => { e.preventDefault(); onPick(c); }}>
            <span className="mono" style={{ fontSize: 12.5, flex: '0 0 auto' }}>/{c.name}{c.hint && <span className="muted"> {c.hint}</span>}</span>
            <span className="muted ellipsis" style={{ fontSize: 12, flex: '1 1 auto', minWidth: 0 }}>{c.description}</span>
            {c.runsAs && <span className="mono muted cmd-runs" style={{ fontSize: 10.5, flex: '0 0 auto' }}>→ {c.runsAs}</span>}
          </button>
        </div>
      ))}
    </div>
  );
}

function Row({ k, v }: { k: string; v: ReactNode }) {
  return <><span className="muted">{k}</span><span className="ellipsis" style={{ minWidth: 0 }}>{v}</span></>;
}

const n = (x: number) => x.toLocaleString();

export function NoteCard({ note, onClose }: { note: ChatNote; onClose: () => void }) {
  return (
    <div className="card rise" role="status" style={{ padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 10, borderStyle: 'dashed' }}>
      <div className="row" style={{ gap: 8 }}>
        <b className="grow" style={{ fontSize: 13 }}>{note.kind === 'status' ? `/status · ${note.status.agentName}` : `/help · commands in this chat`}</b>
        <span className="muted" style={{ fontSize: 11 }}>only you see this</span>
        <button type="button" className="link" aria-label="Dismiss" onClick={onClose} style={{ display: 'inline-flex' }}><Icon name="x" size={14} /></button>
      </div>
      {note.kind === 'status' ? <StatusBody s={note.status} ran={note.ran} /> : (
        <div style={{ display: 'grid', gridTemplateColumns: 'auto 1fr', gap: '4px 14px', fontSize: 12.5 }}>
          {note.commands.map((c) => (
            <div key={c.source + c.name} style={{ display: 'contents' }}>
              <span className="mono">/{c.name}</span>
              <span className="muted">{c.description}{c.runsAs ? <span className="mono" style={{ fontSize: 11 }}> → {c.runsAs}</span> : c.source === 'agent' ? <span style={{ fontSize: 11 }}> · {note.agentName}</span> : null}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function StatusBody({ s, ran }: { s: ChatStatus; ran?: string }) {
  return (
    <>
      <div className="status-grid" style={{ display: 'grid', gridTemplateColumns: 'auto 1fr', gap: '4px 14px', fontSize: 12.5 }}>
        <Row k="Agent" v={s.agentName} />
        <Row k="Model" v={<span className="mono">{s.model || 'agent default'}</span>} />
        <Row k="Effort" v={s.effort ? effortLabel(s.effort) : 'model default'} />
        <Row k="Project" v={s.project} />
        <Row k="Folder" v={<span className="mono">{s.folder}</span>} />
        <Row k="Session" v={<span className="mono">{s.session}</span>} />
        <Row k="Permissions" v={s.fullAccess ? 'Full access — tools run without asking' : 'Ask first'} />
        <Row k="Web search" v={s.web ? 'On' : 'Off'} />
        <Row k="Connectors" v={s.connectors + ' on'} />
        {s.agent === 'opencode' && <Row k="Engine" v={s.engine} />}
        {s.tokens && <Row k="Tokens" v={`${n(s.tokens.input)} in · ${n(s.tokens.output)} out${s.tokens.reasoning ? ' · ' + n(s.tokens.reasoning) + ' reasoning' : ''}${s.tokens.cacheRead ? ' · ' + n(s.tokens.cacheRead) + ' cached' : ''}`} />}
        {s.cost !== undefined && <Row k="Cost" v={'$' + s.cost.toFixed(4)} />}
      </div>
      {ran && <span className="muted" style={{ fontSize: 12 }}>Also asked {s.agentName} for its own status with <span className="mono">{ran}</span> — its answer follows.</span>}
    </>
  );
}
