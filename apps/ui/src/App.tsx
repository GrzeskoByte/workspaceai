import { useEffect, useState } from 'react';
import { useStore, setState, getState, switchWorkspace, refreshApp, refreshSnap, refreshModels, startEvents } from './store.ts';
import { initApi } from './api.ts';
import { Onboarding } from './onboarding/Onboarding.tsx';
import { Shell } from './shell/Shell.tsx';

function useSystemDark(): boolean {
  const [dark, setDark] = useState(() => window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false);
  useEffect(() => { const m = window.matchMedia('(prefers-color-scheme: dark)'); const f = () => setDark(m.matches); m.addEventListener('change', f); return () => m.removeEventListener('change', f); }, []);
  return dark;
}

export function App() {
  const app = useStore((s) => s.app);
  const snap = useStore((s) => s.snap);
  const error = useStore((s) => s.error);
  const sysDark = useSystemDark();

  useEffect(() => {
    let stop = () => {};
    (async () => {
      try {
        await initApi();
        await refreshApp();
        await refreshSnap();
        refreshModels();
        stop = startEvents();
        setState({ ready: true });
      } catch (e) { setState({ error: (e as Error).message }); }
    })();
    return () => stop();
  }, []);

  // ⌘K / Ctrl+K palette, ⌘1–9 workspaces, Esc closes overlays (R-05, R-44).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = getState();
      if (!s.app?.settings.onboarded) return;
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === 'k') { e.preventDefault(); setState({ palette: !s.palette }); return; }
      if (mod && /^[1-9]$/.test(e.key)) {
        const w = s.app.workspaces[Number(e.key) - 1];
        if (w) { e.preventDefault(); if (w.id !== s.app.settings.activeWorkspace) switchWorkspace(w.id); }
        return;
      }
      if (e.key === 'Escape' && (s.newWs || s.customProvider || s.confirm)) setState({ newWs: null, customProvider: false, confirm: null });
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const settings = app?.settings;
  const theme = settings?.followSystem ? (sysDark ? 'opencode' : 'claude') : settings?.theme || 'claude';
  const w = snap?.workspace;
  const accent = w && w.accent !== 'default' ? w.accent : w?.kind === 'org' ? 'org' : settings?.accent && settings.accent !== 'default' ? settings.accent : 'default';
  useEffect(() => { document.body.style.background = theme === 'claude' ? '#faf9f5' : '#0a0a0a'; }, [theme]);

  return (
    <div className="hx" data-theme={theme} data-accent={accent} data-reduce={settings?.reduceMotion ? 'true' : 'false'}>
      {error ? (
        <div style={{ margin: 'auto', textAlign: 'center' }} className="card rise"><div style={{ padding: 30 }}><h2 className="disp">Harness can’t reach its supervisor</h2><p className="muted">{error}</p><button className="btn" onClick={() => location.reload()}>Try again</button></div></div>
      ) : !app ? (
        <div style={{ margin: 'auto' }} className="muted mono pulse">starting harness…</div>
      ) : !settings?.onboarded ? <Onboarding /> : <Shell />}
    </div>
  );
}
