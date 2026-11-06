// Beside the chat: the memory and vault notes this session read, and the way to this chat on the project's map.
import { useEffect, useMemo, useState } from 'react';
import { useStore, setState, go, wsId } from '../store.ts';
import { get } from '../api.ts';
import { readsFromTools, type SessionRead } from '../../../../packages/shared/src/messages.ts';
import type { ResolvedMap } from '../../../../packages/shared/src/types.ts';
import { Icon } from '../ui/Icon.tsx';

function Section({ title, rows, empty, open }: { title: string; rows: SessionRead[]; empty: string; open: (r: SessionRead) => (() => void) | undefined }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <span className="lbl">{title} · {rows.length}</span>
      {!rows.length && <span className="muted" style={{ fontSize: 13 }}>{empty}</span>}
      {rows.map((r) => { const onOpen = open(r); return onOpen
        ? <button key={r.title || r.label} type="button" className="navrow mono" title={r.title || r.label} onClick={onOpen} style={{ fontSize: 12 }}><span className="ellipsis">{r.label}</span></button>
        : <span key={r.title || r.label} className="mono ellipsis" title={r.title || r.label} style={{ fontSize: 12, padding: '4px 8px' }}>{r.label}</span>; })}
    </div>
  );
}

export function SessionReadsPanel({ chatId, project }: { chatId: string; project: string }) {
  const vaults = useStore((s) => s.snap?.workspace.vaults);
  const messages = useStore((s) => s.messages[chatId]);
  const liveTools = useStore((s) => s.live[chatId]?.tools);
  const mapTick = useStore((s) => s.mapTick);
  const [onMap, setOnMap] = useState(false);
  const reads = useMemo(() => readsFromTools([...(messages || []).flatMap((m) => m.tools || []), ...(liveTools || [])], vaults), [messages, liveTools, vaults]);

  useEffect(() => {
    let live = true;
    setOnMap(false);
    get<ResolvedMap>(`/api/ws/${wsId()}/projects/${project}/map`).then((m) => { if (live) setOnMap(m.nodes.some((n) => n.id === 'chat:' + chatId)); }, () => {});
    return () => { live = false; };
  }, [chatId, project, mapTick]);

  return (
    <section aria-label="Session reads" className="rise art-panel map-panel" style={{ flex: '0 0 var(--right-w, 620px)' }}>
      <div className="row" style={{ gap: 8, padding: '10px 14px', borderBottom: '1px solid var(--bg2)' }}>
        <b className="grow">Read this session</b>
        {onMap && <button type="button" className="btn" onClick={() => setState({ mapOpen: true })} title="Show this chat on the project’s map" style={{ height: 28 }}><Icon name="map" size={14} /> Open in map</button>}
        <button type="button" className="btn" aria-label="Close" onClick={() => setState({ readsOpen: false })} style={{ height: 28, width: 28, padding: 0, justifyContent: 'center' }}><Icon name="x" size={15} /></button>
      </div>
      <div style={{ overflow: 'auto', padding: 14, display: 'flex', flexDirection: 'column', gap: 16, minHeight: 0 }}>
        <Section title="Memory" rows={reads.filter((r) => r.kind === 'memory')} empty="No memory read yet." open={(r) => (r.title ? undefined : () => go('memory'))} />
        <Section title="Wiki" rows={reads.filter((r) => r.kind === 'wiki')} empty="No vault notes read yet." open={(r) => (r.note ? () => go('vaults', { vaultOpen: r.note }) : undefined)} />
      </div>
    </section>
  );
}
