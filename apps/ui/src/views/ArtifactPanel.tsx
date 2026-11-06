// Side panel for one artifact: provenance, content, versions, Open Design lint, export, share (R-13, R-14).
import { useCallback, useEffect, useState } from 'react';
import { useStore, setState, act, refreshSnap, wsId, toast, go } from '../store.ts';
import { get, post, del, download } from '../api.ts';
import type { Artifact, ArtifactFormat, ArtifactType } from '../../../../packages/shared/src/types.ts';
import { flatProjects, projectPathName } from '../../../../packages/shared/src/util.ts';
import { Markdown } from '../ui/Markdown.tsx';
import { ArtifactFullscreen, withEscRelay } from './ArtifactFullscreen.tsx';
import { Icon } from '../ui/Icon.tsx';

/** Icon names (ui/Icon.tsx) per artifact kind. */
export const ART_ICON: Record<string, string> = { Doc: 'doc', Deck: 'deck', Prototype: 'app', Sheet: 'table', Image: 'image', Video: 'video', LiveArtifact: 'activity', DesignSystem: 'palette' };
export const ART_TYPES: ArtifactType[] = ['Doc', 'Deck', 'Prototype', 'Sheet', 'Image', 'Video', 'LiveArtifact', 'DesignSystem'];

interface ExportFormat { label: string; ext: string; format: string }
/** A version from the supervisor: its source and the page Open Design stores for it. */
interface View { content: string; version: number; format: ArtifactFormat; html: string }

const FORMAT_LABEL: Record<ArtifactFormat, string> = { markdown: 'Markdown', html: 'HTML', csv: 'CSV', svg: 'SVG', json: 'JSON' };
const EXT_FORMAT: Record<string, ArtifactFormat> = { md: 'markdown', html: 'html', csv: 'csv', svg: 'svg', json: 'json' };

function csvRows(csv: string): string[][] { return csv.split('\n').filter((l) => l.trim()).map((l) => l.split(',').map((c) => c.trim())); }

