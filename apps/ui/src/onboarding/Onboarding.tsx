// First-run setup: opencode, sessions, workspace, model, web search, memory, first project, look (R-29).
// Every step validates; "Skip setup" loads demo data. Joining an organization is not a step (R-08).
import { useEffect, useMemo, useState } from 'react';
import { get, post } from '../api.ts';
import { setState, refreshApp, refreshSnap, refreshModels, toast, useStore } from '../store.ts';
import { ACCENTS, AGENTS_TEMPLATES } from '../../../../packages/shared/src/defaults.ts';
import { nestFolders, titleCaseSegment, timeAgo, slugify, initialsOf } from '../../../../packages/shared/src/util.ts';
import { webConfigPreview } from '../../../../packages/shared/src/opencode-config.ts';
import { makeWorkspace } from '../../../../packages/shared/src/defaults.ts';
import type { WebMode, WebProvider, MemoryBackend, AccentId, SubscriptionStatus } from '../../../../packages/shared/src/types.ts';
import { Switch } from '../ui/Switch.tsx';
import { OpenDesignCard } from './OpenDesignCard.tsx';
import { Icon } from '../ui/Icon.tsx';

const STEPS = [['welcome', 'Welcome', 'what this is'], ['engine', 'opencode', 'start or connect'], ['agents', 'Subscriptions', 'Claude, Grok, opencode'], ['sessions', 'Sessions', 'import earlier chats'], ['workspace', 'Workspace', 'your personal space'], ['model', 'Model', 'a provider to start'], ['web', 'Web search', 'look things up'], ['memory', 'Memory', 'facts or a graph'], ['project', 'Project', 'your first project'], ['look', 'Look', 'theme']] as const;
type Prov = 'claude' | 'existing' | 'opencode' | 'anthropic' | 'openrouter' | 'ollama' | 'custom';
interface Sess { id: string; title: string; dir: string; updated: number }
type Test = null | 'busy' | { ok: boolean; msg: string };

