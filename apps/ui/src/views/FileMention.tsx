// `@` in a composer: completes a file or folder in the project and inserts a reference the agent reads.
import { useEffect, useRef, useState, type KeyboardEvent, type RefObject } from 'react';
import { get } from '../api.ts';
import { wsId } from '../store.ts';
import { insertMention, mentionAt } from '../../../../packages/shared/src/commands.ts';
import { Icon } from '../ui/Icon.tsx';

export function useFileMention(project: string, draft: string, setDraft: (text: string) => void, input: RefObject<HTMLTextAreaElement | null>) {
  const [caret, setCaret] = useState(0);
  const [items, setItems] = useState<string[]>([]);
  const [active, setActive] = useState(0);
  const [dismissed, setDismissed] = useState(-1);
  const nextCaret = useRef<number | null>(null);
  const found = mentionAt(draft, caret);
  const mention = found && found.start !== dismissed ? found : null;
  const query = mention?.query ?? null;

  useEffect(() => {
    if (query === null) { setItems([]); return; }
    let stale = false;
    const t = setTimeout(() => {
      get<{ files: string[] }>(`/api/ws/${wsId()}/projects/${project}/files?q=${encodeURIComponent(query)}`)
        .then((r) => { if (!stale) { setItems(r.files); setActive(0); } })
        .catch(() => { if (!stale) setItems([]); });
    }, 80);
    return () => { stale = true; clearTimeout(t); };
  }, [project, query]);

  // The caret goes back where the inserted reference ends once React has written the new text.
  useEffect(() => {
    if (nextCaret.current === null) return;
    input.current?.focus();
    input.current?.setSelectionRange(nextCaret.current, nextCaret.current);
    setCaret(nextCaret.current);
    nextCaret.current = null;
  }, [draft, input]);

  /** `drill` (Tab on a folder) keeps completing inside it; otherwise the reference is finished. */
  const pick = (path: string, drill = false) => {
    if (!mention) return;
    const r = insertMention(draft, mention.start, caret, path, drill);
    nextCaret.current = r.caret;
    setDraft(r.text);
  };

  /** True when the key belonged to the menu, so the composer must not act on it too. */
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (!mention || !items.length) return false;
    const cur = items[Math.min(active, items.length - 1)];
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') setActive((i) => (i + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length);
    else if (e.key === 'Tab') pick(cur, cur.endsWith('/'));
    else if (e.key === 'Enter' && !e.shiftKey) pick(cur);
    else if (e.key === 'Escape') setDismissed(mention.start);
    else return false;
    e.preventDefault();
    e.stopPropagation();
    return true;
  };

  return {
    open: !!mention && items.length > 0,
    items,
    active: Math.min(active, Math.max(items.length - 1, 0)),
    setActive,
    pick,
    onKey,
    /** Call on every change and selection of the textarea. */
    track: (el: HTMLTextAreaElement) => { setCaret(el.selectionStart); setDismissed((d) => (found && found.start === d ? d : -1)); },
  };
}

export function FileMenu({ items, active, onHover, onPick, style }: { items: string[]; active: number; onHover: (i: number) => void; onPick: (path: string) => void; style: React.CSSProperties }) {
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  useEffect(() => { el?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' }); }, [active, el]);
  return (
    <div ref={setEl} className="menu rise cmd-menu" role="listbox" aria-label="Files and folders" style={{ maxHeight: 'min(340px, 50vh)', ...style }}>
      <span className="lbl" style={{ padding: '4px 10px' }}>Files and folders · ↵ add · ⇥ open folder · esc close</span>
      {items.map((p, i) => (
        <button key={p} type="button" role="option" aria-selected={i === active} data-active={i === active} className="navrow cmd-row" onMouseEnter={() => onHover(i)} onMouseDown={(e) => { e.preventDefault(); onPick(p); }}>
          {p.endsWith('/') && <Icon name="folder" size={13} />}
          <span className="mono ellipsis" style={{ fontSize: 12.5, minWidth: 0 }}>{p}</span>
        </button>
      ))}
    </div>
  );
}
