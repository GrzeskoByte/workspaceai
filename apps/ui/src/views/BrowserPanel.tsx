// A browser beside the chat while developing: run the project's dev server and watch it, or open any page. Pages
// load in a frame; ones that refuse to be framed open in the system browser instead.
import { useEffect, useRef, useState } from 'react';
import { useStore, setState, toast, wsId } from '../store.ts';
import { get, post } from '../api.ts';
import { Icon } from '../ui/Icon.tsx';

interface Server { port: number; url: string; process: string; pid: number }
interface Dev { command: string; detected: string; folder: string; running: boolean; url: string; log: string[]; exit?: number | null; servers: Server[] }
interface Check { ok: boolean; status: number; embeddable: boolean; reason?: string }

/** What was typed, as a URL: "5173" → localhost:5173, "localhost:3000/x" → http://…, "example.com" → https://…. */
export function toUrl(raw: string): string {
  const t = raw.trim();
  if (!t) return '';
  if (/^\d{2,5}$/.test(t)) return `http://localhost:${t}/`;
  if (/^https?:\/\//i.test(t)) return t;
  if (/^(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]|[\w-]+\.localhost)(:\d+)?(\/|$)/.test(t)) return 'http://' + t;
  return 'https://' + t;
}

const LOCAL_URL = /https?:\/\/(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(?::\d+)?(?:\/[^\s'"`)<>\]]*)?/;
/** The first local address in a message or tool output — offered as a preview under it. */
export function localUrlIn(text: string): string {
  return (LOCAL_URL.exec(text)?.[0] || '').replace('0.0.0.0', 'localhost').replace(/[.,;:]+$/, '');
}

export function BrowserPanel() {
  const snap = useStore((s) => s.snap)!;
  const browser = useStore((s) => s.browser)!;
  const wide = useStore((s) => s.browserWide);
  const pid = useStore((s) => (s.view === 'chat' ? s.snap?.chats.find((c) => c.id === s.chatId)?.project : s.projId) || s.snap?.projects[0]?.id || '');
  const proj = snap.projects.find((p) => p.id === pid);
  const [input, setInput] = useState(browser.url);
  const [nav, setNav] = useState(0);
  const [hist, setHist] = useState<{ list: string[]; at: number }>({ list: browser.url ? [browser.url] : [], at: browser.url ? 0 : -1 });
  const [check, setCheck] = useState<Check | null>(null);
  const [dev, setDev] = useState<Dev | null>(null);
  const [cmd, setCmd] = useState<string | null>(null);
  const [showLog, setShowLog] = useState(false);
  const wantDevUrl = useRef(false);
  const base = `/api/ws/${wsId()}/projects/${pid}/dev`;
  const url = browser.url;

  const go = (raw: string, push = true) => {
    const u = toUrl(raw);
    if (!u) return;
    setInput(u);
    setState({ browser: { url: u } });
    setNav((n) => n + 1);
    if (push) setHist((h) => { const list = [...h.list.slice(0, h.at + 1), u].slice(-50); return { list, at: list.length - 1 }; });
  };
  const step = (d: number) => setHist((h) => { const at = h.at + d; if (at < 0 || at >= h.list.length) return h; go(h.list[at], false); return { ...h, at }; });

  useEffect(() => { setInput(url); }, [url]);
  // Pages that forbid frames would show blank; say so instead.
  useEffect(() => {
    if (!url) { setCheck(null); return; }
    let stop = false;
    get<Check>(`/api/preview/check?url=${encodeURIComponent(url)}`).then((c) => { if (!stop) setCheck(c); }, () => {});
    return () => { stop = true; };
  }, [url, nav]);

  // The project's dev server: refreshed while it runs, and opened here once it prints its address.
  const refresh = () => get<Dev>(base).then((d) => {
    setDev(d);
    if (wantDevUrl.current && d.url) { wantDevUrl.current = false; go(d.url); }
    if (wantDevUrl.current && !d.running && d.exit != null) wantDevUrl.current = false;
  }, () => {});
  useEffect(() => { setDev(null); setCmd(null); void refresh(); }, [pid]);
  useEffect(() => {
    if (!dev?.running && !wantDevUrl.current) return;
    const t = window.setInterval(() => void refresh(), 1500);
    return () => window.clearInterval(t);
  }, [dev?.running, pid]);

  const command = cmd ?? dev?.command ?? '';
  const start = async () => {
    try { wantDevUrl.current = true; const r = await post<Partial<Dev>>(base, { action: 'start', command }); setDev((d) => (d ? { ...d, ...r } : d)); void refresh(); }
    catch (e) { wantDevUrl.current = false; toast((e as Error).message, 'error'); }
  };
  const stop = async () => { await post(base, { action: 'stop' }).catch(() => {}); window.setTimeout(() => void refresh(), 600); };

  const btn = { height: 28, width: 28, padding: 0, justifyContent: 'center' } as const;
  return (
    <section aria-label="Browser" className="rise art-panel browser-panel" style={{ flex: wide ? '1 1 auto' : '0 0 560px' }}>
      <form className="row browser-bar" onSubmit={(e) => { e.preventDefault(); go(input); }}>
        <button type="button" className="btn" style={btn} onClick={() => step(-1)} disabled={hist.at <= 0} aria-label="Back"><Icon name="left" size={16} /></button>
        <button type="button" className="btn" style={btn} onClick={() => step(1)} disabled={hist.at >= hist.list.length - 1} aria-label="Forward"><Icon name="right" size={16} /></button>
        <button type="button" className="btn" style={btn} onClick={() => setNav((n) => n + 1)} disabled={!url} aria-label="Reload"><Icon name="reload" size={14} /></button>
        <input className="inp grow mono" value={input} onChange={(e) => setInput(e.target.value)} placeholder="localhost:5173, a port, or any address" aria-label="Address" spellCheck={false} style={{ height: 30, minWidth: 0, fontSize: 12.5 }} />
        {url && <a className="btn" style={{ ...btn, textDecoration: 'none' }} href={url} target="_blank" rel="noreferrer noopener" aria-label="Open in your browser" title="Open in your browser"><Icon name="external" size={14} /></a>}
        <button type="button" className="btn wide-toggle" style={btn} onClick={() => setState({ browserWide: !wide })} aria-pressed={wide} aria-label={wide ? 'Show the chat beside the browser' : 'Expand the browser'}><Icon name={wide ? 'collapse' : 'expand'} size={15} /></button>
        <button type="button" className="btn" style={btn} onClick={() => setState({ browser: null, browserWide: false })} aria-label="Close browser"><Icon name="x" size={15} /></button>
      </form>

      <div className="browser-dev">
        <span className={'dot' + (dev?.running ? ' on' : '')} aria-hidden />
        <input className="inp grow mono" value={command} onChange={(e) => setCmd(e.target.value)} placeholder={dev ? 'Command that starts the dev server' : '…'} aria-label="Dev server command" spellCheck={false} disabled={!!dev?.running} style={{ height: 28, minWidth: 0, fontSize: 12 }} />
        {dev?.running
          ? <button type="button" className="btn" onClick={() => void stop()} style={{ height: 28 }}>Stop</button>
          : <button type="button" className="btn btn-p" onClick={() => void start()} disabled={!command.trim()} style={{ height: 28 }}>Run</button>}
        {!!dev?.log.length && <button type="button" className="btn" onClick={() => setShowLog(!showLog)} aria-pressed={showLog} style={{ height: 28 }}>Log</button>}
      </div>
      {(dev?.url || !!dev?.servers.length) && <div className="row browser-servers">
        <span className="muted" style={{ fontSize: 11 }}>{proj?.name || 'Project'}</span>
        {[...(dev?.url ? [{ url: dev.url, label: dev.url.replace(/^https?:\/\//, '').replace(/\/$/, ''), title: 'started here' }] : []),
          ...(dev?.servers || []).filter((s) => !dev?.url || !dev.url.includes(':' + s.port)).map((s) => ({ url: s.url, label: 'localhost:' + s.port, title: `${s.process} (pid ${s.pid}) in this project` }))]
          .map((s) => <button key={s.url} type="button" className="chip mono" title={s.title} aria-pressed={url.startsWith(s.url.replace(/\/$/, ''))} onClick={() => go(s.url)} style={{ height: 22, fontSize: 11 }}>{s.label}</button>)}
      </div>}
      {showLog && dev && <pre className="pre browser-log">{dev.log.join('\n') || '…'}</pre>}

      <div className="browser-view">
        {url ? <>
          <iframe key={url + ':' + nav} title="Browser" src={url} className="browser-frame" sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-modals allow-downloads allow-popups-to-escape-sandbox" allow="clipboard-read; clipboard-write; fullscreen" />
          {check && !check.embeddable && <div className="browser-blocked">
            <b>This page can’t be shown here</b>
            <span className="muted" style={{ fontSize: 12.5 }}>{check.reason}.</span>
            <a className="btn btn-p" href={url} target="_blank" rel="noreferrer noopener" style={{ textDecoration: 'none' }}>Open in your browser</a>
          </div>}
          {check && check.embeddable && check.status === 0 && <div className="browser-note muted">{check.reason}</div>}
        </> : (
          <div className="browser-empty">
            <b style={{ fontSize: 14 }}>Preview what you’re building</b>
            <span className="muted" style={{ fontSize: 12.5 }}>{dev?.command ? <>Run <span className="mono">{dev.command}</span> in {proj?.name || 'this project'} and its page opens here, or type an address above.</> : <>Set the command that starts {proj?.name || 'this project'}’s dev server above, or type an address — a port like <span className="mono">5173</span> works.</>}</span>
          </div>
        )}
      </div>
    </section>
  );
}
