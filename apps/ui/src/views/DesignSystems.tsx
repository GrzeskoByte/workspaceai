// Design systems: browse every system, set the workspace default, and make your own. A workspace system is a
// DesignSystem artifact, so it lives in a project and is versioned and refined in chat like any other artifact.
import { useEffect, useState } from 'react';
import { useStore, setState, act, refreshSnap, wsId, toast } from '../store.ts';
import { get, post, patch } from '../api.ts';
import { flatProjects } from '../../../../packages/shared/src/util.ts';
import { readZip, claudeSystemToDesignMd, DESIGN_MD_LIMIT, type MigratedSystem } from '../../../../packages/shared/src/claude-design-system.ts';
import type { Artifact, Chat } from '../../../../packages/shared/src/types.ts';
import { openArtifact, openChat } from '../shell/Shell.tsx';
import { Markdown } from '../ui/Markdown.tsx';

interface DS { id: string; title: string; summary: string; swatches: string[]; source: string }

const SOURCE: Record<string, string> = { harness: 'WorkspaceAI default', workspace: 'This workspace', 'open-design': 'Open Design' };
const RANK: Record<string, number> = { workspace: 0, harness: 1 };

/** The prompt for a chat that writes a system; the agent saves it with the artifact tools. */
function designSystemPrompt(name: string, about: string, fromCode: boolean): string {
  return [
    `Create a design system named “${name}”.`,
    about,
    fromCode && 'Derive it from this project’s code: read its stylesheets, theme and token files and components, and use the values you find rather than inventing new ones. Say which files you used.',
    'Call artifact_guide with type "design-system", then save it with artifact_create, type "design-system", format "markdown".',
  ].filter(Boolean).join('\n\n');
}

