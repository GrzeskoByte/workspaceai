// Confirm dialog and toasts. Both read the store the rest of the shell writes.
import { useStore, setState } from '../store.ts';

export function Confirm() {
  const confirm = useStore((s) => s.confirm);
  if (!confirm) return null;
  return (
    <div className="scrim" onMouseDown={() => setState({ confirm: null })}>
      <div className="card rise" role="dialog" aria-modal="true" aria-labelledby="confirm-title" style={{ marginTop: 120, width: 440, maxWidth: 'calc(100vw - 24px)', padding: 22, display: 'flex', flexDirection: 'column', gap: 12 }} onMouseDown={(e) => e.stopPropagation()}>
        <h2 id="confirm-title" className="disp" style={{ margin: 0, fontSize: 24 }}>{confirm.title}</h2>
        <p style={{ margin: 0 }}>{confirm.body}</p>
        <div className="row">
          <span className="grow" />
          <button type="button" className="btn" onClick={() => setState({ confirm: null })}>Cancel</button>
          <button type="button" className="btn btn-d" onClick={() => { const run = confirm.run; setState({ confirm: null }); void run(); }}>{confirm.ok}</button>
        </div>
      </div>
    </div>
  );
}

export function Toasts() {
  const toasts = useStore((s) => s.toasts);
  if (!toasts.length) return null;
  return (
    <div aria-live="polite" style={{ position: 'fixed', right: 16, bottom: 16, display: 'flex', flexDirection: 'column', gap: 8, zIndex: 40 }}>
      {toasts.map((t) => <div key={t.id} className="card" style={{ padding: '10px 14px', maxWidth: 360, color: t.kind === 'error' ? 'var(--error-text)' : 'var(--text)' }}>{t.text}</div>)}
    </div>
  );
}
