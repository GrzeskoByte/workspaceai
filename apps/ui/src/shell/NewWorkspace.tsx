// New workspace, or join an organization later through SSO (R-08). Not part of first-run setup.
import { useState } from 'react';
import { useStore, setState, switchWorkspace, toast } from '../store.ts';
import { post } from '../api.ts';

const TYPES = [
  { id: 'team', label: 'Team', note: 'shared space you manage' },
  { id: 'personal', label: 'Personal', note: 'another private space' },
  { id: 'org', label: 'Organization', note: 'join with SSO' },
] as const;

export function NewWorkspace() {
  const draft = useStore((s) => s.newWs);
  const [email, setEmail] = useState('');
  const [issuer, setIssuer] = useState('');
  const [busy, setBusy] = useState(false);
  if (!draft) return null;
  const close = () => setState({ newWs: null });
  const create = async () => {
    const name = draft.name.trim();
    if (!name) { toast('Name the workspace'); return; }
    setBusy(true);
    try {
      const r = draft.type === 'org'
        ? await post<{ id: string }>('/api/workspaces/join-org', { name, email, issuer })
        : await post<{ id: string }>('/api/workspaces', { name, kind: draft.type });
      close();
      // Load the new workspace's own snapshot; nothing from the previous one stays on screen (R-06).
      await switchWorkspace(r.id);
      toast(draft.type === 'org' ? `Joined ${name} · policy verified` : `Created ${name} · its own engine starts with the first chat`);
    } catch (e) { toast((e as Error).message, 'error'); } finally { setBusy(false); }
  };
  return (
    <div className="scrim" onMouseDown={close}>
      <form className="card rise" role="dialog" aria-modal="true" aria-label="New workspace" style={{ marginTop: 120, width: 560, maxWidth: 'calc(100vw - 24px)', padding: 22, display: 'flex', flexDirection: 'column', gap: 14, background: 'var(--bg1)' }} onMouseDown={(e) => e.stopPropagation()} onSubmit={(e) => { e.preventDefault(); void create(); }}>
        <h2 className="disp" style={{ fontSize: 24 }}>New workspace</h2>
        <div role="radiogroup" aria-label="Workspace type" style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 8 }}>
          {TYPES.map((t) => (
            <button key={t.id} type="button" className="opt" role="radio" aria-checked={draft.type === t.id} aria-pressed={draft.type === t.id} onClick={() => setState({ newWs: { ...draft, type: t.id, name: draft.name } })} style={{ padding: '10px 12px' }}><b style={{ fontSize: 13 }}>{t.label}</b><span className="muted" style={{ fontSize: 11 }}>{t.note}</span></button>
          ))}
        </div>
        <label className="field">Name<input className="inp" autoFocus value={draft.name} onChange={(e) => setState({ newWs: { ...draft, name: e.target.value } })} placeholder={draft.type === 'org' ? 'Organization name' : 'Workspace name'} /></label>
        {draft.type === 'org' && <>
          <p className="muted" style={{ margin: 0, fontSize: 13 }}>Sign in with your organization’s identity provider. Its signed policy decides the models, connectors, sharing and retention here.</p>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 10 }}>
            <label className="field">Email<input className="inp" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="you@example.com" /></label>
            <label className="field">SSO issuer<input className="inp" value={issuer} onChange={(e) => setIssuer(e.target.value)} placeholder="https://login.example.com/realms/org" /></label>
          </div>
        </>}
        <span className="muted" style={{ fontSize: 12 }}>Gets its own opencode server, Open Design daemon, memory, secrets and a General project. Nothing is copied from other workspaces.</span>
        <div className="row"><span className="grow" /><button type="button" className="btn" onClick={close}>Cancel</button><button type="submit" className="btn btn-p" disabled={busy}>{busy ? 'Working…' : draft.type === 'org' ? 'Continue with SSO' : 'Create workspace'}</button></div>
      </form>
    </div>
  );
}
