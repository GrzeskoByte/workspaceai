// Composer pieces shared by Home and Chat: project picker, model + effort picker, web chip.
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from 'react';
import { createPortal } from 'react-dom';
import { Command } from 'cmdk';
import { useStore, type AgentModels } from '../store.ts';
import { flatProjects, projectPathName } from '../../../../packages/shared/src/util.ts';
import { Icon } from '../ui/Icon.tsx';

export function useOutside<T extends HTMLElement>(onOut: () => void) {
  const ref = useRef<T>(null);
  useEffect(() => {
    const f = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) onOut(); };
    const k = (e: KeyboardEvent) => { if (e.key === 'Escape') onOut(); };
    document.addEventListener('mousedown', f);
    document.addEventListener('keydown', k);
    return () => { document.removeEventListener('mousedown', f); document.removeEventListener('keydown', k); };
  }, [onOut]);
  return ref;
}

/**
 * Where a dropdown sits: fixed to the window under its anchor (or above it with `up`, or when there is more room
 * there), clamped inside the window. Being fixed, it is never clipped by the scrolling page or covered by the sidebar.
 */
export function useAnchored(open: boolean, anchor: RefObject<HTMLElement | null>, width: number, o: { up?: boolean; align?: 'left' | 'right'; maxHeight?: number } = {}): CSSProperties {
  const [st, setSt] = useState<CSSProperties>({ visibility: 'hidden' });
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const a = anchor.current; if (!a) return;
      const r = a.getBoundingClientRect(), vw = window.innerWidth, vh = window.innerHeight, gap = 6, pad = 12;
      const w = Math.min(width, vw - pad * 2);
      const left = Math.max(pad, Math.min((o.align === 'right' ? r.right - w : r.left), vw - pad - w));
      const below = vh - r.bottom - gap - pad, above = r.top - gap - pad, want = o.maxHeight ?? 420;
      const up = o.up ? !(above < 160 && below > above) : below < Math.min(want, 240) && above > below;
      const maxHeight = Math.min(want, up ? above : below);
      setSt({ position: 'fixed', left, width: w, maxHeight, ...(up ? { bottom: vh - r.top + gap } : { top: r.bottom + gap }) });
    };
    place();
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    return () => { window.removeEventListener('resize', place); window.removeEventListener('scroll', place, true); };
  }, [open, anchor, width, o.up, o.align, o.maxHeight]);
  return st;
}

export function ProjectPicker({ value, onPick, label = 'in' }: { value: string; onPick: (id: string) => void; label?: string }) {
  const snap = useStore((s) => s.snap)!;
  const [open, setOpen] = useState(false);
  const ref = useOutside<HTMLSpanElement>(() => setOpen(false));
  const pos = useAnchored(open, ref, 320, { maxHeight: 360 });
  return (
    <span ref={ref} style={{ position: 'relative' }}>
      <button type="button" className="chip" onClick={() => setOpen(!open)} aria-expanded={open} aria-haspopup="listbox" style={{ borderColor: 'var(--accent-p)' }}><Icon name="folder" size={13} /> {label} {projectPathName(snap.projects, value) || 'General'} <Icon name="down" size={12} /></button>
      {open && (
        <div className="menu rise menu-sheet" role="listbox" aria-label="Project" style={pos}>
          <span className="muted" style={{ fontSize: 11, padding: '4px 8px' }}>Every chat needs a project</span>
          {flatProjects(snap.projects).map(({ project: p, depth }) => (
            <button key={p.id} type="button" className="navrow" role="option" aria-selected={p.id === value} aria-current={p.id === value ? 'true' : 'false'} onClick={() => { onPick(p.id); setOpen(false); }} style={{ paddingLeft: 8 + depth * 16 }}>▤ {p.name}{p.isGeneral && <span className="muted" style={{ marginLeft: 'auto', fontSize: 11 }}>default</span>}</button>
          ))}
        </div>
      )}
    </span>
  );
}

/**
 * The model a chat runs on. opencode chats use the pick or the project/workspace default; an ACP agent
 * (Claude Code, Grok) only runs models its own subscription offers, so anything else falls back to its default.
 */
export function pickModel(agentModels: Record<string, AgentModels>, agent: string, picked: string, opencodeDefault: string): string {
  if (agent === 'opencode') return picked || opencodeDefault;
  const am = agentModels[agent];
  if (!am?.models.length) return picked;
  return am.models.some((m) => m.id === picked) ? picked : am.current || am.models[0].id;
}

const EFFORT_LABEL: Record<string, string> = { none: 'None', minimal: 'Minimal', low: 'Low', medium: 'Medium', high: 'High', xhigh: 'Extra high', max: 'Max' };
export const effortLabel = (e: string) => EFFORT_LABEL[e] || e.charAt(0).toUpperCase() + e.slice(1);

interface PickItem { agent: string; id: string; name: string; description?: string; efforts?: string[]; defaultEffort?: string; missing?: boolean }
interface PickGroup { key: string; title: string; note: string; items: PickItem[]; empty?: string }