export function DesignSystemsView() {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const [systems, setSystems] = useState<DS[]>([]);
  const [q, setQ] = useState('');
  const [picked, setPicked] = useState<DS | null>(null);
  const [body, setBody] = useState('');
  const [making, setMaking] = useState(false);
  const [migrating, setMigrating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [name, setName] = useState('');
  const [about, setAbout] = useState('');
  const [fromCode, setFromCode] = useState(true);
  const [project, setProject] = useState(ws.defaultProject || snap.projects[0]?.id || '');
  const own = snap.artifacts.filter((a) => a.type === 'DesignSystem');

  useEffect(() => { get<{ systems: DS[] }>(`/api/ws/${wsId()}/design-systems`).then((r) => setSystems(r.systems)).catch((e) => toast(e.message, 'error')); }, [own.map((a) => a.id + a.version).join()]);
  useEffect(() => { if (picked) get<{ body: string }>(`/api/ws/${wsId()}/design-systems/${encodeURIComponent(picked.id)}`).then((r) => setBody(r.body)).catch((e) => setBody(e.message)); }, [picked?.id]);

  const useIt = (d: DS) => act(() => patch(`/api/ws/${wsId()}`, { designSystem: d.title }).then(() => refreshSnap()), 'Design system · ' + d.title);
  const create = async () => {
    const chat = await act(() => post<Chat>(`/api/ws/${wsId()}/chats`, { project, title: 'Design system · ' + name.trim(), text: designSystemPrompt(name.trim(), about.trim(), fromCode) }));
    if (!chat) return;
    setState((s) => ({ live: { ...s.live, [chat.id]: { text: '', tools: [], messageId: '' } } }));
    await refreshSnap();
    openChat(chat);
  };
  const importFile = async (file: File) => {
    const content = await file.text();
    const title = /^#\s+(.+)$/m.exec(content)?.[1].trim() || file.name.replace(/\.\w+$/, '');
    const a = await act(() => post<Artifact>(`/api/ws/${wsId()}/artifacts`, { project, type: 'DesignSystem', title, content, format: 'markdown', note: 'imported' }), 'Imported ' + title);
    if (a) await refreshSnap();
  };

  // Migrating a system again adds a version. A name already taken by a built-in or Open Design system gets a suffix, since the workspace default is stored by name.
  const save = async ({ title, markdown, notCarried }: MigratedSystem) => {
      const name = systems.some((d) => d.title === title && d.source !== 'workspace') ? `${title} (Claude)` : title;
      const existing = own.find((a) => a.title === name);
      if (existing) await post(`/api/ws/${wsId()}/artifacts/${existing.id}/versions`, { content: markdown, note: 'migrated from Claude again' });
      else await post(`/api/ws/${wsId()}/artifacts`, { project, type: 'DesignSystem', title: name, content: markdown, format: 'markdown', note: 'migrated from Claude' });
      await refreshSnap();
      toast(`${existing ? 'Updated' : 'Migrated'} ${name}${notCarried.length ? ' · still in Claude: ' + notCarried.join(', ') : ''}`);
      if (markdown.length > DESIGN_MD_LIMIT) toast(`${name} is long: agents read its first ${DESIGN_MD_LIMIT.toLocaleString()} characters, tokens first`);
  };
  const migrate = (file: File) => act(async () => save(claudeSystemToDesignMd(await readZip(new Uint8Array(await file.arrayBuffer())), file.name.replace(/\.zip$/i, ''))));
  const importAll = async () => {
    setImporting(true);
    try {
      await act(async () => {
        const r = await post<{ systems: MigratedSystem[]; failed: string[] }>(`/api/ws/${wsId()}/design-systems/claude`, {});
        for (const m of r.systems) await save(m);
        toast(`${r.systems.length} design system${r.systems.length === 1 ? '' : 's'} imported from Claude${r.failed.length ? ` · ${r.failed.length} couldn’t be read` : ''}`);
      });
    } finally { setImporting(false); }
  };

  if (picked) {
    const art = own.find((a) => a.id === picked.id);
    return (
      <div className="page-pad rise" style={{ flex: '1 1 auto', overflowY: 'auto', padding: 28, display: 'flex', flexDirection: 'column', gap: 14 }}>
        <div className="row" style={{ flexWrap: 'wrap' }}>
          <button type="button" className="btn" onClick={() => { setPicked(null); setBody(''); }}>Back</button>
          <h1 className="disp grow" style={{ margin: 0, fontSize: 30 }}>{picked.title}</h1>
          <span className="muted" style={{ fontSize: 12 }}>{SOURCE[picked.source]}</span>
          {art && <button type="button" className="btn" onClick={() => openArtifact(art)}>Refine</button>}
          <button type="button" className="btn btn-p" disabled={ws.designSystem === picked.title} onClick={() => void useIt(picked)}>{ws.designSystem === picked.title ? 'Workspace default' : 'Use for new artifacts'}</button>
        </div>
        {!!picked.swatches.length && <div className="row" style={{ gap: 6 }}>{picked.swatches.map((c) => <span key={c} className="mono muted" style={{ fontSize: 11, display: 'flex', flexDirection: 'column', gap: 4 }}><span style={{ width: 72, height: 44, borderRadius: 8, background: c, border: '1px solid var(--line)' }} />{c}</span>)}</div>}
        <div className="card" style={{ padding: '6px 18px' }}>{body ? <Markdown text={body} /> : <p className="muted pulse">Loading…</p>}</div>
      </div>
    );
  }

  const needle = q.toLowerCase().trim();
  const rows = systems.filter((d) => !needle || (d.title + ' ' + d.summary).toLowerCase().includes(needle)).sort((a, b) => (RANK[a.source] ?? 2) - (RANK[b.source] ?? 2));
  return (
    <div className="page-pad" style={{ flex: '1 1 auto', overflowY: 'auto', padding: 28, display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div className="row" style={{ flexWrap: 'wrap' }}>
        <h1 className="disp grow" style={{ margin: 0, fontSize: 32 }}>Design systems</h1>
        <label className="btn" style={{ cursor: 'pointer' }}>Import DESIGN.md<input type="file" className="sr" accept=".md,.markdown,text/markdown" onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) void importFile(f); }} /></label>
        <button type="button" className="btn" disabled={importing} onClick={() => void importAll()}>{importing ? 'Importing from Claude…' : 'Import from Claude'}</button>
        <button type="button" className="btn" aria-expanded={migrating} onClick={() => setMigrating(!migrating)}>Import a .zip</button>
        <button type="button" className="btn btn-p" aria-expanded={making} onClick={() => setMaking(!making)}>New design system</button>
      </div>
      <p className="muted" style={{ margin: 0, fontSize: 13 }}>New artifacts follow <b>{ws.designSystem}</b>. Agents read its DESIGN.md before they make anything.</p>
      <label className="field" style={{ maxWidth: 320 }}>Project for new and imported systems<select className="inp" value={project} onChange={(e) => setProject(e.target.value)}>{flatProjects(snap.projects).map(({ project: p, depth }) => <option key={p.id} value={p.id}>{' '.repeat(depth * 2)}{p.name}</option>)}</select></label>
      {migrating && <div className="card rise" style={{ padding: 14, display: 'flex', flexDirection: 'column', gap: 10, maxWidth: 640 }}>
        <span style={{ fontSize: 13 }}>In Claude, open a design system and download it as a .zip, then pick one or more here. Its tokens, brand book and component guidelines become a DESIGN.md. Migrating one again adds a new version.</span>
        <span className="muted" style={{ fontSize: 12 }}>Previews, fonts and logos stay in Claude. Nothing in the file is run.</span>
        <label className="field">Claude design system .zip<input className="inp" type="file" accept=".zip,application/zip" multiple onChange={(e) => { const picks = [...(e.target.files || [])]; e.target.value = ''; void picks.reduce((p, f) => p.then(() => migrate(f)), Promise.resolve()); }} /></label>
      </div>}
      {making && <form className="card rise" style={{ padding: 14, display: 'flex', flexDirection: 'column', gap: 10, maxWidth: 640 }} onSubmit={(e) => { e.preventDefault(); void create(); }}>
        <label className="field">Name<input className="inp" autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Acme" /></label>
        <label className="field">What should it feel like?<textarea className="inp" rows={3} value={about} onChange={(e) => setAbout(e.target.value)} placeholder="Brand colours, tone, products it’s for" /></label>
        <label className="row" style={{ fontSize: 13 }}><input type="checkbox" checked={fromCode} onChange={(e) => setFromCode(e.target.checked)} />Base it on this project’s code</label>
        <div className="row"><span className="grow" /><button type="submit" className="btn btn-p" disabled={!name.trim() || !project || (!about.trim() && !fromCode)}>Create in a chat</button></div>
      </form>}
      <input className="inp" value={q} onChange={(e) => setQ(e.target.value)} placeholder={`Search ${systems.length} design systems`} aria-label="Search design systems" style={{ maxWidth: 320 }} />
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))', gap: 12 }}>
        {rows.map((d) => (
          <button key={d.id} type="button" className="tile" onClick={() => setPicked(d)} style={{ textAlign: 'left', display: 'flex', flexDirection: 'column', gap: 6, ...(ws.designSystem === d.title ? { borderColor: 'var(--accent-p)' } : {}) }}>
            <span className="row" style={{ gap: 3 }}>{d.swatches.slice(0, 5).map((c) => <span key={c} style={{ width: 18, height: 18, borderRadius: 4, background: c, border: '1px solid var(--line)' }} />)}</span>
            <b>{d.title}</b>
            <span className="muted" style={{ fontSize: 12 }}>{ws.designSystem === d.title ? 'Workspace default · ' : ''}{SOURCE[d.source]}</span>
          </button>
        ))}
      </div>
      {!rows.length && <p className="muted">{systems.length ? 'No design system matches.' : 'Loading…'}</p>}
    </div>
  );
}
