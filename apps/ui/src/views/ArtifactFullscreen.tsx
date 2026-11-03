// An artifact on its own, filling the screen: the page (or document) with a slim bar to leave. In the desktop app
// the window goes fullscreen too; in a browser the page asks for fullscreen. Esc or the bar's button leaves.
import { useEffect, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { Icon } from '../ui/Icon.tsx';

async function setNativeFullscreen(on: boolean): Promise<void> {
  try {
    if (window.__TAURI_INTERNALS__) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().setFullscreen(on);
    } else if (on && !document.fullscreenElement) await document.documentElement.requestFullscreen?.();
    else if (!on && document.fullscreenElement) await document.exitFullscreen?.();
  } catch { /* not allowed here: the in-app view still fills the window */ }
}

// A sandboxed page keeps the keyboard while it has focus (a deck needs it for its arrows), so Esc inside it would
// never reach the app: the full-screen copy of the page passes Esc up as a message.
const ESC_RELAY = '<script>addEventListener("keydown",function(e){if(e.key==="Escape"){e.preventDefault();parent.postMessage({harness:"leave-fullscreen"},"*")}},true)</script>';
export function withEscRelay(html: string): string { return html.includes('</body>') ? html.replace(/<\/body>(?![\s\S]*<\/body>)/, ESC_RELAY + '</body>') : html + ESC_RELAY; }

export function ArtifactFullscreen({ title, meta, onExit, children }: { title: string; meta: string; onExit: () => void; children: ReactNode }) {
  const root = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    void setNativeFullscreen(true);
    const prev = document.activeElement as HTMLElement | null;
    // Focus the page itself so a deck's arrow keys work straight away.
    const t = setTimeout(() => { (root.current?.querySelector('iframe') as HTMLIFrameElement | null)?.focus(); }, 60);
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); onExit(); } };
    const onMessage = (e: MessageEvent) => {
      const frame = root.current?.querySelector('iframe');
      if (frame && e.source === frame.contentWindow && (e.data as { harness?: string })?.harness === 'leave-fullscreen') onExit();
    };
    window.addEventListener('message', onMessage);
    // Leaving the browser's fullscreen (its own Esc) leaves this view too.
    const onChange = () => { if (!window.__TAURI_INTERNALS__ && !document.fullscreenElement) onExit(); };
    window.addEventListener('keydown', onKey);
    document.addEventListener('fullscreenchange', onChange);
    return () => {
      clearTimeout(t);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('message', onMessage);
      document.removeEventListener('fullscreenchange', onChange);
      void setNativeFullscreen(false);
      prev?.focus?.();
    };
  }, [onExit]);
  return createPortal(
    <div ref={root} className="art-full" role="dialog" aria-modal="true" aria-label={title + ' — full screen'}>
      <div className="art-full-bar">
        <b className="ellipsis" style={{ minWidth: 0 }}>{title}</b>
        <span className="mono" style={{ fontSize: 11, opacity: 0.7, flex: '0 0 auto' }}>{meta}</span>
        <span style={{ flex: '1 1 auto' }} />
        <span className="art-full-hint">Esc to leave</span>
        <button type="button" className="btn art-full-exit" onClick={onExit} aria-label="Leave full screen" style={{ height: 28 }}><Icon name="x" size={14} /> Exit full screen</button>
      </div>
      <div className="art-full-body">{children}</div>
    </div>,
    document.querySelector('.hx') || document.body,
  );
}