/** Everything the picker can offer: a section per subscription agent, then a section per connected opencode provider. */
function usePickGroups(agent: string, model: string, lockAgent?: boolean): PickGroup[] {
  const snap = useStore((s) => s.snap)!;
  const modelGroups = useStore((s) => s.modelGroups);
  const agentModels = useStore((s) => s.agentModels);
  const ws = snap.workspace;
  const groups: PickGroup[] = [];
  for (const a of ws.agents.filter((x) => x.on && x.id !== 'opencode' && (!lockAgent || x.id === agent))) {
    const am = agentModels[a.id];
    groups.push({
      key: a.id, title: a.name + (a.subscription ? ' subscription' : ''), note: 'runs in ' + a.name,
      items: (am?.models || []).map((m) => ({ agent: a.id, ...m })),
      empty: am ? `${a.name} reported no models` : `Asking ${a.name} which models your plan offers…`,
    });
  }
  if (!lockAgent || agent === 'opencode') {
    for (const g of modelGroups) groups.push({ key: 'oc:' + g.provider, title: g.name, note: 'opencode · ' + g.note, items: g.models.map((m) => ({ agent: 'opencode', ...m })) });
    // A chat still set to a model that is no longer connected keeps showing it, marked, until another is picked.
    if (agent === 'opencode' && model && !modelGroups.some((g) => g.models.some((m) => m.id === model))) {
      groups.push({ key: 'oc:missing', title: 'Not connected', note: 'pick another model', items: [{ agent: 'opencode', id: model, name: model.split('/').pop() || model, description: 'Its provider is not connected', missing: true }] });
    }
    if (!modelGroups.length) groups.push({ key: 'oc', title: 'opencode', note: '', items: [], empty: 'No connected API provider — add one in Settings › Model providers.' });
  }
  return groups;
}

/**
 * The composer's model chip. It opens a dialog to search every model the workspace can use — subscription agents
 * (Claude Code, Grok) and connected opencode providers — and pick its effort level. Picking a model picks its agent too.
 * `lockAgent` (a chat that already has messages) shows only the chat's own agent, since a chat keeps its agent.
 */
export function ModelPicker({ agent, model, effort, onPick, lockAgent, open: openProp, onOpenChange }: { agent: string; model: string; effort: string; onPick: (agent: string, model: string, effort: string) => void; lockAgent?: boolean; open?: boolean; onOpenChange?: (open: boolean) => void }) {
  const snap = useStore((s) => s.snap)!;
  const [openSelf, setOpenSelf] = useState(false);
  // Controlled when the parent passes `open` (the /model and /effort commands open it).
  const open = openProp ?? openSelf;
  const setOpen = (o: boolean) => { setOpenSelf(o); onOpenChange?.(o); };
  const groups = usePickGroups(agent, model, lockAgent);
  const ws = snap.workspace;
  const all = groups.flatMap((g) => g.items);
  const cur = all.find((m) => m.agent === agent && m.id === model);
  const source = agent === 'opencode' ? (groups.find((g) => g.items.includes(cur as PickItem))?.title || 'opencode') : ws.agents.find((a) => a.id === agent)?.name || agent;
  const label = cur ? cur.name : model ? model.split('/').pop() : agent === 'opencode' ? 'no model' : 'loading models…';
  return (
    <>
      <button type="button" className="chip mono model-chip" onClick={() => setOpen(true)} aria-haspopup="dialog" title="Choose model and effort" style={{ fontSize: 11 }}>
        <Icon name="sparkle" size={13} /> <span className="ellipsis">{source} · {label}{effort ? ' · ' + effortLabel(effort) : ''}</span> <Icon name="down" size={12} />
      </button>
      {open && <ModelDialog groups={groups} agent={agent} model={model} effort={effort} lockAgent={lockAgent} onClose={() => setOpen(false)} onPick={(a, m, e) => { onPick(a, m, e); setOpen(false); }} />}
    </>
  );
}