export function ArtifactPanel() {
  const snap = useStore((s) => s.snap)!;
  const artId = useStore((s) => s.artId);
  const art = snap.artifacts.find((a) => a.id === artId);
  const [content, setContent] = useState('');
  // What the version is written in, and the page Open Design stores for it (decks on its deck framework).
  const [format, setFormat] = useState<ArtifactFormat>('markdown');
  const [page, setPage] = useState('');
  const wide = useStore((s) => s.artWide);
  const setWide = (v: boolean) => setState({ artWide: v });
  const [full, setFull] = useState(false);
  const leaveFull = useCallback(() => setFull(false), []);
  const [version, setVersion] = useState(0);
  const [formats, setFormats] = useState<ExportFormat[]>([]);
  const [editing, setEditing] = useState(false);
  const [ask, setAsk] = useState('');
  useEffect(() => {
    if (!art) return;
    let stop = false;
    get<View>(`/api/ws/${wsId()}/artifacts/${art.id}/content`).then((r) => { if (!stop) { show(r); setEditing(false); } }).catch((e) => toast(e.message, 'error'));
    get<{ formats: ExportFormat[] }>(`/api/ws/${wsId()}/artifacts/${art.id}/exports`).then((r) => { if (!stop) setFormats(r.formats || []); }).catch(() => {});
    return () => { stop = true; };
  }, [art?.id, art?.version]);
  if (!art) return null;
  function show(r: View) { setContent(r.content); setVersion(r.version); setFormat(r.format); setPage(r.html); }
  const base = `/api/ws/${wsId()}/artifacts/${art.id}`;
  const fromChat = art.chat ? snap.chats.find((c) => c.id === art.chat) : undefined;
  const cur = art.versions.find((v) => v.v === (version || art.version));
  const od = cur?.od;

  const save = async () => {
    const next = await act(() => post<Artifact>(`${base}/versions`, { content, note: 'edited in the panel' }), (a) => a.title + ' v' + a.version);
    if (next) { setEditing(false); await refreshSnap(); }
  };
  const revise = () => {
    const t = ask.trim();
    if (!t) { toast('Describe the change first'); return; }
    if (!fromChat) { toast('This artifact came from the library — open a chat in its project to ask for changes'); return; }
    setAsk('');
    go('chat', { chatId: fromChat.id, projId: art.project, artId: art.id });
    setState((s) => ({ live: { ...s.live, [fromChat.id]: { text: '', tools: [], messageId: '' } } }));
    void act(() => post(`/api/ws/${wsId()}/chats/${fromChat.id}/send`, { text: `[Revise the artifact “${art.title}” (artifact id ${art.id}) with the artifact_update tool, saving a new version.] ${t}` }), 'Asked for a new version');
  };

  return (
    <section aria-label="Artifact" className="rise art-panel" style={{ flex: wide ? '1 1 auto' : '0 0 var(--right-w, 520px)', minWidth: 0, maxWidth: '100%', display: 'flex', flexDirection: 'column', background: 'var(--bg1)', borderLeft: '1px solid var(--bg2)', minHeight: 0 }}>
      <header className="row" style={{ padding: '10px 14px', borderBottom: '1px solid var(--bg2)' }}>
        <span className="mono" style={{ fontSize: 10.5, padding: '2px 6px', borderRadius: 4, background: 'var(--bg3)', color: 'var(--accent-p-text)' }}>{art.type.toUpperCase()}</span>
        <span className="mono muted" style={{ fontSize: 10.5 }} title="What this version is written in">{FORMAT_LABEL[format]}</span>
        <span className="grow" style={{ display: 'flex', flexDirection: 'column', minWidth: 0 }}>
          <b className="ellipsis" style={{ fontSize: 13 }}>{art.title}</b>
          <span className="row ellipsis" style={{ gap: 6, fontSize: 11 }}>
            <button type="button" className="link" onClick={() => go('project', { projId: art.project, ptab: 'arts', artId: null })} style={{ fontSize: 11 }}>▤ {projectPathName(snap.projects, art.project)}</button>
            {fromChat && <button type="button" onClick={() => go('chat', { chatId: fromChat.id, projId: art.project, artId: art.id })} className="link ellipsis" style={{ fontSize: 11, color: 'var(--muted)', minWidth: 0 }}>· from <Icon name="chat" size={12} style={{ verticalAlign: -2 }} /> {fromChat.title}</button>}
          </span>
        </span>
        <span className="chip mono" style={{ fontSize: 11 }}>v{version || art.version} of {art.version}</span>
        <button type="button" className="btn" onClick={() => setFull(true)} aria-label="Full screen" title="Full screen" style={{ height: 28, width: 28, padding: 0, justifyContent: 'center' }}><Icon name="fullscreen" size={15} /></button>
        <button type="button" className="btn wide-toggle" onClick={() => setWide(!wide)} aria-pressed={wide} aria-label={wide ? 'Show the chat beside the artifact' : 'Expand the artifact'} title={wide ? 'Show the chat' : 'Expand'} style={{ height: 28, width: 28, padding: 0, justifyContent: 'center' }}><Icon name={wide ? 'collapse' : 'expand'} size={15} /></button>
        <button type="button" className="btn" onClick={() => setState({ artId: null, artWide: false })} aria-label="Close artifact" style={{ height: 28, width: 28, padding: 0, justifyContent: 'center' }}><Icon name="x" size={15} /></button>
      </header>

      <div style={{ flex: '1 1 auto', overflowY: 'auto', padding: 16, display: 'flex', flexDirection: 'column', gap: 12 }}>
        <span className="row" style={{ fontSize: 12 }}>
          <span className="dot" style={{ background: od ? (od.lint.length ? 'var(--warning)' : 'var(--success)') : 'var(--dim)' }} />
          <span className="muted">{od ? `Saved by Open Design · lint ${od.lint.length ? od.lint.length + ' finding' + (od.lint.length === 1 ? '' : 's') : 'clean'}` : 'Built-in store · Open Design not running for this version'}</span>
        </span>
        {od && od.lint.length > 0 && <div className="card" style={{ padding: '8px 12px', fontSize: 12, display: 'flex', flexDirection: 'column', gap: 4 }}>{od.lint.slice(0, 8).map((f, i) => <span key={i}><b>{String(f.severity || f.level || 'P?')}</b> {String(f.message || f.rule || f.id || JSON.stringify(f))}</span>)}</div>}

        {editing ? <textarea className="inp mono" style={{ minHeight: 320, fontSize: 12 }} value={content} onChange={(e) => setContent(e.target.value)} aria-label="Artifact content" />
          : format === 'csv' ? (
            <div style={{ overflow: 'auto', border: '1px solid var(--line)', borderRadius: 8 }}>
              <table style={{ borderCollapse: 'collapse', width: '100%', fontSize: 12 }}><tbody>{csvRows(content).map((r, i) => <tr key={i}>{r.map((c, j) => <td key={j} style={{ padding: '7px 10px', borderBottom: '1px solid var(--bg3)', background: i === 0 ? 'var(--bg2)' : 'transparent', fontWeight: i === 0 ? 600 : 400 }}>{c}</td>)}</tr>)}</tbody></table>
            </div>
          ) : format === 'markdown' && art.type !== 'Deck' ? <Markdown text={content || 'Empty'} />
          : (
            // The page as Open Design stores it, sandboxed: scripts run, but no same-origin access to the app (security).
            <iframe key={art.id + ':' + (version || art.version)} title={art.title} sandbox="allow-scripts" srcDoc={page} className="art-frame" style={{ width: '100%', border: '1px solid var(--line)', borderRadius: 10, background: 'white', ...(art.type === 'Deck' ? { aspectRatio: '16 / 9', height: 'auto', maxHeight: wide ? 'calc(100vh - 260px)' : undefined } : { height: wide ? 'calc(100vh - 260px)' : 'min(64vh, 640px)' }) }} />
          )}

        <div className="row" style={{ flexWrap: 'wrap' }}>
          {editing ? <><button type="button" className="btn btn-p" onClick={() => void save()}>Save version</button><button type="button" className="btn" onClick={() => setEditing(false)}>Cancel</button></> : <button type="button" className="btn" onClick={() => setEditing(true)}>Edit</button>}
          {formats.map((f) => <button key={f.format} type="button" className="chip" onClick={() => void download(`${base}/export?format=${encodeURIComponent(f.format)}&v=${version}`).then(() => toast('Exported ' + art.title + f.ext), (e) => toast(e.message, 'error'))}>Export {f.label}</button>)}
          <button type="button" className="chip" onClick={() => void act(async () => { const r = await post<{ note: string; url?: string }>(`${base}/share`); if (r.url) await navigator.clipboard?.writeText(r.url).catch(() => {}); return r; }, (r) => r.note + (r.url ? ' · ' + r.url : ''))}>Share</button>
        </div>

        <label className="field">Project
          <select className="inp" value={art.project} onChange={(e) => void act(() => post(`${base}/move`, { project: e.target.value }).then(() => refreshSnap()), 'Moved — the origin-chat link is kept')}>
            {flatProjects(snap.projects).map(({ project, depth }) => <option key={project.id} value={project.id}>{' '.repeat(depth * 2)}{project.name}</option>)}
          </select>
        </label>

        <span className="lbl">Versions</span>
        {art.versions.slice().reverse().map((v) => {
          const vc = v.chat ? snap.chats.find((c) => c.id === v.chat) : undefined;
          return (
            <div key={v.v} className="row" style={{ gap: 6 }}>
              <button type="button" className="navrow grow" aria-current={v.v === (version || art.version) ? 'true' : 'false'} onClick={() => void get<View>(`${base}/content?v=${v.v}`).then(show)}>
                <span className="mono">v{v.v}</span><span className="mono muted" style={{ fontSize: 10 }}>{FORMAT_LABEL[v.format || EXT_FORMAT[v.file.split('.').pop() || ''] || 'markdown']}</span><span className="ellipsis grow">{v.note}{vc ? ' · 💬 ' + vc.title : ''}</span><span className="muted" style={{ fontSize: 11 }}>{v.od ? 'OD' : ''} {new Date(v.at).toLocaleString()}</span>
              </button>
              {v.v !== art.version && <button type="button" className="btn" style={{ height: 28 }} onClick={() => void act(() => post(`${base}/restore`, { v: v.v }).then(() => refreshSnap()), `Restored v${v.v} as a new version`)}>Restore</button>}
            </div>
          );
        })}
        <button type="button" className="btn btn-d" style={{ alignSelf: 'flex-start' }} onClick={() => setState({ confirm: { title: 'Delete ' + art.title + '?', body: 'This removes the artifact and its versions from the project.', ok: 'Delete', run: async () => { await del(base); setState({ artId: null }); await refreshSnap(); } } })}>Delete…</button>
      </div>

      <form className="row" onSubmit={(e) => { e.preventDefault(); revise(); }} style={{ padding: '10px 14px', borderTop: '1px solid var(--bg2)' }}>
        <input className="inp grow" value={ask} onChange={(e) => setAsk(e.target.value)} placeholder="Ask for a change to this artifact" aria-label="Change request" />
        <button type="submit" className="btn btn-p">Apply</button>
      </form>
      {full && (
        <ArtifactFullscreen title={art.title} meta={`${art.type} · ${FORMAT_LABEL[format]} · v${version || art.version}`} onExit={leaveFull}>
          {format === 'csv' ? (
            <div className="art-full-doc"><div><table style={{ borderCollapse: 'collapse', width: '100%', fontSize: 14 }}><tbody>{csvRows(content).map((r, i) => <tr key={i}>{r.map((c, j) => <td key={j} style={{ padding: '8px 12px', borderBottom: '1px solid var(--bg3)', fontWeight: i === 0 ? 600 : 400 }}>{c}</td>)}</tr>)}</tbody></table></div></div>
          ) : format === 'markdown' && art.type !== 'Deck' ? <div className="art-full-doc"><div><Markdown text={content || 'Empty'} /></div></div>
            : <iframe title={art.title} sandbox="allow-scripts" srcDoc={withEscRelay(page)} />}
        </ArtifactFullscreen>
      )}
    </section>
  );
}
