// Artifact library. Making one from here must pick a project (R-13).
import { useState } from 'react';
import { useStore, go } from '../store.ts';
import { projectPathName } from '../../../../packages/shared/src/util.ts';
import { openArtifact } from '../shell/Shell.tsx';
import { ART_ICON } from './ArtifactPanel.tsx';
import { Icon } from '../ui/Icon.tsx';

export function Library() {
  const snap = useStore((s) => s.snap)!;
  const [q, setQ] = useState('');
  const [type, setType] = useState('all');
  const rows = snap.artifacts.filter((a) => (type === 'all' || a.type === type) && (!q || a.title.toLowerCase().includes(q.toLowerCase())));
  const types = ['all', ...new Set(snap.artifacts.map((a) => a.type))];
  return (
    <div className="page-pad" style={{ flex: '1 1 auto', overflowY: 'auto', padding: 28 }}>
      <div className="row" style={{ marginBottom: 16 }}>
        <h1 className="disp" style={{ margin: 0, fontSize: 32 }}>Artifacts</h1>
        <span className="grow" />
        <button type="button" className="btn btn-p" onClick={() => go('brief')}>New artifact</button>
      </div>
      <div className="row" style={{ marginBottom: 14, flexWrap: 'wrap' }}>
        <input className="inp" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter by title" aria-label="Filter artifacts" style={{ width: 220 }} />
        {types.map((t) => <button key={t} type="button" className="chip" aria-pressed={type === t} onClick={() => setType(t)}>{t}</button>)}
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))', gap: 12 }}>
        {rows.map((a) => (
          <button key={a.id} type="button" className="tile" onClick={() => openArtifact(a)} style={{ textAlign: 'left', display: 'flex', flexDirection: 'column', gap: 6 }}>
            <span className="row" style={{ gap: 6 }}><Icon name={ART_ICON[a.type] || 'layers'} size={15} style={{ color: 'var(--accent-p-text)' }} /> <b>{a.title}</b></span>
            <span className="muted" style={{ fontSize: 12 }}>{a.type} v{a.version} · {projectPathName(snap.projects, a.project)}</span>
          </button>
        ))}
      </div>
      {!rows.length && <p className="muted">No artifacts yet. New ones land in the project you pick.</p>}
    </div>
  );
}
