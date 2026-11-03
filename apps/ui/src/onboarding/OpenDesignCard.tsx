// Open Design install status in onboarding and Settings › Engine (R-04).
import { useEffect, useRef, useState } from 'react';
import { get, post } from '../api.ts';
import { Icon } from '../ui/Icon.tsx';

interface OdStatus { installed: boolean; path?: string; version?: string; installing?: boolean; progress?: number; log?: string; error?: string }

export function OpenDesignCard({ ready, onReady }: { ready: 'ok' | 'later' | null; onReady: (r: 'ok' | 'later' | null) => void }) {
  const [st, setSt] = useState<OdStatus | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const load = () => get<OdStatus>('/api/opendesign/status').then((s) => { setSt(s); if (s.installed && ready !== 'later') onReady('ok'); if (s.installing) timer.current = window.setTimeout(load, 1000); }).catch(() => setSt({ installed: false, error: 'Status unavailable' }));
  useEffect(() => { load(); return () => clearTimeout(timer.current); }, []);
  const install = async () => { await post('/api/opendesign/install').catch((e) => setSt({ installed: false, error: e.message })); load(); };
  const pct = st?.installed ? 100 : st?.progress ?? 0;
  const dot = st?.installed ? 'var(--success)' : st?.installing ? 'var(--warning)' : 'var(--dim)';
  const text = st?.installed ? `installed${st.version ? ' · ' + st.version : ''}, one daemon per workspace` : st?.installing ? `installing… ${pct}%` : ready === 'later' ? 'will install later — agents can’t save artifacts until it is installed' : 'not installed';
  return (
    <div className="card" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 10 }}>
      <span className="row" style={{ gap: 12 }}>
        <span className={'dot ' + (st?.installing ? 'pulse' : '')} style={{ width: 10, height: 10, background: dot }} />
        <span className="grow"><b>Open Design</b> · {text}</span>
        <button type="button" className="btn" onClick={install} disabled={!st || st.installed || st.installing}>{st?.installed ? <>Installed <Icon name="check" size={13} stroke={2.5} /></> : st?.installing ? 'Installing…' : 'Install'}</button>
      </span>
      <div style={{ height: 6, borderRadius: 999, background: 'var(--bg3)', overflow: 'hidden', marginLeft: 22 }}><div style={{ height: 6, width: pct + '%', background: 'var(--accent-p)', transition: 'width .3s linear' }} /></div>
      <span className="muted" style={{ fontSize: 12, paddingLeft: 22 }}>
        The artifact engine: every doc, deck, prototype and sheet is saved, versioned and linted here, with design systems and PDF/PowerPoint export.{' '}
        {!st?.installed && <button type="button" className="link" onClick={() => onReady(ready === 'later' ? null : 'later')}>{ready === 'later' ? 'Install now instead' : 'Install later'}</button>}
      </span>
      {st?.error && <span style={{ fontSize: 12, paddingLeft: 22, color: 'var(--error-text)' }}>{st.error}</span>}
      {st?.installing && st.log && <pre className="pre" style={{ marginLeft: 22, maxHeight: 90 }}>{st.log}</pre>}
    </div>
  );
}