export function Onboarding() {
  const theme = useStore((s) => s.app?.settings.theme || 'claude');
  const reduce = useStore((s) => !!s.app?.settings.reduceMotion);
  const [ob, setOb] = useState(0);
  const [obMax, setObMax] = useState(0);
  const [tour, setTour] = useState(0);
  // engine
  const [ocMode, setOcMode] = useState<'managed' | 'remote'>('managed');
  const [detect, setDetect] = useState<null | { found: boolean; path?: string; version?: string }>(null);
  const [binary, setBinary] = useState('opencode');
  const [ocUrl, setOcUrl] = useState('http://127.0.0.1:4096');
  const [ocUser, setOcUser] = useState('opencode');
  const [ocPass, setOcPass] = useState('');
  const [ocConn, setOcConn] = useState<Test>(null);
  const [odReady, setOdReady] = useState<'ok' | 'later' | null>(null);
  // subscriptions
  const [subs, setSubs] = useState<SubscriptionStatus[] | null>(null);
  const [subsOn, setSubsOn] = useState<Record<string, boolean>>({});
  const [subsErr, setSubsErr] = useState('');
  // sessions
  const [sessions, setSessions] = useState<Sess[] | null>(null);
  const [scanErr, setScanErr] = useState('');
  const [dirOff, setDirOff] = useState<Record<string, boolean>>({});
  const [sessOff, setSessOff] = useState<Record<string, boolean>>({});
  const [sync, setSync] = useState(true);
  // workspace
  const [wsName, setWsName] = useState('Personal');
  const [accent, setAccent] = useState<AccentId>('default');
  // model
  const [prov, setProv] = useState<Prov | null>(null);
  const [key, setKey] = useState('');
  const [url, setUrl] = useState('https://[YOUR-ENDPOINT]/v1');
  const [model, setModel] = useState('my-model');
  const [ollama, setOllama] = useState<Record<string, boolean>>({ 'qwen3-coder': true });
  const [provTest, setProvTest] = useState<Test>(null);
  // web
  const [webMode, setWebMode] = useState<WebMode>('exa');
  const [webProv, setWebProv] = useState<WebProvider>('Tavily');
  const [webKey, setWebKey] = useState('');
  const [searx, setSearx] = useState('http://localhost:8888');
  const [webFetch, setWebFetch] = useState(true);
  const [webAsk, setWebAsk] = useState(false);
  const [webTest, setWebTest] = useState<Test>(null);
  // memory
  const [mem, setMem] = useState<MemoryBackend>('both');
  const [folders, setFolders] = useState<string[]>(['~/code']);
  const [folderDraft, setFolderDraft] = useState('');
  const [watch, setWatch] = useState(true);
  // project
  const [projName, setProjName] = useState('My project');
  const [projFolder, setProjFolder] = useState('~/code/my-project');
  const [tpl, setTpl] = useState('Software');
  const [agents, setAgents] = useState<string | null>(null);
  // look
  const [follow, setFollow] = useState(false);
  const [busy, setBusy] = useState(false);

  const key0 = STEPS[ob][0];
  const ocHas = ocMode === 'remote' ? (ocConn !== null && ocConn !== 'busy' && ocConn.ok) : !!detect?.found;
  const provK: Prov = prov || (ocHas && sessions && sessions.length ? 'existing' : 'opencode');
  const claudeSub = subs?.find((s) => s.id === 'claude');
  const claudeReady = !!claudeSub?.installed && !!claudeSub.signedIn;
  const tested = (provTest !== null && provTest !== 'busy' && provTest.ok) || provK === 'existing' || provK === 'opencode' || (provK === 'claude' && claudeReady);
  const agentsText = agents ?? AGENTS_TEMPLATES[tpl](projName || 'My project');

  const recheck = () => { setDetect(null); get(`/api/onboarding/detect?binary=${encodeURIComponent(binary)}`).then(setDetect).catch(() => setDetect({ found: false })); };
  useEffect(recheck, [binary]);

  // Scan sessions when entering the sessions step.
  useEffect(() => {
    if (key0 !== 'sessions' || sessions !== null || !ocHas) return;
    post<{ sessions: Sess[]; error?: string }>('/api/onboarding/scan', ocMode === 'remote' ? { mode: 'remote', url: ocUrl, username: ocUser, password: ocPass } : { mode: 'managed', binary })
      .then((r) => { setSessions(r.sessions); setScanErr(r.error || ''); })
      .catch((e) => { setSessions([]); setScanErr(e.message); });
  }, [key0, sessions, ocHas]);

  // Detect the three subscriptions when entering their step.
  useEffect(() => {
    if (key0 !== 'agents' || subs !== null) return;
    get<SubscriptionStatus[]>('/api/subscriptions?force=1')
      .then((rows) => { setSubs(rows); setSubsOn(Object.fromEntries(rows.filter((r) => r.installed && r.signedIn).map((r) => [r.id, true]))); })
      .catch((e) => { setSubs([]); setSubsErr((e as Error).message); });
  }, [key0, subs]);

  const found = ocHas ? sessions || [] : [];
  const sessOn = (x: Sess) => !dirOff[x.dir] && !sessOff[x.id];
  const nSel = found.filter(sessOn).length;
  const groups = useMemo(() => {
    const dirs = Array.from(new Set(found.map((x) => x.dir)));
    const nest = nestFolders(dirs);
    return nest.map(({ dir, parent }) => ({ dir, parent, items: found.filter((x) => x.dir === dir).sort((a, b) => b.updated - a.updated) }));
  }, [found]);

  const connectOc = async () => {
    setOcConn('busy'); setSessions(null);
    type Remote = { ok: boolean; version?: string; sessions?: number; folders?: number; providers?: number; error?: string };
    const r: Remote = await post<Remote>('/api/onboarding/test-remote', { url: ocUrl, username: ocUser, password: ocPass }).catch((e) => ({ ok: false, error: (e as Error).message }));
    setOcConn(r.ok ? { ok: true, msg: `Connected · opencode ${r.version} · ${r.sessions} sessions in ${r.folders} folders · ${r.providers} providers configured` } : { ok: false, msg: r.error || 'Can’t reach that server' });
  };
  const testProv = async () => {
    setProvTest('busy');
    type Prov = { ok: boolean; error?: string; ms?: number; models?: string[] };
    const r: Prov = await post<Prov>('/api/onboarding/test-provider', { kind: provK, key, url }).catch((e) => ({ ok: false, error: (e as Error).message }));
    setProvTest(r.ok ? { ok: true, msg: provK === 'ollama' ? `Connected · ${(r.models || []).length || Object.values(ollama).filter(Boolean).length} models ready` : `Connected${r.ms ? ' · ' + r.ms + ' ms' : ''}` } : { ok: false, msg: r.error || 'That doesn’t look right — check the key and try again.' });
  };
  const testWeb = async () => {
    setWebTest('busy');
    type Web = { ok: boolean; note?: string; error?: string };
    const r: Web = await post<Web>('/api/onboarding/test-web', { mode: webMode, provider: webProv, key: webKey, url: searx }).catch((e) => ({ ok: false, error: (e as Error).message }));
    setWebTest(r.ok ? { ok: true, msg: r.note || 'OK' } : { ok: false, msg: r.error || 'Failed' });
  };

  const blocked = (key0 === 'engine' && ((ocMode === 'remote' && !ocHas) || (ocMode === 'managed' && !detect?.found) || !odReady)) || (key0 === 'workspace' && !wsName.trim()) || (key0 === 'project' && (!projName.trim() || !projFolder.trim()));
  const hint = blocked ? ({ engine: ocMode === 'remote' && !ocHas ? 'Connect to the server first' : (ocMode === 'managed' && !detect?.found ? 'Install opencode, or connect to a server' : 'Install Open Design, or choose Install later'), workspace: 'Give the workspace a name', project: 'Name your project and pick a folder' } as Record<string, string>)[key0] || '' : '';
  const nextLabel = ob === STEPS.length - 1 ? 'Open harness' : ob === 0 ? 'Get started' : key0 === 'model' && !tested ? 'Set up later' : key0 === 'sessions' && found.length && !nSel ? 'Start fresh' : key0 === 'web' && webMode === 'off' ? 'Skip' : 'Continue';

  const finish = async () => {
    setBusy(true);
    try {
      const ids = found.filter(sessOn).map((s) => s.id);
      const skipped = found.filter((s) => !sessOn(s)).map((s) => s.id);
      const r = await post<{ workspace: string; imported: { chats: number; failed: string[] } }>('/api/onboarding/finish', {
        workspaceName: wsName.trim(), accent,
        engine: { mode: ocMode, url: ocUrl, username: ocUser, password: ocPass, binary },
        provider: provK === 'claude' ? { kind: 'opencode' } : { kind: provK, key, url, model, ollamaModels: Object.keys(ollama).filter((k) => ollama[k]) },
        defaultAgent: provK === 'claude' ? 'claude' : undefined,
        web: { mode: webMode, provider: webProv, searxngUrl: searx, fetch: webFetch, ask: webAsk, showSources: true, locked: false, key: webKey },
        memory: mem, graphifyFolders: mem === 'facts' ? [] : folders, graphifyWatch: watch,
        project: { name: projName.trim(), folder: projFolder.trim(), agents: agentsText },
        importIds: ids, skippedIds: skipped, sync, theme, reduceMotion: reduce, followSystem: follow,
        subscriptions: Array.from(new Set([...Object.keys(subsOn).filter((k) => subsOn[k]), ...(provK === 'claude' ? ['claude'] : [])])),
      });
      await refreshApp(); await refreshSnap(); refreshModels();
      setState({ view: 'home' });
      toast(r.imported.chats ? `Setup done — ${r.imported.chats} chats imported` : 'Setup done — welcome to harness');
      if (r.imported.failed?.length) toast(`${r.imported.failed.length} sessions could not be copied`, 'error');
    } catch (e) { toast((e as Error).message, 'error'); } finally { setBusy(false); }
  };
  const next = () => { if (ob === STEPS.length - 1) { finish(); return; } setOb(ob + 1); setObMax(Math.max(obMax, ob + 1)); };
  const demo = async () => { setBusy(true); try { await post('/api/onboarding/demo'); await refreshApp(); await refreshSnap(); refreshModels(); setState({ view: 'home', open: {} }); toast('Demo data loaded'); } catch (e) { toast((e as Error).message, 'error'); } finally { setBusy(false); } };
  const setTheme = (t: 'claude' | 'opencode') => fetchSettings({ theme: t });
  const setReduce = (v: boolean) => fetchSettings({ reduceMotion: v });

  const wsPreview = useMemo(() => { const w = makeWorkspace('preview', wsName || 'Personal', 'personal', '~/.harness'); w.web = { ...w.web, mode: webMode, provider: webProv, searxngUrl: searx, fetch: webFetch, ask: webAsk }; return w; }, [wsName, webMode, webProv, searx, webFetch, webAsk]);
  const msg = (t: Test, idle: string) => (t === 'busy' ? { text: '…', color: 'var(--muted)' } : t ? { text: t.msg, color: t.ok ? 'var(--success-text)' : 'var(--error-text)' } : { text: idle, color: 'var(--muted)' });

  return (
    <div className="onb-wrap" style={{ flex: '1 1 auto', display: 'flex', alignItems: 'center', justifyContent: 'center', background: 'var(--bg-deep)', padding: 32, minHeight: 0 }}>
      <div className="card onb-card" role="dialog" aria-label="Set up harness" style={{ width: 1080, maxWidth: '100%', height: 760, maxHeight: '100%', display: 'flex', overflow: 'hidden', background: 'var(--bg0)', boxShadow: '0 24px 60px rgba(0,0,0,0.16)' }}>
        <aside className="onb-steps" style={{ flex: '0 0 280px', padding: '28px 22px', background: 'var(--bg1)', borderRight: '1px solid var(--bg2)', display: 'flex', flexDirection: 'column', gap: 18 }}>
          <span className="mono" style={{ fontWeight: 600, fontSize: 16 }}>harness</span>
          <ol style={{ margin: 0, padding: 0, listStyle: 'none', display: 'flex', flexDirection: 'column', gap: 2 }}>
            {STEPS.map((st, k) => (
              <li key={st[0]}><button type="button" onClick={() => setOb(k)} disabled={k > obMax} className="navrow" aria-current={k === ob ? 'step' : 'false'} style={{ gap: 12, padding: '7px 10px' }}>
                <span style={{ width: 22, height: 22, borderRadius: '50%', border: `2px solid ${k <= ob ? 'var(--accent-p)' : 'var(--line2)'}`, background: k < ob ? 'var(--accent-p-fill)' : 'transparent', color: 'var(--on-accent)', fontSize: 11, fontWeight: 700, display: 'flex', alignItems: 'center', justifyContent: 'center', flex: '0 0 auto', transition: 'all .3s ease' }}>{k < ob ? <Icon name="check" size={13} stroke={2.5} /> : k + 1}</span>
                <span style={{ display: 'flex', flexDirection: 'column', textAlign: 'left' }}><span style={{ fontSize: 13, fontWeight: 500, color: k === ob ? 'var(--text)' : 'var(--muted)' }}>{st[1]}</span><span className="muted" style={{ fontSize: 11 }}>{st[2]}</span></span>
              </button></li>
            ))}
          </ol>
          <span className="muted" style={{ marginTop: 'auto', fontSize: 12 }}>Everything here can be changed later in Settings.</span>
          <button type="button" className="btn" onClick={demo} disabled={busy} style={{ justifyContent: 'center' }}>Skip setup · use demo data</button>
        </aside>

        <main className="onb-main" style={{ flex: '1 1 auto', minWidth: 0, padding: '34px 40px 24px', display: 'flex', flexDirection: 'column', gap: 16, overflowY: 'auto' }}>
          {key0 === 'welcome' && (
            <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 18, paddingTop: 40 }}>
              <span className="lbl">Welcome</span>
              <h1 className="disp" style={{ fontSize: 52, lineHeight: 1.04 }}>Your workspaces,<br />your models,<br />your machine.</h1>
              <p style={{ margin: 0, maxWidth: 560, fontSize: 16, color: 'var(--text2)' }}>Harness keeps personal and work completely apart. Chats run on opencode, artifacts are made with Open Design, and everything stays on this computer unless you share it.</p>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 12, maxWidth: 720 }}>
                {[['Separate workspaces', 'Own chats, memory, models and connectors each'], ['Projects all the way down', 'Every chat lives in a project; projects nest'], ['Artifacts and loops', 'Docs, decks, prototypes; drag-and-drop agent graphs']].map((t, i) => (
                  <button key={t[0]} type="button" className="opt" onClick={() => setTour(i + 1)} aria-pressed={tour === i + 1}><b>{t[0]}</b><span className="muted" style={{ fontSize: 12 }}>{t[1]}</span></button>
                ))}
              </div>
              <span className="rise muted" key={tour} style={{ fontSize: 13, maxWidth: 640 }}>{['Pick a card to see how it works.', 'Each workspace runs its own engine with its own config folder. Switching changes which one you talk to; nothing is copied across.', 'Start a chat and pick the project it belongs to. A sub-project is a folder inside its parent and inherits its instructions.', 'Ask for a doc, deck or prototype in any chat and it appears beside the conversation. Loops are built by dragging blocks.'][tour]}</span>
            </div>
          )}

          {key0 === 'engine' && (
            <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
              <h1 className="disp" style={{ fontSize: 36 }}>Connect opencode</h1>
              <p style={{ margin: 0, color: 'var(--text2)' }}>Every chat here is an opencode session. Let harness start opencode for this workspace, or connect to a server you already run — then the same sessions show here and in the opencode TUI.</p>
              <div role="radiogroup" aria-label="How to run opencode" style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 10 }}>
                <button type="button" className="opt" role="radio" aria-checked={ocMode === 'managed'} aria-pressed={ocMode === 'managed'} onClick={() => { setOcMode('managed'); setSessions(null); }}><b>Start it for me</b><span className="muted" style={{ fontSize: 12 }}>Harness runs <span className="mono">opencode serve</span> per workspace on this computer</span></button>
                <button type="button" className="opt" role="radio" aria-checked={ocMode === 'remote'} aria-pressed={ocMode === 'remote'} onClick={() => { setOcMode('remote'); setSessions(null); }}><b>Connect to a running server</b><span className="muted" style={{ fontSize: 12 }}>Local or remote <span className="mono">opencode serve</span>, with its sessions and config</span></button>
              </div>
              {ocMode === 'managed' ? (
                <div className="card rise" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 10 }}>
                  <span className="row" style={{ gap: 12 }}><span className={'dot ' + (detect ? '' : 'pulse')} style={{ width: 10, height: 10, background: detect?.found ? 'var(--success)' : detect ? 'var(--error)' : 'var(--warning)' }} /><span className="grow"><b>opencode</b> · {detect ? (detect.found ? `found at ${detect.path} · ${detect.version}` : 'not found on this computer') : 'looking for opencode…'}</span><button type="button" className="btn" onClick={recheck}>Check again</button></span>
                  <label className="field" style={{ paddingLeft: 22, maxWidth: 520 }}>Binary<input className="inp mono" value={binary} onChange={(e) => setBinary(e.target.value)} aria-label="opencode binary" /></label>
                  <span className="muted" style={{ fontSize: 12, paddingLeft: 22 }}>Bound to 127.0.0.1 with a random password per launch. Attach the TUI any time from a chat’s menu.</span>
                  {detect && !detect.found && <span style={{ fontSize: 12, paddingLeft: 22, color: 'var(--error-text)' }}>Install opencode (<span className="mono">curl -fsSL https://opencode.ai/install | bash</span>) and press Check again.</span>}
                </div>
              ) : (
                <div className="card rise" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 10 }}>
                  <div style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 2fr) minmax(0, 1fr) minmax(0, 1fr)', gap: 10 }}>
                    <label className="field">Server URL<input className="inp mono" value={ocUrl} onChange={(e) => { setOcUrl(e.target.value); setOcConn(null); }} /></label>
                    <label className="field">Username<input className="inp mono" value={ocUser} onChange={(e) => { setOcUser(e.target.value); setOcConn(null); }} /></label>
                    <label className="field">Password<input className="inp mono" type="password" value={ocPass} onChange={(e) => { setOcPass(e.target.value); setOcConn(null); }} placeholder="OPENCODE_SERVER_PASSWORD" /></label>
                  </div>
                  <div className="row" style={{ gap: 10 }}>
                    <button type="button" className="btn" onClick={connectOc} disabled={ocConn === 'busy'}>{ocConn === 'busy' ? 'Connecting…' : ocConn && ocConn.ok ? 'Reconnect' : 'Connect'}</button>
                    <span className="rise" style={{ fontSize: 13, color: msg(ocConn, 'Connect to continue.').color }}>{ocConn === 'busy' ? 'GET /api/info …' : msg(ocConn, 'Connect to continue.').text}</span>
                  </div>
                  <span className="muted" style={{ fontSize: 12 }}>The password goes to your keychain and is sent as basic auth. For a remote machine use HTTPS or an SSH tunnel.</span>
                </div>
              )}
              <OpenDesignCard onReady={setOdReady} ready={odReady} />
            </div>
          )}

          {key0 === 'agents' && (
            <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
              <h1 className="disp" style={{ fontSize: 36 }}>Connect your subscriptions</h1>
              <p style={{ margin: 0, color: 'var(--text2)' }}>Use the accounts you already pay for. Harness copies the login on this computer into {wsName.trim() || 'this workspace'}’s own folder — no token is written to a config file, and another workspace never sees this history.</p>
              {subs === null && <p className="muted pulse">Looking for opencode, Claude Code and Grok…</p>}
              {subsErr && <span style={{ fontSize: 12, color: 'var(--error-text)' }}>{subsErr}</span>}
              <div className="row"><span className="grow" /><button type="button" className="btn" onClick={() => setSubs(null)}>Check again</button></div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                {(subs || []).map((s) => {
                  const can = s.installed && s.signedIn;
                  const on = !!subsOn[s.id];
                  return (
                    <button key={s.id} type="button" className="opt" aria-pressed={on} disabled={!can} onClick={() => setSubsOn({ ...subsOn, [s.id]: !on })} style={{ opacity: can ? 1 : 0.6 }}>
                      <span className="row" style={{ gap: 10 }}>
                        <b className="grow">{s.name}</b>
                        <span className="muted" style={{ fontSize: 12 }}>{s.installed ? (s.version || 'installed') : 'not installed'}</span>
                        <span className="mono" style={{ fontSize: 11, color: on ? 'var(--accent-p-text)' : 'var(--muted)' }}>{on ? <><Icon name="check" size={11} stroke={2.5} style={{ verticalAlign: -1 }} /> connect</> : can ? 'connect' : 'unavailable'}</span>
                      </span>
                      <span className="muted" style={{ fontSize: 12 }}>{s.detail}{s.account ? ' · ' + s.account : ''}</span>
                    </button>
                  );
                })}
              </div>
              <p className="muted" style={{ margin: 0, fontSize: 12 }}>Optional. You can connect or disconnect any of them later in Settings › Agents. If a subscription is not signed in yet, run its login command on this computer and press Continue again.</p>
            </div>
          )}

          {key0 === 'sessions' && (
            <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
              <h1 className="disp" style={{ fontSize: 36 }}>Bring your opencode chats</h1>
              <p style={{ margin: 0, color: 'var(--text2)' }}>{!ocHas ? (ocMode === 'remote' ? 'Connect to a server on the previous step to see its sessions.' : 'Harness looks for sessions opencode already saved on this computer.') : sessions === null ? 'Looking for sessions…' : found.length ? `Found ${found.length} sessions in ${groups.length} folders ${ocMode === 'remote' ? 'on ' + ocUrl : 'in this computer’s opencode data'}. Each folder becomes a project (nested by path), and each session becomes a chat in it with its full history.` : 'No earlier sessions were found.'}</p>
              {scanErr && <span style={{ fontSize: 12, color: 'var(--error-text)' }}>{scanErr}</span>}
              {sessions !== null && !found.length && <div className="card rise" style={{ padding: 18, display: 'flex', flexDirection: 'column', gap: 6 }}><b>No earlier sessions</b><span className="muted" style={{ fontSize: 13 }}>Chats you start here are new opencode sessions; you can open any of them in the TUI later.</span></div>}
              <div style={{ display: 'flex', flexDirection: 'column', gap: 10, maxHeight: 360, overflowY: 'auto' }}>
                {groups.map((g) => {
                  const on = !dirOff[g.dir];
                  const target = !on ? 'skipped' : g.parent ? `sub-project “${titleCaseSegment(g.dir)}” inside ${titleCaseSegment(g.parent)}` : `project “${titleCaseSegment(g.dir)}”`;
                  return (
                    <div key={g.dir} className="card rise" style={{ padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 6 }}>
                      <span className="row" style={{ gap: 10 }}><span className="mono ellipsis" style={{ fontSize: 12.5, fontWeight: 600 }}>{g.dir}</span><span className="muted grow" style={{ fontSize: 12 }}>→ {target}</span><Switch on={on} label="Import this folder" onChange={() => setDirOff({ ...dirOff, [g.dir]: on })} /></span>
                      {g.items.map((x) => { const io = sessOn(x); return (
                        <button key={x.id} type="button" className="navrow" onClick={() => setSessOff({ ...sessOff, [x.id]: io })} aria-pressed={io} disabled={!on} style={{ gap: 10, padding: '5px 8px' }}><span style={{ width: 16, color: 'var(--accent-p-text)' }}>{io ? <Icon name="check" size={13} stroke={2.5} /> : ''}</span><span className="grow ellipsis">{x.title}</span><span className="mono muted" style={{ fontSize: 11 }}>{x.id.slice(0, 12)}… · {timeAgo(x.updated)}</span></button>
                      ); })}
                    </div>
                  );
                })}
              </div>
              {found.length > 0 && <>
                <label className="row" style={{ justifyContent: 'space-between', maxWidth: 640, fontSize: 13 }}><span>Keep in sync both ways<span className="muted" style={{ display: 'block', fontSize: 12 }}>Sessions started in the TUI appear here; chats started here appear in the TUI.</span></span><Switch on={sync} label="Sync sessions" onChange={() => setSync(!sync)} /></label>
                <span className="row" style={{ gap: 10, fontSize: 13 }}><b>{nSel === 1 ? '1 chat will be imported' : nSel + ' chats will be imported'}</b><button type="button" className="link" onClick={() => { setDirOff({}); setSessOff({}); }}>Select all</button><button type="button" className="link" onClick={() => setDirOff(Object.fromEntries(groups.map((g) => [g.dir, true])))}>Start fresh</button></span>
              </>}
            </div>
          )}

          {key0 === 'workspace' && (
            <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
              <h1 className="disp" style={{ fontSize: 36 }}>Your personal workspace</h1>
              <p style={{ margin: 0, color: 'var(--text2)' }}>Your own keys, accounts and files. Nothing here is visible from a work workspace. You can join an organization later from + in the rail.</p>
              <label className="field" style={{ maxWidth: 420 }}>Name<input className="inp" value={wsName} onChange={(e) => setWsName(e.target.value)} /></label>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                <span style={{ fontSize: 13, fontWeight: 500 }}>Accent</span>
                <div className="row" style={{ gap: 10 }}>{ACCENTS.map((a) => <button key={a.id} type="button" aria-label={a.name} aria-pressed={accent === a.id} onClick={() => setAccent(a.id)} style={{ width: 34, height: 34, borderRadius: '50%', border: 0, background: a.hex, boxShadow: accent === a.id ? '0 0 0 2px var(--bg0), 0 0 0 4px var(--text)' : 'none' }} />)}</div>
              </div>
              <div className="card" data-accent={accent} style={{ padding: '14px 16px', display: 'flex', alignItems: 'center', gap: 14, maxWidth: 520 }}>
                <span className="mono" style={{ width: 40, height: 40, borderRadius: 10, background: 'var(--bg3)', color: accent === 'default' ? 'var(--accent-p-text)' : ACCENTS.find((a) => a.id === accent)?.hex, display: 'flex', alignItems: 'center', justifyContent: 'center', fontWeight: 700 }}>{initialsOf(wsName || 'P')}</span>
                <span style={{ display: 'flex', flexDirection: 'column' }}><b>{wsName.trim() || 'Personal'}</b><span className="mono muted" style={{ fontSize: 12 }}>~/.harness/ws/{slugify(wsName, 'personal')}</span></span>
              </div>
            </div>
          )}

          {key0 === 'model' && (
            <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
              <h1 className="disp" style={{ fontSize: 36 }}>Connect a model</h1>
              <p style={{ margin: 0, color: 'var(--text2)' }}>For {wsName.trim() || 'Personal'}. Keys go to your keychain; config files only hold a reference.</p>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 10 }}>
                {((claudeSub ? ['claude'] : []) as Prov[]).concat(ocHas ? ['existing'] : [], ['opencode', 'anthropic', 'openrouter', 'ollama', 'custom']).map((k) => (
                  <button key={k} type="button" className="opt" aria-pressed={provK === k} disabled={k === 'claude' && !claudeReady} style={k === 'claude' && !claudeReady ? { opacity: 0.6 } : undefined} onClick={() => { setProv(k); setProvTest(null); }}><b>{{ claude: 'Claude subscription', existing: 'From opencode', opencode: 'OpenCode Zen', anthropic: 'Anthropic', openrouter: 'OpenRouter', ollama: 'Ollama', custom: 'Custom endpoint' }[k]}</b><span className="muted" style={{ fontSize: 12 }}>{{ claude: claudeReady ? 'Pro / Max · via Claude Code' : claudeSub?.installed ? 'run “claude auth login” first' : 'needs Claude Code installed', existing: 'reuse its providers', opencode: 'free models, no key', anthropic: 'API key', openrouter: 'one key, many models', ollama: 'local, no key', custom: 'any OpenAI-compatible URL' }[k]}</span></button>
                ))}
              </div>
              <div className="card rise" style={{ padding: 16, display: 'flex', flexDirection: 'column', gap: 12 }}>
                {(provK === 'anthropic' || provK === 'openrouter' || provK === 'custom') && <label className="field">{provK === 'custom' ? 'API key (optional)' : (provK === 'anthropic' ? 'Anthropic' : 'OpenRouter') + ' API key'}<input className="inp mono" type="password" value={key} onChange={(e) => { setKey(e.target.value); setProvTest(null); }} placeholder="paste your key" /></label>}
                {provK === 'custom' && <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 10 }}><label className="field">Base URL<input className="inp mono" value={url} onChange={(e) => { setUrl(e.target.value); setProvTest(null); }} /></label><label className="field">Model id<input className="inp mono" value={model} onChange={(e) => setModel(e.target.value)} /></label></div>}
                {provK === 'ollama' && <><span style={{ fontSize: 13 }}>Ollama answers on <span className="mono">localhost:11434</span>. Pick the models to use:</span><div className="row" style={{ flexWrap: 'wrap' }}>{['qwen3-coder', 'llama3.3', 'mistral-small'].map((m) => <button key={m} type="button" className="chip mono" aria-pressed={!!ollama[m]} onClick={() => setOllama({ ...ollama, [m]: !ollama[m] })}><Icon name={ollama[m] ? 'check' : 'plus'} size={12} stroke={2.25} /> {m}</button>)}</div></>}
                {provK === 'existing' && <span style={{ fontSize: 13 }}>Uses the providers your opencode already has — keys stay where opencode keeps them.</span>}
                {provK === 'claude' && <span style={{ fontSize: 13 }}>Chats run in Claude Code on your Claude plan{claudeSub?.account ? ' (' + claudeSub.account + ')' : ''}; its models (Opus, Sonnet, Haiku) show in the chat picker. The login is copied into this workspace only. OpenCode Zen’s free models stay available for opencode chats, routines and the artifact pipeline.</span>}
                {provK === 'opencode' && <span style={{ fontSize: 13 }}>OpenCode Zen’s free models answer without a key. Add your own providers any time in Settings › Model providers.</span>}
                <div className="row" style={{ gap: 10 }}>{provK !== 'claude' && <button type="button" className="btn" onClick={testProv} disabled={provTest === 'busy'}>{provTest === 'busy' ? 'Testing…' : 'Test connection'}</button>}<span className="rise" style={{ fontSize: 13, color: msg(provTest, tested ? 'Ready to use.' : 'Test before continuing, or set it up later.').color }}>{msg(provTest, tested ? 'Ready to use.' : 'Test before continuing, or set it up later.').text}</span></div>
              </div>
            </div>
          )}

          {key0 === 'web' && (
            <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <h1 className="disp" style={{ fontSize: 36 }}>Web search</h1>
              <p style={{ margin: 0, color: 'var(--text2)' }}>Let the agent look things up for {wsName.trim() || 'Personal'}. opencode has two tools: <span className="mono">websearch</span> finds pages and <span className="mono">webfetch</span> reads one by URL. You can turn search on or off per chat.</p>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0, 1fr))', gap: 10 }}>
                {([['off', 'Off', 'No search; pasted links only if page reading is on'], ['exa', 'Built-in', 'opencode’s websearch (Exa), no key'], ['mcp', 'Search provider', 'Tavily, Brave or Firecrawl over MCP'], ['searxng', 'Self-hosted', 'Your SearXNG; queries stay on your network']] as [WebMode, string, string][]).map((w) => (
                  <button key={w[0]} type="button" className="opt" aria-pressed={webMode === w[0]} onClick={() => { setWebMode(w[0]); setWebTest(null); }}><b>{w[1]}</b><span className="muted" style={{ fontSize: 12 }}>{w[2]}</span></button>
                ))}
              </div>
              <div className="card rise" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 10 }}>
                {webMode === 'mcp' && <><div className="row"><span style={{ fontSize: 13, fontWeight: 500 }}>Provider</span>{(['Tavily', 'Brave', 'Firecrawl'] as WebProvider[]).map((p) => <button key={p} type="button" className="chip" aria-pressed={webProv === p} onClick={() => { setWebProv(p); setWebTest(null); }}>{p}</button>)}</div>
                  <label className="field" style={{ maxWidth: 460 }}>{webProv} API key<input className="inp mono" type="password" value={webKey} onChange={(e) => { setWebKey(e.target.value); setWebTest(null); }} placeholder="paste your key" /></label></>}
                {webMode === 'searxng' && <label className="field" style={{ maxWidth: 460 }}>SearXNG URL<input className="inp mono" value={searx} onChange={(e) => { setSearx(e.target.value); setWebTest(null); }} /></label>}
                <label className="row" style={{ justifyContent: 'space-between', fontSize: 13 }}>Read pages by URL (webfetch)<Switch on={webFetch} label="webfetch" onChange={() => setWebFetch(!webFetch)} /></label>
                <label className="row" style={{ justifyContent: 'space-between', fontSize: 13 }}>Ask before each search<Switch on={webAsk} label="Ask before searching" onChange={() => setWebAsk(!webAsk)} /></label>
                {webMode !== 'off' && <div className="row" style={{ gap: 10 }}><button type="button" className="btn" onClick={testWeb} disabled={webTest === 'busy'}>{webTest === 'busy' ? 'Checking…' : 'Try it'}</button><span className="rise" style={{ fontSize: 13, color: msg(webTest, 'Optional — check it works.').color }}>{msg(webTest, 'Optional — check it works.').text}</span></div>}
                <pre className="pre">{webConfigPreview(wsPreview)}</pre>
              </div>
            </div>
          )}

          {key0 === 'memory' && (
            <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
              <h1 className="disp" style={{ fontSize: 36 }}>How should it remember?</h1>
              <p style={{ margin: 0, color: 'var(--text2)' }}>For {wsName.trim() || 'Personal'}. Each workspace picks its own memory.</p>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                {([['facts', 'Facts', 'Short statements you approve, kept as files'], ['graph', 'Graphify', 'A knowledge graph of your project folders, queried over MCP'], ['both', 'Graphify + facts', 'Recommended for code-heavy projects'], ['mcp', 'Custom MCP', 'Any memory MCP server you run']] as [MemoryBackend, string, string][]).map((m) => (
                  <button key={m[0]} type="button" className="opt" aria-pressed={mem === m[0]} onClick={() => setMem(m[0])}><b>{m[1]}</b><span className="muted" style={{ fontSize: 12 }}>{m[2]}</span></button>
                ))}
              </div>
              {(mem === 'graph' || mem === 'both') && (
                <div className="card rise" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 8 }}>
                  <span style={{ fontSize: 13, fontWeight: 600 }}>Folders Graphify should map</span>
                  {folders.map((f) => <span key={f} className="row" style={{ gap: 10 }}><span className="mono grow" style={{ fontSize: 12 }}>{f}</span><button type="button" className="btn" onClick={() => setFolders(folders.filter((x) => x !== f))} style={{ height: 26 }}>Remove</button></span>)}
                  <span className="row"><input className="inp mono grow" value={folderDraft} onChange={(e) => setFolderDraft(e.target.value)} aria-label="Folder path" placeholder="~/code/another" /><button type="button" className="btn" onClick={() => { const f = folderDraft.trim(); if (f && !folders.includes(f)) setFolders([...folders, f]); setFolderDraft(''); }}>Add folder</button></span>
                  <label className="row" style={{ justifyContent: 'space-between', fontSize: 13 }}>Keep the graph updated as code changes<Switch on={watch} label="Watch" onChange={() => setWatch(!watch)} /></label>
                </div>
              )}
            </div>
          )}

          {key0 === 'project' && (
            <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
              <h1 className="disp" style={{ fontSize: 36 }}>Your first project</h1>
              <p style={{ margin: 0, color: 'var(--text2)' }}>Chats always belong to a project. A project is a folder; sub-projects are folders inside it. A <b>General</b> project is made for quick chats.</p>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 12 }}>
                <label className="field">Project name<input className="inp" value={projName} onChange={(e) => { setProjName(e.target.value); if (!agents) setProjFolder('~/code/' + slugify(e.target.value, 'project')); }} /></label>
                <label className="field">Folder<input className="inp mono" value={projFolder} onChange={(e) => setProjFolder(e.target.value)} /></label>
              </div>
              <div className="row"><span style={{ fontSize: 13, fontWeight: 500 }}>AGENTS.md template</span>{Object.keys(AGENTS_TEMPLATES).map((t) => <button key={t} type="button" className="chip" aria-pressed={tpl === t} onClick={() => { setTpl(t); setAgents(null); }}>{t}</button>)}</div>
              <label className="field">Project instructions · saved as AGENTS.md in the folder
                <textarea className="inp mono" rows={7} value={agentsText} onChange={(e) => setAgents(e.target.value)} style={{ fontSize: 12 }} />
              </label>
            </div>
          )}

          {key0 === 'look' && (
            <div className="rise" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
              <h1 className="disp" style={{ fontSize: 36 }}>Choose a look</h1>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 14 }}>
                <button type="button" className="opt" aria-pressed={theme === 'claude'} onClick={() => setTheme('claude')} style={{ padding: 0, overflow: 'hidden' }}>
                  <span style={{ display: 'block', height: 130, background: '#faf9f5', position: 'relative', width: '100%' }}><span style={{ position: 'absolute', left: 18, top: 16, fontFamily: "'Cormorant Garamond', Georgia, serif", fontSize: 30, color: '#141413' }}>Aa</span><span style={{ position: 'absolute', right: 18, bottom: 18, width: 70, height: 26, borderRadius: 6, background: '#cc785c' }} /><span style={{ position: 'absolute', left: 18, bottom: 18, width: 120, height: 10, borderRadius: 4, background: '#e8e0d2' }} /></span>
                  <span style={{ display: 'block', padding: '10px 14px' }}><b>Claude</b> · warm cream, serif headings</span>
                </button>
                <button type="button" className="opt" aria-pressed={theme === 'opencode'} onClick={() => setTheme('opencode')} style={{ padding: 0, overflow: 'hidden' }}>
                  <span style={{ display: 'block', height: 130, background: '#0a0a0a', position: 'relative', width: '100%' }}><span style={{ position: 'absolute', left: 18, top: 16, fontFamily: "'JetBrains Mono', monospace", fontSize: 24, color: '#eeeeee' }}>Aa</span><span style={{ position: 'absolute', right: 18, bottom: 18, width: 70, height: 26, borderRadius: 6, background: '#fab283' }} /><span style={{ position: 'absolute', left: 18, bottom: 18, width: 120, height: 10, borderRadius: 4, background: '#282828' }} /></span>
                  <span style={{ display: 'block', padding: '10px 14px' }}><b>opencode</b> · dark, monospace accents</span>
                </button>
              </div>
              <label className="row" style={{ justifyContent: 'space-between', maxWidth: 520, fontSize: 13 }}>Follow system light and dark<Switch on={follow} label="Follow system" onChange={() => { setFollow(!follow); fetchSettings({ followSystem: !follow }); }} /></label>
              <label className="row" style={{ justifyContent: 'space-between', maxWidth: 520, fontSize: 13 }}>Reduce motion<Switch on={reduce} label="Reduce motion" onChange={() => setReduce(!reduce)} /></label>
              <div className="card" style={{ padding: '14px 16px', maxWidth: 620, fontSize: 13, display: 'flex', flexDirection: 'column', gap: 4 }}>
                <span className="lbl">Summary</span>
                {[['Workspace', wsName.trim() || 'Personal'], ['opencode', ocMode === 'remote' ? 'connected · ' + ocUrl : 'started by harness'], ['Subscriptions', Object.keys(subsOn).filter((k) => subsOn[k]).length ? Object.keys(subsOn).filter((k) => subsOn[k]).join(', ') : 'none yet'], ['Open Design', odReady === 'ok' ? 'ready' : 'install later'], ['Imported chats', String(nSel)], ['Model', { claude: 'Claude subscription', existing: 'From opencode', opencode: 'OpenCode Zen', anthropic: 'Anthropic', openrouter: 'OpenRouter', ollama: 'Ollama', custom: 'Custom endpoint' }[provK] + (provTest !== null && provTest !== 'busy' && provTest.ok ? ' · tested' : '')], ['Web search', { off: 'off', exa: 'built-in (Exa)', mcp: webProv, searxng: 'SearXNG' }[webMode]], ['Memory', { facts: 'Facts', graph: 'Graphify', both: 'Graphify + facts', mcp: 'Custom MCP' }[mem]], ['First project', projName + ' · ' + projFolder]].map((x) => <span key={x[0]} className="row" style={{ justifyContent: 'space-between', gap: 12 }}><span className="muted">{x[0]}</span><span>{x[1]}</span></span>)}
              </div>
            </div>
          )}

          <div style={{ marginTop: 'auto', display: 'flex', alignItems: 'center', gap: 10, paddingTop: 10 }}>
            <div style={{ flex: '1 1 auto', height: 4, borderRadius: 999, background: 'var(--bg3)', overflow: 'hidden' }}><div style={{ height: 4, width: Math.round((ob + 1) / STEPS.length * 100) + '%', background: 'var(--accent-p)', transition: 'width .4s ease' }} /></div>
            <span className="muted" style={{ fontSize: 12 }}>{hint}</span>
            <button type="button" className="btn" onClick={() => setOb(Math.max(0, ob - 1))} disabled={ob === 0 || busy} style={{ height: 38 }}>Back</button>
            <button type="button" className="btn btn-p" onClick={next} disabled={blocked || busy} style={{ height: 38, padding: '0 18px' }}>{busy ? 'Setting up…' : nextLabel}</button>
          </div>
        </main>
      </div>
    </div>
  );
}

async function fetchSettings(patch: Record<string, unknown>) {
  const { patch: p } = await import('../api.ts');
  const app = await p('/api/settings', patch);
  setState({ app });
}
