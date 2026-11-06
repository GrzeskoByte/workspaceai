// ⌘K palette on cmdk (R-44): grouped results, fuzzy match, ↑↓, Enter, Esc.
// It searches only the current workspace, plus the list of workspace names.
import { useState } from 'react';
import { Command } from 'cmdk';
import { useStore, setState, go, switchWorkspace, act, getState, toast, wsId, type View } from '../store.ts';
import { get, patch } from '../api.ts';
import { projectPathName } from '../../../../packages/shared/src/util.ts';
import { openArtifact, openChat } from './Shell.tsx';

const SCREENS: [View, string][] = [['home', 'Home'], ['artifacts', 'Artifacts'], ['design-systems', 'Design systems'], ['graph', 'Graph editor'], ['routines', 'Routines'], ['computers', 'Computers'], ['memory', 'Memory'], ['settings', 'Settings']];

function Item({ value, keywords, onSelect, label, meta }: { value: string; keywords?: string[]; onSelect: () => void; label?: string; meta?: string }) {
  return (
    <Command.Item value={value} keywords={keywords} onSelect={onSelect}>
      <span className="grow ellipsis">{label ?? value}</span>{meta && <span className="muted ellipsis" style={{ fontSize: 12, maxWidth: 260 }}>{meta}</span>}
    </Command.Item>
  );
}

export function Palette() {
  const open = useStore((s) => s.palette);
  const app = useStore((s) => s.app);
  const snap = useStore((s) => s.snap);
  const chatId = useStore((s) => s.chatId);
  const view = useStore((s) => s.view);
  const projId = useStore((s) => s.projId);
  const [count, setCount] = useState(0);
  if (!open || !app || !snap) return null;
  const close = () => setState({ palette: false });
  const theme = app.settings.theme;
  const proj = snap.projects.find((p) => p.id === projId) || snap.projects[0];
  const chat = view === 'chat' ? snap.chats.find((c) => c.id === chatId) : undefined;
  const run = (fn: () => void) => () => { close(); fn(); };
  const total = 8 + app.workspaces.length + snap.projects.length + snap.chats.length + snap.artifacts.length + SCREENS.length;
  return (
    <div className="scrim" onMouseDown={close}>
      <Command label="Search and jump" className="card rise" style={{ width: 640, maxWidth: 'calc(100vw - 24px)', marginTop: 'min(90px, 8vh)', overflow: 'hidden', background: 'var(--bg1)', boxShadow: '0 24px 60px rgba(0,0,0,0.25)' }}
        onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => { if (e.key === 'Escape') { e.preventDefault(); close(); } }}
        filter={(value, search, keywords) => {
          const q = search.toLowerCase().trim(); if (!q) return 1;
          const v = value.toLowerCase(); const hay = (v + ' ' + (keywords || []).join(' ')).toLowerCase();
          if (v.startsWith(q)) return 1; if (hay.includes(q)) return 0.7;
          let i = 0; for (const ch of hay) { if (ch === q[i]) i++; if (i === q.length) return 0.3; } return 0;
        }}>
        <Command.Input placeholder="Search workspaces, projects, chats, screens…" autoFocus onValueChange={() => requestAnimationFrame(() => setCount(document.querySelectorAll('[cmdk-item]').length))} />
        <Command.List>
          <Command.Empty>No results in {snap.workspace.name}.</Command.Empty>
          <Command.Group heading="Actions">
            <Item value="New chat" keywords={['action']} meta={'in ' + proj.name} onSelect={run(() => { setState({ composeProj: proj.id, draft: '' }); go('home'); })} />
            <Item value="New artifact" keywords={['action', 'doc', 'deck', 'prototype']} meta="Open Design pipeline" onSelect={run(() => go('brief'))} />
            <Item value="New workspace" keywords={['action', 'team', 'organization', 'sso']} onSelect={run(() => setState({ newWs: { type: 'team', name: '' } }))} />
            <Item value="Switch theme" keywords={['action', 'dark', 'light', 'claude', 'opencode']} meta={theme === 'claude' ? 'to opencode' : 'to Claude'} onSelect={run(() => void act(() => patch('/api/settings', { theme: theme === 'claude' ? 'opencode' : 'claude', followSystem: false }).then((a) => setState({ app: a }))))} />
            <Item value="New routine" keywords={['action', 'schedule']} meta="in words" onSelect={run(() => go('routines'))} />
            <Item value="New graph" keywords={['action', 'loop', 'bpmn']} meta="BPMN" onSelect={run(() => go('graph'))} />
            {chat && <Item value="Open this chat in opencode TUI" keywords={['action', 'terminal', 'attach']} meta={chat.sid} onSelect={run(() => void get<{ command: string }>(`/api/ws/${wsId()}/chats/${chat.id}/attach`).then((r) => navigator.clipboard.writeText(r.command).then(() => toast('Copied the opencode TUI command'), () => toast(r.command))))} />}
          </Command.Group>
          <Command.Group heading="Workspaces">
            {app.workspaces.map((w, i) => (
              <Item key={w.id} value={w.name} keywords={['workspace', w.kind]} meta={`${w.kind} · ⌘${i + 1}`} onSelect={run(() => { if (w.id !== getState().app?.settings.activeWorkspace) void switchWorkspace(w.id); })} />
            ))}
          </Command.Group>
          <Command.Group heading="Projects">
            {snap.projects.map((p) => (
              <Item key={p.id} value={p.name + ' ' + p.id} keywords={['project', projectPathName(snap.projects, p.id)]} label={p.name} meta={projectPathName(snap.projects, p.id)} onSelect={run(() => go('project', { projId: p.id, ptab: 'chats' }))} />
            ))}
          </Command.Group>
          <Command.Group heading="Chats">
            {snap.chats.map((c) => (
              <Item key={c.id} value={c.title + ' ' + c.id} keywords={['chat', c.sid]} label={c.title} meta={(snap.chatActivity[c.id] ? snap.chatActivity[c.id] + ' · ' : '') + projectPathName(snap.projects, c.project)} onSelect={run(() => openChat(c))} />
            ))}
          </Command.Group>
          <Command.Group heading="Artifacts">
            {snap.artifacts.map((a) => (
              <Item key={a.id} value={a.title + ' ' + a.id} keywords={['artifact', a.type]} label={a.title} meta={a.type + ' · ' + projectPathName(snap.projects, a.project)} onSelect={run(() => openArtifact(a))} />
            ))}
          </Command.Group>
          <Command.Group heading="Screens">
            {SCREENS.map(([v, label]) => <Item key={v} value={label} keywords={['screen', v]} onSelect={run(() => go(v))} />)}
          </Command.Group>
        </Command.List>
        <div className="mono muted row" style={{ gap: 14, padding: '8px 14px', borderTop: '1px solid var(--bg2)', fontSize: 11 }}><span>↑↓ move</span><span>↵ open</span><span>esc close</span><span style={{ marginLeft: 'auto' }}>{count || total} results · {snap.workspace.name}</span></div>
      </Command>
    </div>
  );
}
