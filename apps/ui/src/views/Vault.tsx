// Obsidian vaults: browse the workspace's notes and edit them in place, saving as you type.
import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from 'react';
import { act, go, setState, toast, useStore, wsId } from '../store.ts';
import { ApiError, del, get, post, put } from '../api.ts';
import type { Vault } from '../../../../packages/shared/src/types.ts';
import { Icon } from '../ui/Icon.tsx';
import { createNoteEditor, type Format, type NoteEditor } from './vaultEditor.ts';

interface Tree { dirs: string[]; files: { path: string; mtime: number; size: number }[]; truncated: boolean }
type Note = { path: string; text: string; hash: string } | { path: string; readOnly: true; reason: string; hash?: string } | { same: true };
interface Hit { vault: string; path: string; line: number; text: string }

const url = (vault: string) => `/api/ws/${wsId()}/vaults/${vault}/file`;
const withPath = (vault: string, path: string, extra = '') => `${url(vault)}?path=${encodeURIComponent(path)}${extra}`;
const asNote = (name: string) => (/\.md$/i.test(name.trim()) ? name.trim() : name.trim() + '.md');
/** Typed text a save couldn't write when the note closed; it comes back when the note opens again. */
const drafts = new Map<string, { text: string; hash: string | null }>();

const TOOLS: [Format, string][] = [['bold', 'Bold'], ['italic', 'Italic'], ['heading', 'Heading'], ['list', 'List'], ['task', 'Task'], ['link', 'Link']];