function ModelDialog({ groups, agent, model, effort, lockAgent, onClose, onPick }: { groups: PickGroup[]; agent: string; model: string; effort: string; lockAgent?: boolean; onClose: () => void; onPick: (agent: string, model: string, effort: string) => void }) {
  const key = (m: { agent: string; id: string }) => m.agent + '\n' + m.id;
  const all = groups.flatMap((g) => g.items);
  const [sel, setSel] = useState<string>(key({ agent, id: model }));
  const [eff, setEff] = useState(effort);
  const [hl, setHl] = useState<string>(key({ agent, id: model }));
  const picked = all.find((m) => key(m) === sel);
  const efforts = picked?.efforts || [];
  const effOk = !eff || efforts.includes(eff);
  const choose = (m: PickItem) => { setSel(key(m)); if (eff && !(m.efforts || []).includes(eff)) setEff(''); };
  const use = () => { if (picked && !picked.missing) onPick(picked.agent, picked.id, effOk ? eff : ''); };
  return createPortal(
    <div className="scrim model-scrim" onMouseDown={onClose} style={{ position: 'fixed', zIndex: 80 }}>
      <Command label="Choose a model" className="card rise model-dialog" value={hl} onValueChange={setHl} loop
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === 'Escape') { e.preventDefault(); onClose(); }
          // ⌘/Ctrl+Enter uses the selection; plain Enter selects the highlighted model (twice = use it).
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); use(); }
        }}
        filter={(value, search, keywords) => {
          const q = search.toLowerCase().trim(); if (!q) return 1;
          const hay = (value + ' ' + (keywords || []).join(' ')).toLowerCase();
          return q.split(/\s+/).every((w) => hay.includes(w)) ? 1 : 0;
        }}>
        <div className="row" style={{ padding: '14px 16px 0', gap: 10 }}>
          <b style={{ fontSize: 15 }} className="grow">Choose a model</b>
          <button type="button" className="btn" onClick={onClose} aria-label="Close" style={{ height: 28, width: 28, padding: 0, justifyContent: 'center' }}><Icon name="x" size={15} /></button>
        </div>
        <Command.Input placeholder="Search models, providers, subscriptions…" autoFocus />
        <Command.List className="model-list">
          <Command.Empty>No model matches.</Command.Empty>
          {groups.map((g) => (
            <Command.Group key={g.key} heading={<span className="row" style={{ gap: 8 }}><span className="grow">{g.title}</span><span className="mono" style={{ textTransform: 'none', letterSpacing: 0 }}>{g.note}</span></span>}>
              {g.items.map((m) => (
                <Command.Item key={key(m)} value={key(m)} keywords={[m.name, m.id, m.description || '', g.title]} onSelect={() => (sel === key(m) ? use() : choose(m))}
                  onDoubleClick={() => { choose(m); if (!m.missing) onPick(m.agent, m.id, eff && (m.efforts || []).includes(eff) ? eff : ''); }}
                  data-picked={sel === key(m) ? 'true' : undefined}>
                  <span className="model-radio" aria-hidden>{sel === key(m) ? '●' : '○'}</span>
                  <span style={{ display: 'flex', flexDirection: 'column', minWidth: 0, flex: '1 1 auto' }}>
                    <span className="row" style={{ gap: 8 }}><b className="ellipsis">{m.name}</b>{m.agent === agent && m.id === model && <span className="muted" style={{ fontSize: 11 }}>current</span>}</span>
                    {m.description && <span className="muted ellipsis" style={{ fontSize: 12 }}>{m.description}</span>}
                  </span>
                  <span className="mono muted model-meta" style={{ fontSize: 10.5, textAlign: 'right' }}>{m.efforts?.length ? m.efforts.length + ' effort levels' : ''}<br />{m.id.includes('/') ? m.id.split('/').slice(1).join('/') : m.id}</span>
                </Command.Item>
              ))}
              {!g.items.length && g.empty && <div className="muted" style={{ fontSize: 12, padding: '4px 12px 8px' }}>{g.empty}</div>}
            </Command.Group>
          ))}
        </Command.List>
        <div className="model-foot">
          <div className="row" style={{ gap: 8, flexWrap: 'wrap' }}>
            <span className="lbl" style={{ minWidth: 52 }}>Effort</span>
            {efforts.length ? (
              <span className="row" role="radiogroup" aria-label="Effort" style={{ gap: 6, flexWrap: 'wrap' }}>
                <button type="button" className="chip" role="radio" aria-checked={!eff} aria-pressed={!eff} onClick={() => setEff('')}>Default{picked?.defaultEffort ? ` (${effortLabel(picked.defaultEffort)})` : ''}</button>
                {efforts.map((e) => <button key={e} type="button" className="chip" role="radio" aria-checked={eff === e} aria-pressed={eff === e} onClick={() => setEff(e)}>{effortLabel(e)}</button>)}
              </span>
            ) : <span className="muted" style={{ fontSize: 12 }}>{picked ? `${picked.name} has no effort setting` : 'Pick a model'}</span>}
          </div>
          <div className="row" style={{ gap: 8 }}>
            <span className="mono muted model-keys" style={{ fontSize: 11 }}>↑↓ move · ↵ select · ⌘↵ use · esc close</span>
            {lockAgent && <span className="muted" style={{ fontSize: 11.5 }}>A chat keeps its agent — start a new chat for another.</span>}
            <span className="grow" />
            <button type="button" className="btn" onClick={onClose}>Cancel</button>
            <button type="button" className="btn btn-p" onClick={use} disabled={!picked || picked.missing}>Use {picked ? picked.name : 'model'}{effOk && eff ? ' · ' + effortLabel(eff) : ''}</button>
          </div>
        </div>
      </Command>
    </div>,
    // Inside the themed app root (`.hx`), which carries the colour and font variables, yet above every layout layer.
    document.querySelector('.hx') || document.body,
  );
}
