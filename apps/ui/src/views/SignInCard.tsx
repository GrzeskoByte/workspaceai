// Shown under a turn that failed because the agent's sign-in expired: sign in again without leaving the chat, then
// the message is sent again by itself.
import { useEffect, useRef, useState } from 'react';
import { get, post } from '../api.ts';
import { toast, wsId } from '../store.ts';
import type { SubscriptionId } from '../../../../packages/shared/src/types.ts';
import { Icon } from '../ui/Icon.tsx';

interface Status { state: 'idle' | 'waiting' | 'done' | 'failed'; url?: string; error?: string; browser?: boolean; computer?: { account: string } | null }

const NAMES: Record<SubscriptionId, string> = { claude: 'Claude', grok: 'Grok', opencode: 'opencode' };

export function SignInCard({ sub, agentName, onSignedIn }: { sub: SubscriptionId; agentName: string; onSignedIn: () => void }) {
  const base = `/api/ws/${wsId()}/subscriptions/${sub}`;
  const [s, setS] = useState<Status | null>(null);
  const [busy, setBusy] = useState(false);
  const [code, setCode] = useState('');
  const done = useRef(false);
  const finish = () => { if (done.current) return; done.current = true; setS((x) => ({ ...(x || {}), state: 'done' })); onSignedIn(); };

  useEffect(() => { void get<Status>(`${base}/signin`).then(setS, () => setS({ state: 'idle' })); }, [base]);
  // While the browser sign-in runs, watch for it to finish.
  useEffect(() => {
    if (s?.state !== 'waiting') return;
    const t = window.setInterval(() => void get<Status>(`${base}/signin`).then((n) => { if (n.state === 'done') finish(); else setS(n); }, () => {}), 1500);
    return () => window.clearInterval(t);
  }, [s?.state, base]);

  const browser = async () => {
    setBusy(true);
    try { const n = await post<Status>(`${base}/signin`); if (n.state === 'done') finish(); else setS((x) => ({ ...x, ...n })); }
    catch (e) { toast((e as Error).message, 'error'); }
    finally { setBusy(false); }
  };
  const useComputer = async () => {
    setBusy(true);
    try { await post(`${base}/connect`); toast(`Using this computer’s ${NAMES[sub]} login`); finish(); }
    catch (e) { toast((e as Error).message, 'error'); }
    finally { setBusy(false); }
  };
  const sendCode = async () => {
    if (!code.trim()) return;
    setBusy(true);
    try { const n = await post<Status>(`${base}/signin/code`, { code }); setS((x) => ({ ...x, ...n })); setCode(''); }
    catch (e) { toast((e as Error).message, 'error'); }
    finally { setBusy(false); }
  };
  const cancel = () => void post<Status>(`${base}/signin/cancel`).then((n) => setS((x) => ({ ...x, ...n, url: undefined, error: undefined })), () => {});

  if (!s) return null;
  if (s.state === 'done') return <div className="card rise signin-card"><b style={{ fontSize: 13 }}>Signed in to {NAMES[sub]}</b><span className="muted" style={{ fontSize: 12.5 }}>Sending your message again…</span></div>;
  return (
    <div className="card rise signin-card" role="group" aria-label={`Sign in to ${NAMES[sub]}`}>
      <span style={{ fontSize: 13.5 }}><b>{agentName}</b> needs you to sign in again</span>
      <span className="muted" style={{ fontSize: 12.5 }}>This workspace’s {NAMES[sub]} login has expired. Sign in and your message is sent again.</span>
      {s.state === 'waiting' ? <>
        <span style={{ fontSize: 12.5 }}><span className="pulse" style={{ color: 'var(--accent-p)', display: 'inline-flex', verticalAlign: -2 }}><Icon name="dot" size={12} /></span> Finish signing in in your browser.</span>
        {s.url && <span className="row" style={{ flexWrap: 'wrap', gap: 6 }}>
          <span className="muted" style={{ fontSize: 12 }}>No browser window?</span>
          <a className="chip" href={s.url} target="_blank" rel="noreferrer noopener" style={{ height: 24, fontSize: 11.5, textDecoration: 'none' }}>Open the sign-in page</a>
          <button type="button" className="chip" style={{ height: 24, fontSize: 11.5 }} onClick={() => navigator.clipboard.writeText(s.url!).then(() => toast('Link copied'), () => toast('Copy failed', 'error'))}>Copy link</button>
        </span>}
        <form className="row" style={{ gap: 6 }} onSubmit={(e) => { e.preventDefault(); void sendCode(); }}>
          <input className="inp grow" value={code} onChange={(e) => setCode(e.target.value)} placeholder="If the page shows a code, paste it here" aria-label="Sign-in code" style={{ height: 30, minWidth: 0 }} />
          <button type="submit" className="btn" disabled={busy || !code.trim()} style={{ height: 30 }}>Submit</button>
          <button type="button" className="btn" onClick={cancel} style={{ height: 30 }}>Cancel</button>
        </form>
      </> : (
        <span className="row" style={{ flexWrap: 'wrap', gap: 8 }}>
          {s.browser && <button type="button" className="btn btn-p" disabled={busy} onClick={() => void browser()}>Sign in with {NAMES[sub]}</button>}
          {s.computer && <button type="button" className={s.browser ? 'btn' : 'btn btn-p'} disabled={busy} onClick={() => void useComputer()}>Use this computer’s login{s.computer.account ? ' · ' + s.computer.account : ''}</button>}
          {!s.browser && !s.computer && <span className="muted mono" style={{ fontSize: 12 }}>Run “{sub === 'grok' ? 'grok login' : sub === 'opencode' ? 'opencode auth login' : 'claude auth login'}” in a terminal, then retry.</span>}
        </span>
      )}
      {s.state === 'failed' && s.error && <span style={{ color: 'var(--error-text)', fontSize: 12 }}>{s.error}</span>}
    </div>
  );
}
