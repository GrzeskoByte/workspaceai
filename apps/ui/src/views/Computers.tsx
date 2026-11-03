// Computers (R-28): pair with a code, rename, wake (Wake-on-LAN), unlink; capabilities, allowed apps,
// per-workspace access and folders, ask-first actions; activity and a live view you can stop or take over.
import { useState } from 'react';
import { useStore, act, refreshApp, wsId, toast, setState } from '../store.ts';
import { post, patch, del } from '../api.ts';
import type { Computer } from '../../../../packages/shared/src/types.ts';
import { Switch } from '../ui/Switch.tsx';
import { timeAgo } from '../../../../packages/shared/src/util.ts';
import { Icon } from '../ui/Icon.tsx';

const CAPS = [['files', 'Read and edit files'], ['commands', 'Run commands'], ['browser', 'Use the built-in browser'], ['computerUse', 'Computer use (screen control in allowed apps)'], ['wake', 'Wake for routines']] as const;
const ASKS = [['deleteFiles', 'Deleting files'], ['sendMessages', 'Sending messages'], ['purchases', 'Purchases'], ['installSoftware', 'Installing software'], ['systemSettings', 'System settings'], ['openApps', 'Opening new apps']] as const;
const STATUS: Record<string, string> = { online: 'var(--success)', asleep: 'var(--warning)', offline: 'var(--dim)' };

