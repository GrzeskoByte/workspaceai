// Approval requests from chats you are not looking at — a graph run's steps — shown where the run is watched.
import { useEffect } from 'react';
import { useStore, setState, toast, wsId, loadPermissions } from '../store.ts';
import { post } from '../api.ts';
import { Icon } from '../ui/Icon.tsx';

export function Approvals({ chatIds, compact }: { chatIds: string[]; compact?: boolean }) {
  const key = chatIds.join(',');
  const perms = useStore((s) => s.permissions);
  const snap = useStore((s) => s.snap);
  const asks = chatIds.flatMap((c) => (perms[c] || []).map((p) => ({ ...p, chat: c })));
  const agentOf = (c: string) => { const ch = snap?.chats.find((x) => x.id === c); return ch ? (ch.agent === 'opencode' ? 'opencode' : snap?.workspace.agents.find((a) => a.id === ch.agent)?.name || ch.agent) : 'The agent'; };
  // After a restart the cards come from the engine, not from events.
  useEffect(() => { for (const c of chatIds) void loadPermissions(c); }, [key]);
  if (!asks.length) return null;
  const reply = async (chat: string, id: string, decision: 'once' | 'always' | 'reject') => {
    const drop = () => setState((s) => ({ permissions: { ...s.permissions, [chat]: (s.permissions[chat] || []).filter((p) => p.id !== id) } }));
    try { await post(`/api/ws/${wsId()}/chats/${chat}/permission/${id}`, { decision }); drop(); toast(decision === 'reject' ? 'Rejected — the step goes on without it' : 'Allowed — the run goes on'); }
    catch (e) { drop(); toast((e as Error).message || 'That request had already expired', 'error'); }
  };
  return (
    <div className="approvals" role="group" aria-label="Approvals the run is waiting for">
      {asks.map((p) => (
        <div key={p.id} className="approval">
          <span className="row" style={{ gap: 8, alignItems: 'flex-start' }}>
            <span style={{ color: 'var(--warning-text)', display: 'flex', paddingTop: 2 }}><Icon name="lock" size={14} /></span>
            <span className="grow" style={{ fontSize: 13, minWidth: 0 }}><b>{agentOf(p.chat)}</b> asks to use <span className="mono" style={{ fontSize: 12 }}>{p.title}</span>{!compact && p.detail && p.detail !== '""' && <span className="muted mono ellipsis" style={{ display: 'block', fontSize: 11, marginTop: 2 }}>{p.detail}</span>}</span>
          </span>
          <span className="row" style={{ gap: 6, flexWrap: 'wrap' }}>
            <button type="button" className="btn btn-p" style={{ height: 28 }} onClick={() => void reply(p.chat, p.id, 'once')}>Allow once</button>
            <button type="button" className="btn" style={{ height: 28 }} onClick={() => void reply(p.chat, p.id, 'always')}>Always allow</button>
            <button type="button" className="btn btn-d" style={{ height: 28 }} onClick={() => void reply(p.chat, p.id, 'reject')}>Reject</button>
          </span>
        </div>
      ))}
    </div>
  );
}