function NoteView({ vault, path, flushRef, onWiki }: { vault: string; path: string; flushRef: RefObject<(() => Promise<boolean>) | null>; onWiki: (target: string) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const ed = useRef<NoteEditor | null>(null);
  const hash = useRef<string | null>(null);
  const dirty = useRef(false);
  const inflight = useRef<Promise<boolean> | null>(null);
  const conflictNow = useRef<null | 'changed' | 'deleted'>(null);
  const timer = useRef<number | undefined>(undefined);
  const retry = useRef<number | undefined>(undefined);
  const tick = useStore((s) => s.vaultTick);
  const [status, setStatus] = useState<'' | 'saving' | 'saved' | 'failed'>('');
  const [conflict, setConflictState] = useState<null | 'changed' | 'deleted'>(null);
  const [blocked, setBlocked] = useState<string | null>(null);
  const setConflict = (c: null | 'changed' | 'deleted') => { conflictNow.current = c; setConflictState(c); };
  const key = vault + '\0' + path;
  const wiki = useRef(onWiki);
  wiki.current = onWiki; // the editor outlives renders; links must resolve against the current note list

  const save = useCallback((force = false): Promise<boolean> => {
    clearTimeout(timer.current); clearTimeout(retry.current);
    if (inflight.current) return inflight.current.then(() => (dirty.current ? save(force) : true));
    if (!dirty.current && !force) return Promise.resolve(true);
    if (conflictNow.current && !force) return Promise.resolve(false);
    const text = ed.current?.getText();
    if (text === undefined) return Promise.resolve(false);
    dirty.current = false;
    setStatus('saving');
    const run = put<{ hash: string }>(url(vault), { path, text, hash: hash.current, force }).then((r) => {
      hash.current = r.hash;
      setConflict(null);
      setStatus(dirty.current ? 'saving' : 'saved');
      return true;
    }).catch((e: unknown) => {
      dirty.current = true;
      if (e instanceof ApiError && e.status === 409) { setConflict(/deleted/.test(e.message) ? 'deleted' : 'changed'); setStatus(''); return false; }
      setStatus('failed');
      retry.current = window.setTimeout(() => void save(), 5000);
      return false;
    }).finally(() => { inflight.current = null; });
    inflight.current = run;
    return run.then((ok) => (ok && dirty.current ? save() : ok));
  }, [vault, path]);

  // A note changed on disk: Obsidian, sync, or an agent. Never touches text the user hasn't saved.
  const poll = useCallback(async () => {
    if (!ed.current || dirty.current || inflight.current || conflictNow.current || document.hidden) return;
    try {
      const r = await get<Note>(withPath(vault, path, hash.current ? `&known=${hash.current}` : ''));
      if (!ed.current || dirty.current || inflight.current || 'same' in r) return;
      if ('readOnly' in r) { setBlocked(r.reason); return; }
      setBlocked(null);
      ed.current.setText(r.text);
      hash.current = r.hash;
    } catch (e) { if (e instanceof ApiError && e.status === 404) setConflict('deleted'); }
  }, [vault, path]);

  useEffect(() => {
    let closed = false;
    const open = (text: string, h: string | null, readOnly = false) => {
      if (closed || !host.current) return;
      hash.current = h;
      ed.current = createNoteEditor({ parent: host.current, text, readOnly, onWiki: (t) => wiki.current(t), onChange: () => {
        dirty.current = true;
        setStatus(conflictNow.current ? '' : 'saving');
        clearTimeout(timer.current);
        timer.current = window.setTimeout(() => void save(), 800);
      } });
      if (!ed.current.faithful) setBlocked('This note can’t be edited here without changing it. Open it in Obsidian.');
    };
    const draft = drafts.get(key);
    drafts.delete(key);
    get<Note>(withPath(vault, path)).then((r) => {
      if ('same' in r) return;
      if ('readOnly' in r) { if (!closed) setBlocked(r.reason); return; }
      if (!draft) { open(r.text, r.hash); return; }
      open(draft.text, draft.hash);
      dirty.current = draft.text !== r.text;
      if (dirty.current) void save();
    }).catch((e: unknown) => {
      if (closed) return;
      if (draft && e instanceof ApiError && e.status === 404) { open(draft.text, draft.hash); dirty.current = true; setConflict('deleted'); return; }
      setBlocked((e as Error).message);
    });
    flushRef.current = () => save();
    const interval = window.setInterval(() => void poll(), 3000);
    const onFocus = () => void poll();
    const onHide = () => { if (dirty.current) void save(); };
    const onVisible = () => { if (document.hidden) onHide(); };
    const warn = (e: BeforeUnloadEvent) => { if (dirty.current || inflight.current) e.preventDefault(); };
    window.addEventListener('focus', onFocus);
    window.addEventListener('pagehide', onHide);
    window.addEventListener('beforeunload', warn);
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      closed = true;
      flushRef.current = null;
      clearInterval(interval); clearTimeout(timer.current); clearTimeout(retry.current);
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('pagehide', onHide);
      window.removeEventListener('beforeunload', warn);
      document.removeEventListener('visibilitychange', onVisible);
      const editor = ed.current;
      ed.current = null;
      if (!editor) return;
      const text = editor.getText();
      const unsaved = dirty.current;
      const conflicted = !!conflictNow.current;
      const pending = inflight.current;
      editor.destroy();
      if (!unsaved && !pending) return;
      // Leaving mid-save: finish it, and keep the text if it can't be written.
      const keep = (h: string | null) => { drafts.set(key, { text, hash: h }); toast(`${path} isn’t saved yet. Open it again to keep your changes.`, 'error'); };
      if (conflicted) { keep(hash.current); return; }
      void (pending || Promise.resolve(true)).then((ok) => {
        if (ok && !unsaved) return;
        put<{ hash: string }>(url(vault), { path, text, hash: hash.current }).catch(() => keep(hash.current));
      });
    };
  }, [key]);

  useEffect(() => { void poll(); }, [tick]);

  const reload = async () => {
    dirty.current = false;
    setConflict(null);
    await poll();
    setStatus('');
  };
  const overwrite = () => { dirty.current = true; void save(true); };

  return (
    <div className="vault-note">
      <div className="row vault-tools" role="toolbar" aria-label="Format">
        {TOOLS.map(([k, label]) => <button key={k} type="button" className="chip" disabled={!!blocked} onMouseDown={(e) => { e.preventDefault(); ed.current?.format(k); }}>{label}</button>)}
        <span className="grow" />
        <span className="muted" role="status" style={{ fontSize: 12 }}>{status === 'saving' ? 'Saving…' : status === 'saved' ? 'Saved' : ''}</span>
      </div>
      {status === 'failed' && <div className="vault-banner" role="alert"><Icon name="alert" size={14} /> Not saved. Your text is safe here; retrying every few seconds.</div>}
      {conflict === 'changed' && <div className="vault-banner" role="alert"><Icon name="alert" size={14} /><span className="grow">This note changed outside Harness while you were editing.</span><button type="button" className="btn" onClick={() => void reload()}>Reload, discard mine</button><button type="button" className="btn" onClick={overwrite}>Overwrite with mine</button></div>}
      {conflict === 'deleted' && <div className="vault-banner" role="alert"><Icon name="alert" size={14} /><span className="grow">This note was deleted outside Harness. Your text is still here.</span><button type="button" className="btn" onClick={overwrite}>Save it again</button></div>}
      {blocked && <p className="muted" style={{ padding: 16 }}>{blocked}</p>}
      <div ref={host} className="vault-editor" style={blocked ? { display: 'none' } : undefined} />
    </div>
  );
}