export function ComputersView() {
  const computers = useStore((s) => s.app?.computers || []);
  const [code, setCode] = useState<{ code: string; expires: number } | null>(null);
  const [open, setOpen] = useState<string | null>(computers[0]?.id || null);
  const pair = () => void act(() => post<{ code: string; expires: number }>('/api/computers/pairing-code').then(setCode));
  return (
    <div className="page-pad" style={{ flex: '1 1 auto', overflowY: 'auto', padding: '26px 30px', display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div className="row"><h1 className="disp grow" style={{ fontSize: 30 }}>Computers</h1><button type="button" className="btn btn-p" onClick={pair}>Pair a computer</button></div>
      <span className="muted" style={{ fontSize: 13 }}>Each computer is set up once; each workspace separately decides whether it may use it, and which folders.</span>
      {code && <div className="card rise" style={{ padding: 16, display: 'flex', alignItems: 'center', gap: 16, borderColor: 'var(--accent-p)' }}>
        <span className="mono disp" style={{ fontSize: 30, letterSpacing: '.12em' }}>{code.code}</span>
        <span className="grow" style={{ fontSize: 13 }}>On the other computer, open Harness → <b>Link this computer</b> and type this code. It expires {new Date(code.expires).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}. A new computer gets no workspace until you allow one.</span>
        <button type="button" className="btn" onClick={() => { setCode(null); void refreshApp(); }}>Done</button>
      </div>}
      {computers.map((c) => <ComputerCard key={c.id} c={c} open={open === c.id} onToggle={() => setOpen(open === c.id ? null : c.id)} />)}
      {!computers.length && <div className="card" style={{ padding: 28, textAlign: 'center' }}><span className="muted">No computers linked yet. Pair one to run routines on it.</span></div>}
    </div>
  );
}

function ComputerCard({ c, open, onToggle }: { c: Computer; open: boolean; onToggle: () => void }) {
  const app = useStore((s) => s.app)!;
  const ws = wsId();
  const [name, setName] = useState(c.name);
  const [mac, setMac] = useState(c.mac || '');
  const [app_, setApp] = useState('');
  const [folder, setFolder] = useState('');
  const save = (body: Partial<Computer>, note: string) => void act(() => patch(`/api/computers/${c.id}`, body).then(() => refreshApp()), note);
  const access = c.access[ws] || { allowed: false, folders: [] };
  const setAccess = (next: typeof access, note: string) => save({ access: { ...c.access, [ws]: next } }, note);
  const live = c.live && c.live.state !== 'stopped' ? c.live : null;
  const control = (op: 'stop' | 'takeover' | 'handback', note: string) => void act(() => post(`/api/computers/${c.id}/live/${op}`).then(() => refreshApp()), note);
  const wsName = (id: string) => app.workspaces.find((w) => w.id === id)?.name || 'another workspace';
  return (
    <div className="card rise" style={{ padding: 14, display: 'flex', flexDirection: 'column', gap: 10, borderColor: live ? 'var(--accent-p)' : undefined }}>
      <button type="button" className="row" aria-expanded={open} onClick={onToggle} style={{ border: 0, background: 'transparent', color: 'var(--text)', padding: 0, textAlign: 'left' }}>
        <span className="dot" style={{ background: STATUS[c.status], width: 10, height: 10 }} />
        <b className="grow">{c.name}</b>
        <span className="muted" style={{ fontSize: 12 }}>{c.os} · {c.status} · seen {timeAgo(c.lastSeen)} · {access.allowed ? 'allowed here' : 'not allowed in this workspace'}{live ? ' · working now' : ''}</span>
      </button>

      {live && <div className="card" aria-label="Live view" style={{ padding: 12, background: 'var(--bg0)', display: 'flex', flexDirection: 'column', gap: 8 }}>
        <span className="row"><span className={'dot ' + (live.state === 'running' ? 'pulse' : '')} style={{ background: live.state === 'running' ? 'var(--accent-p)' : 'var(--warning)' }} /><b className="grow">Live · {live.title}</b><span className="muted" style={{ fontSize: 12 }}>{wsName(live.workspace)} · {live.controlledBy === 'you' ? 'you are in control' : 'agent in control'}</span></span>
        <div style={{ fontSize: 14 }}><span className="lbl">Next action</span><br />{live.action}</div>
        <div className="mono" style={{ fontSize: 11.5, maxHeight: 110, overflowY: 'auto', color: 'var(--text2)' }}>{live.steps.slice(0, 8).map((s, i) => <div key={i}>{new Date(s.at).toLocaleTimeString()} · {s.text}</div>)}</div>
        <span className="row">
          <button type="button" className="btn btn-d" onClick={() => control('stop', 'Stopped')}>Stop</button>
          {live.controlledBy === 'agent' ? <button type="button" className="btn" onClick={() => control('takeover', 'You have control; the agent is paused')}>Take over</button> : <button type="button" className="btn btn-p" onClick={() => control('handback', 'Handed back to the agent')}>Hand back</button>}
          <span className="muted" style={{ fontSize: 12 }}>You can stop or take over at any moment.</span>
        </span>
      </div>}

      {open && <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: 12 }}>
        <section style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <span className="lbl">This computer</span>
          <form className="row" onSubmit={(e) => { e.preventDefault(); if (name.trim() && name !== c.name) save({ name }, 'Renamed'); }}><input className="inp grow" aria-label="Computer name" value={name} onChange={(e) => setName(e.target.value)} /><button type="submit" className="btn" disabled={!name.trim() || name === c.name}>Rename</button></form>
          <form className="row" onSubmit={(e) => { e.preventDefault(); save({ mac }, mac ? 'MAC saved' : 'MAC removed'); }}><input className="inp mono grow" aria-label="MAC address for wake" value={mac} onChange={(e) => setMac(e.target.value)} placeholder="MAC for Wake-on-LAN, 00:11:22:33:44:55" /><button type="submit" className="btn">Save</button></form>
          <span className="row">
            <button type="button" className="btn" disabled={c.status === 'online'} onClick={() => void act(() => post<Computer>(`/api/computers/${c.id}/wake`).then(async (x) => { await refreshApp(); return x; }), (x) => x.activity[0]?.text || 'Wake sent')}>Wake</button>
            <button type="button" className="btn btn-d" onClick={() => setState({ confirm: { title: `Unlink ${c.name}?`, body: 'Every workspace loses access to it; routines that run on it will be skipped.', ok: 'Unlink', run: async () => { await del(`/api/computers/${c.id}`); await refreshApp(); toast(c.name + ' unlinked'); } } })}>Unlink…</button>
          </span>
          <span className="lbl" style={{ paddingTop: 6 }}>What the agent may do</span>
          {CAPS.map(([k, label]) => <label key={k} className="row" style={{ justifyContent: 'space-between', fontSize: 13 }}>{label}<Switch on={c.capabilities[k]} label={label} onChange={() => save({ capabilities: { ...c.capabilities, [k]: !c.capabilities[k] } }, label + (c.capabilities[k] ? ' off' : ' on'))} /></label>)}
          <span className="lbl" style={{ paddingTop: 6 }}>Allowed apps for computer use</span>
          <div className="row" style={{ flexWrap: 'wrap' }}>{c.allowedApps.map((a) => <button key={a} type="button" className="chip" onClick={() => save({ allowedApps: c.allowedApps.filter((x) => x !== a) }, a + ' removed')}>{a} <Icon name="x" size={12} /></button>)}{!c.allowedApps.length && <span className="muted" style={{ fontSize: 12 }}>None — computer use can’t open any app.</span>}</div>
          <form className="row" onSubmit={(e) => { e.preventDefault(); const t = app_.trim(); if (t && !c.allowedApps.includes(t)) save({ allowedApps: [...c.allowedApps, t] }, t + ' allowed'); setApp(''); }}><input className="inp grow" aria-label="App to allow" value={app_} onChange={(e) => setApp(e.target.value)} placeholder="Blender" /><button type="submit" className="btn">Allow</button></form>
        </section>

        <section style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <span className="lbl">This workspace’s access</span>
          <label className="row" style={{ justifyContent: 'space-between', fontSize: 13 }}>May use {c.name}<Switch on={access.allowed} label="Allow this workspace" onChange={() => setAccess({ ...access, allowed: !access.allowed }, access.allowed ? 'Access removed' : 'Access granted')} /></label>
          {access.folders.map((f, i) => (
            <span key={i} className="row" style={{ fontSize: 12.5 }}><span className="mono grow ellipsis">{f.path}</span>
              <button type="button" className="chip" aria-pressed={f.mode === 'rw'} onClick={() => setAccess({ ...access, folders: access.folders.map((x, j) => (j === i ? { ...x, mode: x.mode === 'rw' ? 'ro' : 'rw' } : x)) }, 'Folder mode changed')}>{f.mode === 'rw' ? 'read-write' : 'read-only'}</button>
              <button type="button" className="btn" style={{ height: 26 }} onClick={() => setAccess({ ...access, folders: access.folders.filter((_, j) => j !== i) }, 'Folder removed')}>Remove</button></span>
          ))}
          <form className="row" onSubmit={(e) => { e.preventDefault(); const p = folder.trim(); if (p) setAccess({ ...access, folders: [...access.folders, { path: p, mode: 'ro' }] }, 'Folder added (read-only)'); setFolder(''); }}><input className="inp mono grow" aria-label="Folder" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="~/renders" disabled={!access.allowed} /><button type="submit" className="btn" disabled={!access.allowed}>Add folder</button></form>
          {Object.entries(c.access).filter(([id, a]) => id !== ws && a.allowed).map(([id]) => <span key={id} className="muted" style={{ fontSize: 12 }}>Also used by {wsName(id)}</span>)}
          <span className="lbl" style={{ paddingTop: 6 }}>Always ask first before</span>
          {ASKS.map(([k, label]) => <label key={k} className="row" style={{ justifyContent: 'space-between', fontSize: 13 }}>{label}<Switch on={c.askFirst[k]} label={'Ask before ' + label} onChange={() => save({ askFirst: { ...c.askFirst, [k]: !c.askFirst[k] } }, (c.askFirst[k] ? 'No longer asks before ' : 'Asks before ') + label.toLowerCase())} /></label>)}
        </section>

        <section style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <span className="lbl">Activity</span>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 3, maxHeight: 320, overflowY: 'auto' }}>
            {c.activity.slice(0, 30).map((a, i) => <div key={i} style={{ fontSize: 12 }}><span className="mono muted">{new Date(a.at).toLocaleString()}</span> · {a.text}</div>)}
          </div>
        </section>
      </div>}
    </div>
  );
}