export function VaultView() {
  const snap = useStore((s) => s.snap)!;
  const opened = useStore((s) => s.vaultOpen);
  const tick = useStore((s) => s.vaultTick);
  const vaults: Vault[] = snap.workspace.vaults || [];
  const [vaultId, setVaultId] = useState(opened?.vault || vaults[0]?.id || '');
  const vault = vaults.find((v) => v.id === vaultId) || vaults[0];
  const [tree, setTree] = useState<Tree | null>(null);
  const [filter, setFilter] = useState('');
  const [hits, setHits] = useState<{ hits: Hit[]; truncated: boolean } | null>(null);
  const [closed, setClosed] = useState<Record<string, boolean>>({});
  const [renaming, setRenaming] = useState<string | null>(null);
  const flush = useRef<(() => Promise<boolean>) | null>(null);

  const load = useCallback(() => {
    if (!vault) return;
    get<Tree>(`/api/ws/${wsId()}/vaults/${vault.id}/tree`).then(setTree).catch((e) => { setTree(null); toast(e.message, 'error'); });
  }, [vault?.id]);
  useEffect(load, [load, tick]);
  useEffect(() => { window.addEventListener('focus', load); return () => window.removeEventListener('focus', load); }, [load]);
  useEffect(() => { if (opened) setVaultId(opened.vault); }, [opened?.vault]);

  const open = async (v: string, path: string) => {
    if (flush.current && !(await flush.current())) { toast('Resolve this note first: it isn’t saved yet', 'error'); return; }
    setState({ vaultOpen: { vault: v, path } });
    setRenaming(null);
  };
  const create = async (name: string) => {
    if (!vault) return;
    const path = asNote(name);
    const r = await act(() => put(url(vault.id), { path, text: '', hash: null }));
    if (r) { load(); await open(vault.id, path); }
  };
  const onWiki = (target: string) => {
    if (!vault) return;
    const name = target.split('#')[0].trim();
    if (!name) return;
    const wanted = asNote(name).toLowerCase();
    const found = tree?.files.find((f) => f.path.toLowerCase() === wanted) || tree?.files.find((f) => f.path.split('/').pop()?.toLowerCase() === wanted.split('/').pop());
    if (found) { void open(vault.id, found.path); return; }
    setState({ confirm: { title: `Create ${asNote(name)}?`, body: 'No note in this vault has that name yet.', ok: 'Create', run: () => create(name) } });
  };
  const rename = async (to: string) => {
    if (!vault || !opened) return;
    const target = asNote(to);
    if (target === opened.path) { setRenaming(null); return; }
    if (flush.current && !(await flush.current())) { toast('Resolve this note first: it isn’t saved yet', 'error'); return; }
    const r = await act(() => post<{ path: string }>(`/api/ws/${wsId()}/vaults/${vault.id}/move`, { path: opened.path, to: target }), 'Renamed. Links to it aren’t updated.');
    if (r) { setState({ vaultOpen: { vault: vault.id, path: r.path } }); setRenaming(null); load(); }
  };
  const remove = () => {
    if (!vault || !opened) return;
    const { path } = opened;
    setState({ confirm: { title: `Delete ${path}?`, body: 'It moves to this vault’s .trash folder. Restore it from there in Obsidian or your file manager.', ok: 'Delete', run: async () => {
      const r = await act(() => del(withPath(vault.id, path)), 'Moved to .trash');
      if (r) { setState({ vaultOpen: null }); load(); }
    } } });
  };
  const search = () => {
    if (!filter.trim()) { setHits(null); return; }
    void act(() => get<{ hits: Hit[]; truncated: boolean }>(`/api/ws/${wsId()}/vaults/search?q=${encodeURIComponent(filter)}`).then(setHits));
  };

  const rows = useMemo(() => {
    if (!tree) return [];
    const q = filter.trim().toLowerCase();
    if (q) return tree.files.filter((f) => f.path.toLowerCase().includes(q)).map((f) => ({ path: f.path, dir: false, depth: 0, label: f.path }));
    const all = [...tree.dirs.map((path) => ({ path, dir: true })), ...tree.files.map((f) => ({ path: f.path, dir: false }))].sort((a, b) => a.path.localeCompare(b.path));
    return all.filter((r) => { const parts = r.path.split('/'); return !parts.slice(0, -1).some((_, i) => closed[parts.slice(0, i + 1).join('/')]); })
      .map((r) => ({ ...r, depth: r.path.split('/').length - 1, label: r.path.split('/').pop() as string }));
  }, [tree, filter, closed]);

  if (!vault) return (
    <div style={{ margin: 'auto', textAlign: 'center', display: 'flex', flexDirection: 'column', gap: 12, alignItems: 'center' }}>
      <span className="muted">No vaults are attached to this workspace yet.</span>
      <button type="button" className="btn btn-p" onClick={() => go('settings', { setPage: 'vaults' })}>Attach a vault</button>
    </div>
  );
  return (
    <div className="split" style={{ flex: '1 1 auto', minWidth: 0, display: 'flex' }}>
      <nav aria-label="Notes" className="vault-side">
        {vaults.length > 1 && <select className="inp" aria-label="Vault" value={vault.id} onChange={(e) => { setVaultId(e.target.value); setHits(null); setFilter(''); }}>{vaults.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}</select>}
        <form className="row" onSubmit={(e) => { e.preventDefault(); search(); }}>
          <input className="inp grow" value={filter} onChange={(e) => { setFilter(e.target.value); setHits(null); }} placeholder="Filter, Enter searches text" aria-label="Filter notes, press Enter to search their text" />
        </form>
        <form className="row" onSubmit={(e) => { e.preventDefault(); const f = e.currentTarget.elements.namedItem('name') as HTMLInputElement; if (f.value.trim()) { void create(f.value); f.value = ''; } }}>
          <input name="name" className="inp grow" placeholder="New note, e.g. Work/plan" aria-label="New note path" />
          <button type="submit" className="btn" aria-label="Create note"><Icon name="plus" size={14} /></button>
        </form>
        <div className="vault-list">
          {hits ? <>
            {hits.hits.map((h, i) => <button key={i} type="button" className="navrow vault-hit" onClick={() => void open(h.vault, h.path)}><span className="ellipsis" style={{ fontWeight: 500 }}>{h.path}{h.line ? ':' + h.line : ''}</span>{h.line > 0 && <span className="muted ellipsis" style={{ fontSize: 12 }}>{h.text}</span>}</button>)}
            {!hits.hits.length && <p className="muted">Nothing found.</p>}
            {hits.truncated && <p className="muted" style={{ fontSize: 12 }}>Results cut short. Try a more specific search.</p>}
          </> : <>
            {rows.map((r) => r.dir
              ? <button key={r.path} type="button" className="navrow" aria-expanded={!closed[r.path]} style={{ paddingLeft: 10 + r.depth * 14 }} onClick={() => setClosed((c) => ({ ...c, [r.path]: !c[r.path] }))}><Icon name={closed[r.path] ? 'right' : 'down'} size={12} /><span className="ellipsis">{r.label}</span></button>
              : <button key={r.path} type="button" className="navrow" aria-current={opened?.vault === vault.id && opened.path === r.path ? 'true' : undefined} style={{ paddingLeft: 10 + r.depth * 14 }} onClick={() => void open(vault.id, r.path)}><Icon name="doc" size={13} /><span className="ellipsis">{r.label.replace(/\.md$/i, '')}</span></button>)}
            {tree && !rows.length && <p className="muted">{filter ? 'No note has that name. Press Enter to search their text.' : 'No Markdown notes here yet.'}</p>}
            {tree?.truncated && <p className="muted" style={{ fontSize: 12 }}>This vault has more notes than can be listed. Use search to reach the rest.</p>}
          </>}
        </div>
      </nav>
      <div style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column' }}>
        {opened && opened.vault === vault.id ? <>
          <div className="row vault-head">
            {renaming !== null
              ? <form className="row grow" onSubmit={(e) => { e.preventDefault(); void rename(renaming); }}><input className="inp grow" autoFocus aria-label="New path" value={renaming} onChange={(e) => setRenaming(e.target.value)} onKeyDown={(e) => { if (e.key === 'Escape') setRenaming(null); }} /><button type="submit" className="btn">Rename</button></form>
              : <><span className="mono ellipsis grow" title={opened.path} style={{ fontSize: 12 }}>{opened.path}</span><button type="button" className="link" onClick={() => setRenaming(opened.path)}>Rename</button><button type="button" className="link" onClick={remove}>Delete</button></>}
          </div>
          <NoteView key={opened.vault + '\0' + opened.path} vault={opened.vault} path={opened.path} flushRef={flush} onWiki={onWiki} />
        </> : <p className="muted" style={{ margin: 'auto' }}>Pick a note, or create one.</p>}
      </div>
    </div>
  );
}
