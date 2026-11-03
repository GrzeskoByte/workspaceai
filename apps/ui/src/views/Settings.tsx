// Settings: all 15 sections for the active workspace (R-32). Org-locked rows say why (R-07, R-36).
import { useEffect, useState, type ReactNode } from 'react';
import { useStore, setState, act, refreshSnap, refreshModels, wsId, toast, go, switchWorkspace, getState } from '../store.ts';
import { get, post, patch, del, download } from '../api.ts';
import type { EngineStatus, SubscriptionStatus, SubscriptionId, Project, Member, OpenDesignSettings } from '../../../../packages/shared/src/types.ts';
import { ACCENTS, odSettings } from '../../../../packages/shared/src/defaults.ts';
import { projectPathName } from '../../../../packages/shared/src/util.ts';
import { Switch } from '../ui/Switch.tsx';
import { MemoryPanel } from './MemoryPanel.tsx';
import { OpenDesignCard } from '../onboarding/OpenDesignCard.tsx';
import { Icon } from '../ui/Icon.tsx';

const PAGES: [string, string, string][] = [
  ['general', 'General', 'Name, accent and defaults for this workspace.'],
  ['members', 'Members & SSO', ''],
  ['providers', 'Model providers', 'Which models chats, loops and routines may use here.'],
  ['agents', 'Agents (ACP)', 'Coding agents chats and graph tasks can run on. opencode is the default; others connect over the Agent Client Protocol.'],
  ['connectors', 'Connectors & MCP', 'Apps and MCP servers chats can call. Credentials are kept per workspace.'],
  ['web', 'Web search', 'Which search the agent can use in this workspace. Each chat can still turn it off.'],
  ['memory', 'Memory', 'What the agent remembers in this workspace, and how.'],
  ['skills', 'Skills & plugins', 'SKILL.md folders and plugins available here.'],
  ['artifacts', 'Artifacts & sharing', 'Defaults for docs, decks, prototypes and sheets.'],
  ['opendesign', 'Open Design', 'The artifact engine. Every doc, deck, prototype and sheet an agent makes is saved, versioned and linted by Open Design.'],
  ['leanctx', 'lean-ctx', 'Compressed context for every agent: file reads, searches and command output in far fewer tokens.'],
  ['routines', 'Routines', 'Scheduled prompts and graphs.'],
  ['computers', 'Computers', 'Linked machines this workspace may use.'],
  ['data', 'Data & retention', 'Where this workspace keeps things and for how long.'],
  ['audit', 'Audit log', ''],
  ['engine', 'Engine', 'The processes behind this workspace. Every chat is an opencode session.'],
  ['appearance', 'Appearance', 'Applies to every workspace.'],
];
const ORG_LOCKED = ['providers', 'members', 'artifacts', 'data', 'web', 'agents'];

export function SettingsView() {
  const page = useStore((s) => s.setPage);
  const snap = useStore((s) => s.snap)!;
  const isOrg = snap.workspace.kind === 'org';
  const cur = PAGES.find((p) => p[0] === page) || PAGES[0];
  const lead = cur[0] === 'members' ? (isOrg ? 'Who is in this workspace and how they sign in.' : 'Personal and team workspaces you manage.') : cur[0] === 'audit' ? (isOrg ? 'Kept on this computer, append-only; your organization collects it on its schedule.' : 'What happened in this workspace.') : cur[2];
  return (
    <div className="split" style={{ flex: '1 1 auto', minWidth: 0, display: 'flex' }}>
      <nav aria-label="Settings sections" className="side-col" style={{ flex: '0 0 220px', padding: '16px 10px', borderRight: '1px solid var(--bg2)', display: 'flex', flexDirection: 'column', gap: 1, overflowY: 'auto' }}>
        {PAGES.map(([id, label]) => (
          <button key={id} type="button" className="navrow" aria-current={cur[0] === id ? 'page' : 'false'} onClick={() => setState({ setPage: id })}>{label}<span className="muted" style={{ marginLeft: 'auto', fontSize: 11 }}>{isOrg && ORG_LOCKED.includes(id) ? <Icon name="lock" size={12} /> : id === 'audit' ? snap.audit.length || '' : ''}</span></button>
        ))}
      </nav>
      <div className="page-pad" style={{ flex: '1 1 auto', minWidth: 0, overflowY: 'auto', padding: '26px 34px' }}>
        <div className="rise" key={cur[0]} style={{ maxWidth: 820, display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div><h1 className="disp" style={{ fontSize: 30 }}>{cur[1]}</h1><span className="muted">{lead}</span></div>
          {isOrg && ORG_LOCKED.includes(cur[0]) && <div className="card" style={{ padding: '10px 14px', borderColor: 'var(--accent-o)', fontSize: 13, display: 'flex', alignItems: 'center', gap: 8 }}><Icon name="lock" size={14} /> Parts of this page are set by your organization’s policy{snap.policy ? ` (v${snap.policy.version}, ${snap.policy.residency})` : ''}.</div>}
          {cur[0] === 'general' && <General />}
          {cur[0] === 'members' && <Members />}
          {cur[0] === 'providers' && <Providers />}
          {cur[0] === 'agents' && <Agents />}
          {cur[0] === 'connectors' && <Connectors />}
          {cur[0] === 'web' && <Web />}
          {cur[0] === 'memory' && <MemoryPanel />}
          {cur[0] === 'skills' && <Skills />}
          {cur[0] === 'artifacts' && <ArtifactsSharing />}
          {cur[0] === 'opendesign' && <OpenDesignPanel />}
          {cur[0] === 'leanctx' && <LeanCtxPanel />}
          {cur[0] === 'routines' && <RoutinesSettings />}
          {cur[0] === 'computers' && <ComputersSettings />}
          {cur[0] === 'data' && <DataRetention />}
          {cur[0] === 'audit' && <Audit />}
          {cur[0] === 'engine' && <EnginePanel />}
          {cur[0] === 'appearance' && <Appearance />}
        </div>
      </div>
    </div>
  );
}

/** One settings row in the design's style: label and note on the left, the control on the right. */
function Row({ label, note, children, locked }: { label: string; note?: ReactNode; children?: ReactNode; locked?: boolean }) {
  return (
    <div className="card set-row" style={{ display: 'flex', alignItems: 'center', gap: 14, padding: '12px 16px' }}>
      <span className="grow" style={{ display: 'flex', flexDirection: 'column' }}>
        <span style={{ fontSize: 13, fontWeight: 500 }}>{label}{locked && <span className="muted" style={{ fontWeight: 400 }}> · locked by org policy</span>}</span>
        {note && <span className="muted" style={{ fontSize: 12 }}>{note}</span>}
      </span>
      {children}
    </div>
  );
}

function Choice<T extends string>({ options, value, onPick, disabled, labels }: { options: readonly T[]; value: T; onPick: (v: T) => void; disabled?: boolean; labels?: Partial<Record<T, string>> }) {
  return <span className="row" style={{ gap: 6, flexWrap: 'wrap', justifyContent: 'flex-end' }}>{options.map((o) => <button key={o} type="button" className="chip" aria-pressed={o === value} disabled={disabled} onClick={() => o !== value && onPick(o)}>{labels?.[o] ?? o}</button>)}</span>;
}

function saveWs(patchBody: Record<string, unknown>, ok = 'Saved') {
  return act(() => patch(`/api/ws/${wsId()}`, patchBody).then(() => refreshSnap()), ok);
}

function TextSave({ value, onSave, disabled, label, mono, width = 260, btn = 'Save' }: { value: string; onSave: (v: string) => void; disabled?: boolean; label: string; mono?: boolean; width?: number; btn?: string }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  return (
    <form className="row" style={{ gap: 6 }} onSubmit={(e) => { e.preventDefault(); if (v.trim() && v !== value) onSave(v.trim()); }}>
      <input className={'inp' + (mono ? ' mono' : '')} aria-label={label} value={v} disabled={disabled} onChange={(e) => setV(e.target.value)} style={{ width }} />
      <button type="submit" className="btn" disabled={disabled || !v.trim() || v === value}>{btn}</button>
    </form>
  );
}

function confirm(title: string, body: string, ok: string, run: () => Promise<unknown> | void) { setState({ confirm: { title, body, ok, run: async () => { await run(); } } }); }

function General() {
  const snap = useStore((s) => s.snap)!;
  const app = useStore((s) => s.app)!;
  const models = useStore((s) => s.models);
  const ws = snap.workspace;
  const isOrg = ws.kind === 'org';
  const [instructions, setInstructions] = useState(ws.instructions);
  useEffect(() => setInstructions(ws.instructions), [ws.id]);
  const personalCount = app.workspaces.filter((w) => w.kind === 'personal').length;
  const canDelete = !(ws.kind === 'personal' && personalCount <= 1);
  const remove = () => confirm(isOrg ? `Leave ${ws.name}?` : `Delete ${ws.name}?`, 'This stops its servers and removes its chats, memory, artifacts index and secrets from this computer. Project folders on disk are kept.', isOrg ? 'Leave' : 'Delete', async () => {
    const next = app.workspaces.find((w) => w.id !== ws.id);
    if (await act(() => del(`/api/workspaces/${ws.id}`), ws.name + ' removed') && next) await switchWorkspace(next.id);
  });
  return <>
    <Row label="Workspace name" note="Shown in the rail and the header" locked={isOrg}><TextSave label="Workspace name" value={ws.name} disabled={isOrg} btn="Rename" onSave={(name) => void saveWs({ name }, 'Workspace renamed')} /></Row>
    <Row label="Accent" note="Top strip, rail and buttons"><Choice options={ACCENTS.map((a) => a.id)} labels={Object.fromEntries(ACCENTS.map((a) => [a.id, a.name]))} value={ws.accent} onPick={(accent) => void saveWs({ accent }, 'Accent · ' + ACCENTS.find((a) => a.id === accent)?.name)} /></Row>
    <Row label="Default project" note="Where quick chats go — no chat is left outside a project">
      <select className="inp" aria-label="Default project" value={ws.defaultProject} onChange={(e) => void saveWs({ defaultProject: e.target.value })}>{snap.projects.map((p) => <option key={p.id} value={p.id}>{projectPathName(snap.projects, p.id)}</option>)}</select>
    </Row>
    <Row label="Default model" note={isOrg ? 'Only models your organization allows' : 'Used when a project or chat does not pick one'}>
      <select className="inp mono" aria-label="Default model" value={ws.defaultModel} onChange={(e) => void saveWs({ defaultModel: e.target.value }, 'Model · ' + e.target.value)}>{Array.from(new Set([ws.defaultModel, ...models].filter(Boolean))).map((m) => <option key={m} value={m}>{m}</option>)}</select>
    </Row>
    <div className="card" style={{ padding: '12px 16px', display: 'flex', flexDirection: 'column', gap: 8 }}>
      <span style={{ fontSize: 13, fontWeight: 500 }}>Workspace instructions <span className="muted" style={{ fontWeight: 400, fontSize: 12 }}>· the broadest layer; every project inherits it</span></span>
      <textarea className="inp" rows={4} aria-label="Workspace instructions" value={instructions} onChange={(e) => setInstructions(e.target.value)} />
      <span className="row"><button type="button" className="btn btn-p" disabled={instructions === ws.instructions} onClick={() => void saveWs({ instructions }, 'Workspace instructions saved')}>Save</button><button type="button" className="btn" disabled={instructions === ws.instructions} onClick={() => setInstructions(ws.instructions)}>Revert</button></span>
    </div>
    <Row label={isOrg ? 'Leave organization' : 'Delete workspace'} note={canDelete ? (isOrg ? 'Your org data stays with the org' : 'Deletes its chats, memory and artifacts index') : 'Your only personal workspace can’t be deleted'}>
      <button type="button" className="btn btn-d" disabled={!canDelete} onClick={remove}>{isOrg ? 'Leave…' : 'Delete…'}</button>
    </Row>
  </>;
}

function Members() {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const [invite, setInvite] = useState('');
  if (ws.kind === 'personal') return <>
    <Row label="Members" note="A personal workspace has one member"><span className="mono" style={{ fontSize: 12 }}>Only you</span></Row>
    <Row label="Want to collaborate?" note="Create a team workspace and invite people there"><button type="button" className="btn" onClick={() => setState({ newWs: { type: 'team', name: '' } })}>New team workspace</button></Row>
    <Row label="Join an organization" note="Sign in with your organization’s SSO; its policy applies there"><button type="button" className="btn" onClick={() => setState({ newWs: { type: 'org', name: '' } })}>Join with SSO…</button></Row>
  </>;
  const isOrg = ws.kind === 'org';
  const setRole = (i: number, role: Member['role']) => void saveWs({ members: ws.members.map((m, j) => (j === i ? { ...m, role } : m)) }, 'Role updated');
  return <>
    {isOrg && <Row label="Identity provider" note={ws.org?.issuer}><span className="mono" style={{ fontSize: 12 }}>{ws.org?.idp || 'OIDC'} · {ws.org?.subject}</span></Row>}
    {isOrg && <Row label="Require SSO" note="Set by your organization" locked><Switch on label="Require SSO" disabled onChange={() => {}} /></Row>}
    {ws.members.map((m, i) => (
      <Row key={i} label={m.name} note={m.email || 'member'}><Choice options={['Admin', 'Member', 'Viewer'] as const} value={m.role} disabled={i === 0} onPick={(r) => setRole(i, r)} /></Row>
    ))}
    <Row label="Invite" note={isOrg ? 'By email; they sign in with SSO' : 'By email; they get their own copy of the workspace settings'}>
      <form className="row" style={{ gap: 6 }} onSubmit={(e) => { e.preventDefault(); const t = invite.trim(); if (!t) return; void saveWs({ members: [...ws.members, { name: t, email: t, role: 'Member' }] }, 'Invited ' + t); setInvite(''); }}>
        <input className="inp" aria-label="Invite by email" type="email" value={invite} onChange={(e) => setInvite(e.target.value)} placeholder="name@example.com" style={{ width: 240 }} /><button type="submit" className="btn">Invite</button>
      </form>
    </Row>
  </>;
}

function Providers() {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const isOrg = ws.kind === 'org' && !!snap.policy;
  const [names, setNames] = useState<string[]>([]);
  useEffect(() => { get<{ names: string[] }>(`/api/ws/${wsId()}/secrets`).then((r) => setNames(r.names)).catch(() => {}); }, [ws.providers.length]);
  const list = isOrg ? snap.policy!.allowedProviders : ws.providers;
  const test = (id: string) => void act(() => post<{ ok?: boolean; error?: string; note?: string; ms?: number }>(`/api/ws/${wsId()}/providers/test`, { id }).then((r) => { if (!r.ok) throw new Error(r.error || 'Failed'); return r; }), (r) => r.note || `Connected${r.ms ? ' · ' + r.ms + ' ms' : ''}`);
  return <>
    {!isOrg && <>
      <span className="lbl">Subscriptions</span>
      <p className="muted" style={{ margin: 0, fontSize: 13 }}>Chat on the plan you already pay for. Chats run inside that tool (Claude Code over ACP), so its models appear in the chat picker once connected.</p>
      <SubscriptionRows only={['claude', 'grok']} asProvider />
      <span className="lbl" style={{ paddingTop: 8 }}>API providers (opencode)</span>
    </>}
    {isOrg && snap.policy!.gateway && <Row label="Gateway" note="LiteLLM proxy, open-source (MIT) edition, run by your organization"><span className="mono" style={{ fontSize: 12 }}>{snap.policy!.gateway}</span></Row>}
    {list.map((p) => (
      <Row key={p.id} label={p.name + (p.isDefault ? ' · default' : '')} locked={!!p.locked} note={<>{p.note} · <span className="mono">{p.models.map((m) => m.id).join(', ')}</span>{p.keyEnv ? <> · key <span className="mono">{p.keyEnv}</span>{names.includes(p.keyEnv) ? ' in keychain' : p.fromOpencode ? ' from opencode' : ' not set'}</> : ''}</>}>
        <span className="row" style={{ gap: 6 }}>
          <button type="button" className="btn" onClick={() => test(p.id)}>Test</button>
          {!p.locked && !p.isDefault && <button type="button" className="btn" onClick={() => void act(() => patch(`/api/ws/${wsId()}/providers/${p.id}`, { isDefault: true }).then(() => refreshSnap()), p.name + ' is now the default')}>Make default</button>}
          {!p.locked && <button type="button" className="btn btn-d" onClick={() => confirm(`Remove ${p.name}?`, 'Chats using its models switch to another provider. Its key is removed from the keychain.', 'Remove', () => act(() => del(`/api/ws/${wsId()}/providers/${p.id}`).then(() => refreshSnap()), p.name + ' removed'))}>Remove</button>}
        </span>
      </Row>
    ))}
    {!list.length && <p className="muted">No providers in this workspace yet.</p>}
    <Row label="Custom provider" note={isOrg ? 'Only your org admin can add providers here' : 'Any OpenAI-compatible or AI SDK endpoint, written to opencode.json'} locked={isOrg}>
      <button type="button" className="btn btn-p" disabled={isOrg} onClick={() => setState({ customProvider: true })}>Add custom provider…</button>
    </Row>
  </>;
}

function Agents() {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const [cmd, setCmd] = useState('');
  const orgLocked = ws.kind === 'org';
  const setAgentOn = (agentId: string, on: boolean) => void saveWs({ agents: ws.agents.map((x) => (x.id === agentId ? { ...x, on } : x)), ...(!on && ws.defaultAgent === agentId ? { defaultAgent: 'opencode' } : {}) }, on ? 'Agent on' : 'Agent off');
  const add = () => { const t = cmd.trim(); if (!t) return; const id = t.split(/\s+/)[0].replace(/[^a-z0-9-]/gi, '').toLowerCase() || 'agent'; void saveWs({ agents: [...ws.agents, { id: id + '-' + Date.now().toString(36).slice(-4), name: t.split(/\s+/)[0], via: 'ACP', command: t, on: true }] }, 'Added ' + t.split(/\s+/)[0]); setCmd(''); };
  return <>
    {ws.agents.map((a) => (
      <Row key={a.id} label={a.name + (a.id === ws.defaultAgent ? ' · default' : '')} note={a.builtin ? 'opencode serve (HTTP) · also speaks ACP via opencode acp' : `${a.via}${a.command ? ' · ' + a.command : ''}`} locked={orgLocked && !a.builtin}>
        <Switch on={a.on} disabled={orgLocked || !!a.builtin} label={a.name} onChange={() => setAgentOn(a.id, !a.on)} />
      </Row>
    ))}
    <Row label="Default agent" note="Used for new chats in this workspace" locked={orgLocked}>
      <select className="inp" aria-label="Default agent" value={ws.defaultAgent} disabled={orgLocked} onChange={(e) => void saveWs({ defaultAgent: e.target.value })}>{ws.agents.filter((a) => a.on).map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}</select>
    </Row>
    <Row label="Add ACP agent" note="Command that starts it over stdio (JSON-RPC)" locked={orgLocked}>
      <form className="row" style={{ gap: 6 }} onSubmit={(e) => { e.preventDefault(); add(); }}><input className="inp mono" aria-label="ACP agent command" value={cmd} disabled={orgLocked} onChange={(e) => setCmd(e.target.value)} placeholder="gemini --experimental-acp" style={{ width: 260 }} /><button type="submit" className="btn" disabled={orgLocked}>Add</button></form>
    </Row>
    <Row label="Harness provides" note="ACP client capabilities"><span className="mono" style={{ fontSize: 12 }}>file read/write, limited to the chat’s project folder</span></Row>
    <Row label="Ask before file writes and commands" note="Answers session/request_permission in the chat"><Switch on={ws.acpAskFirst} label="Ask first for ACP" onChange={() => void saveWs({ acpAskFirst: !ws.acpAskFirst })} /></Row>
    <span className="lbl" style={{ paddingTop: 8 }}>Subscriptions on this computer</span>
    <p className="muted" style={{ margin: 0, fontSize: 13 }}>Use the subscriptions you already pay for. Connecting copies the login on this computer into <span className="mono">data/agents/&lt;id&gt;</span> of this workspace, so each workspace keeps its own history and config.</p>
    <SubscriptionRows />
  </>;
}

/**
 * Subscriptions found on this computer, each connectable to this workspace. Shown on Settings › Agents and,
 * for the ones that bring their own models (Claude Code, Grok), on Settings › Model providers.
 */
function SubscriptionRows({ only, asProvider }: { only?: SubscriptionId[]; asProvider?: boolean }) {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const agentModels = useStore((s) => s.agentModels);
  const [rows, setRows] = useState<SubscriptionStatus[] | null>(null);
  const [busy, setBusy] = useState('');
  const orgLocked = ws.kind === 'org';
  const load = (force = false) => get<SubscriptionStatus[]>(`/api/ws/${wsId()}/subscriptions${force ? '?force=1' : ''}`).then(setRows).catch(() => setRows([]));
  useEffect(() => { void load(); }, [ws.id, ws.agents.length]);
  const connect = (id: string) => void act(async () => { setBusy(id); try { await post(`/api/ws/${wsId()}/subscriptions/${id}/connect`); await refreshSnap(); await load(); void refreshModels(); } finally { setBusy(''); } }, 'Connected · this workspace has its own copy of the login and data');
  const disconnect = (id: string) => void act(async () => { setBusy(id); try { await post(`/api/ws/${wsId()}/subscriptions/${id}/disconnect`); await refreshSnap(); await load(); void refreshModels(); } finally { setBusy(''); } }, 'Disconnected · this workspace’s agent data was removed');
  const shown = (rows || []).filter((s) => !only || only.includes(s.id));
  return <>
    {rows === null && <p className="muted pulse">Looking for {only ? only.map((id) => ({ opencode: 'opencode', claude: 'Claude Code', grok: 'Grok' })[id]).join(' and ') : 'opencode, Claude Code and Grok'}…</p>}
    {shown.map((s) => {
      const agent = ws.agents.find((a) => a.subscription === s.id && a.id === s.id);
      const offered = agent ? agentModels[agent.id]?.models || [] : [];
      return (
        <Row key={s.id} label={`${s.name} ${asProvider ? 'subscription' : s.version ? '· ' + s.version : ''}${asProvider && s.connected && agent?.on && ws.defaultAgent === agent.id ? ' · default' : ''}`} locked={orgLocked}
          note={<>{s.detail}{s.account ? ' · ' + s.account : ''}{asProvider && s.connected && offered.length ? <> · <span className="mono">{offered.map((m) => m.name).join(', ')}</span></> : ''}{!asProvider && s.connected && s.dataDir ? <> · <span className="mono">{s.dataDir}</span></> : ''}{s.installed && !s.signedIn ? <> · sign in once: <span className="mono">{({ opencode: 'opencode auth login', claude: 'claude auth login', grok: 'grok login' } as Record<string, string>)[s.id]}</span></> : ''}{!s.installed && s.id === 'claude' ? <> · install: <span className="mono">npm i -g @anthropic-ai/claude-code</span></> : ''}</>}>
          <span className="row" style={{ gap: 6 }}>
            {asProvider && s.connected && agent && ws.defaultAgent !== agent.id && <button type="button" className="btn" onClick={() => void saveWs({ defaultAgent: agent.id }, s.name + ' is now used for new chats')}>Use for new chats</button>}
            {s.connected ? <button type="button" className="btn" disabled={busy === s.id} onClick={() => disconnect(s.id)}>{busy === s.id ? 'Removing…' : 'Disconnect'}</button>
              : <button type="button" className="btn btn-p" disabled={orgLocked || !s.installed || !s.signedIn || busy === s.id} onClick={() => connect(s.id)}>{busy === s.id ? 'Connecting…' : 'Connect'}</button>}
          </span>
        </Row>
      );
    })}
    <button type="button" className="link" style={{ alignSelf: 'flex-start' }} onClick={() => { setRows(null); void load(true); }}>Check again</button>
  </>;
}

function Connectors() {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const [cmd, setCmd] = useState('');
  return <>
    {ws.mcp.map((m) => (
      <Row key={m.id} label={m.name} note={<>{m.kind}{m.locked ? ' · required by org' : ''} · <span className="mono">{m.type === 'remote' ? m.url : (m.command || []).join(' ')}</span></>}>
        <span className="row" style={{ gap: 6 }}>
          <Switch on={m.on} disabled={!!m.locked} label={m.name} onChange={() => void act(() => patch(`/api/ws/${wsId()}/mcp/${m.id}`, { on: !m.on }).then(() => refreshSnap()), m.name + (m.on ? ' off' : ' on'))} />
          {!m.locked && <button type="button" className="btn" onClick={() => confirm(`Remove ${m.name}?`, 'It is removed from this workspace’s opencode.json.', 'Remove', () => act(() => del(`/api/ws/${wsId()}/mcp/${m.id}`).then(() => refreshSnap()), 'Removed'))}>Remove</button>}
        </span>
      </Row>
    ))}
    <Row label="Add MCP server" note="Command or URL, e.g. uvx blender-mcp · https://…/mcp">
      <form className="row" style={{ gap: 6 }} onSubmit={(e) => { e.preventDefault(); const t = cmd.trim(); if (!t) return; void act(() => post(`/api/ws/${wsId()}/mcp`, { command: t }).then(() => refreshSnap()), 'Added ' + t + ' to opencode.json'); setCmd(''); }}>
        <input className="inp mono" aria-label="MCP command or URL" value={cmd} onChange={(e) => setCmd(e.target.value)} placeholder="npx -y some-mcp" style={{ width: 260 }} /><button type="submit" className="btn">Add</button>
      </form>
    </Row>
    <Row label="Harness memory" note="Served to opencode as the harness-memory MCP server"><span className="mono" style={{ fontSize: 12 }}>memory_search · memory_read · memory_write</span></Row>
    <Row label="Artifacts" note={<>Harness artifact tools; every version is saved by {odSettings(ws).engine === 'open-design' ? 'Open Design' : 'the built-in store'} · <button type="button" className="link" onClick={() => setState({ setPage: 'opendesign' })}>Open Design settings</button></>}><span className="mono" style={{ fontSize: 12 }}>artifact_create · artifact_update</span></Row>
  </>;
}

function Web() {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const web = ws.web;
  const locked = ws.kind === 'org' && !!snap.policy;
  const [preview, setPreview] = useState('');
  useEffect(() => { get<{ text: string }>(`/api/ws/${wsId()}/web-preview`).then((r) => setPreview(r.text)).catch(() => {}); }, [JSON.stringify(web)]);
  const set = (w: Record<string, unknown>, ok?: string) => void saveWs({ web: { ...web, ...w } }, ok);
  const tryIt = () => void act(() => post<{ ok: boolean; note?: string; error?: string }>('/api/onboarding/test-web', { mode: web.mode, provider: web.provider, url: web.searxngUrl, key: 'from-keychain-xxxxxxxx' }).then((r) => { if (!r.ok) throw new Error(r.error || 'Failed'); return r; }), (r) => r.note || 'OK');
  return <>
    <Row label="Search" note="opencode websearch, an MCP search server, or none" locked={locked}>
      <Choice options={['off', 'exa', 'mcp', 'searxng'] as const} labels={{ off: 'Off', exa: 'Built-in (Exa)', mcp: 'Search provider', searxng: 'SearXNG' }} value={web.mode} disabled={locked} onPick={(mode) => set({ mode }, 'Web search · ' + mode + ' · opencode restarted')} />
    </Row>
    {web.mode === 'mcp' && <Row label="Provider" note="Runs as a local MCP server; key kept in the keychain"><Choice options={['Tavily', 'Brave', 'Firecrawl'] as const} value={web.provider} disabled={locked} onPick={(provider) => set({ provider })} /></Row>}
    {web.mode === 'searxng' && <Row label="SearXNG URL" note="Your instance" locked={locked}><TextSave label="SearXNG URL" mono value={web.searxngUrl} disabled={locked} onSave={(searxngUrl) => set({ searxngUrl })} /></Row>}
    <Row label="Read pages by URL" note="opencode webfetch" locked={locked}><Switch on={web.fetch} disabled={locked} label="Read pages by URL" onChange={() => set({ fetch: !web.fetch })} /></Row>
    <Row label="Ask before each search" note="permission: ask"><Switch on={web.ask} label="Ask before each search" onChange={() => set({ ask: !web.ask })} /></Row>
    <Row label="Show sources under answers" note="Links to every page the agent read"><Switch on={web.showSources} label="Show sources" onChange={() => set({ showSources: !web.showSources })} /></Row>
    <Row label="Try a search" note={web.mode === 'off' ? 'Search is off' : 'Checks the configured backend'}><button type="button" className="btn" disabled={web.mode === 'off'} onClick={tryIt}>Check</button></Row>
    <div className="card" style={{ padding: '12px 16px', display: 'flex', flexDirection: 'column', gap: 6 }}><span className="lbl">Written to this workspace’s opencode</span><pre className="pre">{preview}</pre></div>
  </>;
}

function Skills() {
  const ws = useStore((s) => s.snap)!.workspace;
  const [folder, setFolder] = useState('');
  return <>
    {ws.skills.map((s) => <Row key={s.id} label={s.name} note={s.note}><Switch on={s.on} label={s.name} onChange={() => void saveWs({ skills: ws.skills.map((x) => (x.id === s.id ? { ...x, on: !x.on } : x)) }, s.name + (s.on ? ' off' : ' on'))} /></Row>)}
    <Row label="Add a skill folder" note="A folder with a SKILL.md; opencode and Open Design both read it">
      <form className="row" style={{ gap: 6 }} onSubmit={(e) => { e.preventDefault(); const f = folder.trim(); if (!f) return; void saveWs({ skills: [...ws.skills, { id: 'sk-' + Date.now().toString(36), name: f.split('/').filter(Boolean).pop() || f, note: f, on: true }] }, 'Added ' + f); setFolder(''); }}>
        <input className="inp mono" aria-label="Skill folder" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="~/skills/my-skill" style={{ width: 240 }} /><button type="submit" className="btn">Add</button>
      </form>
    </Row>
  </>;
}

function ArtifactsSharing() {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const isOrg = ws.kind === 'org';
  const blocked = isOrg && !!snap.policy?.blockExternalSharing;
  return <>
    <Row label="Default visibility" note="Who can see new artifacts"><Choice options={['Private', 'Workspace'] as const} value={ws.visibility} onPick={(visibility) => void saveWs({ visibility })} /></Row>
    <Row label="Allow public links" note={blocked ? 'Blocked by org policy' : 'Anyone with the link can view a static export'} locked={blocked}><Switch on={ws.publicLinks && !blocked} disabled={blocked} label="Allow public links" onChange={() => void saveWs({ publicLinks: !ws.publicLinks })} /></Row>
    <Row label="Artifact engine" note={odSettings(ws).engine === 'open-design' ? 'Open Design saves, versions and lints every artifact' : 'Built-in store — no lint, no PDF/PPTX export through Open Design'}>
      <span className="row" style={{ gap: 8 }}><span className="mono" style={{ fontSize: 12 }}>{odSettings(ws).engine === 'open-design' ? 'Open Design' : 'Built-in'}</span><button type="button" className="btn" onClick={() => setState({ setPage: 'opendesign' })}>Manage</button></span>
    </Row>
    <Row label="Design system" note={<>Used for new artifacts · {ws.designSystem} · <button type="button" className="link" onClick={() => setState({ setPage: 'opendesign' })}>choose in Open Design</button></>} />
  </>;
}

interface OdState {
  install: { installed: boolean; path?: string; version?: string; installing: boolean; progress: number; log: string; error?: string };
  daemon: { state: 'not-installed' | 'stopped' | 'starting' | 'running' | 'error'; url?: string; pid?: number; startedAt?: number; error?: string; health?: unknown };
  settings: OpenDesignSettings; dataDir: string; log: string; projects: number; agentTools: boolean;
  artifacts: { total: number; inOpenDesign: number; lintFindings: number };
}
interface DesignSystem { id: string; title: string; summary: string; swatches: string[]; source: string }

const OD_STATE: Record<OdState['daemon']['state'], [string, string]> = {
  'not-installed': ['not installed', 'var(--dim)'], stopped: ['stopped', 'var(--dim)'], starting: ['starting…', 'var(--warning)'], running: ['running', 'var(--success)'], error: ['error', 'var(--error)'],
};

/** Settings › Open Design: the artifact engine's install, this workspace's daemon, and its options (R-04). */
function OpenDesignPanel() {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const [st, setSt] = useState<OdState | null>(null);
  const [systems, setSystems] = useState<DesignSystem[]>([]);
  const [log, setLog] = useState('');
  const [onboard, setOnboard] = useState<'ok' | 'later' | null>(null);
  const [dsQuery, setDsQuery] = useState('');
  const base = `/api/ws/${wsId()}/opendesign`;
  const load = () => get<OdState>(base).then(setSt).catch((e) => toast(e.message, 'error'));
  useEffect(() => { void load(); }, []);
  // Keep polling while something is changing (install, update, daemon start).
  useEffect(() => {
    if (!st || !(st.install.installing || st.daemon.state === 'starting')) return;
    const t = window.setTimeout(load, 1200);
    return () => clearTimeout(t);
  }, [st]);
  useEffect(() => { if (st?.daemon.state === 'running') get<{ systems: DesignSystem[] }>(`/api/ws/${wsId()}/design-systems`).then((r) => setSystems(r.systems)).catch(() => {}); }, [st?.daemon.state]);
  if (!st) return <span className="muted pulse">Checking Open Design…</span>;
  const o = st.settings;
  const set = (p: Partial<OpenDesignSettings>, ok: string) => void act(() => patch(`/api/ws/${wsId()}`, { openDesign: { ...o, ...p } }).then(async () => { await refreshSnap(); await load(); }), ok);
  const daemon = (action: 'start' | 'restart' | 'stop', ok: string) => { setSt({ ...st, daemon: { state: action === 'stop' ? 'stopped' : 'starting' } }); void act(() => post(`${base}/${action}`).then(load), ok).then(() => load()); };
  const [label, color] = OD_STATE[st.daemon.state];
  const up = st.daemon.startedAt ? Math.max(1, Math.round((Date.now() - st.daemon.startedAt) / 60000)) : 0;
  const missing = st.artifacts.total - st.artifacts.inOpenDesign;
  return <>
    {o.engine === 'open-design' && !st.install.installed && <div className="card" style={{ padding: '10px 14px', borderColor: 'var(--warning)', fontSize: 13 }}>Open Design is this workspace’s artifact engine but isn’t installed yet, so agents can’t save artifacts. Install it below, or switch to the built-in store.</div>}
    <Row label="Status" note={st.daemon.error || (st.daemon.state === 'running' ? `${st.daemon.url} · pid ${st.daemon.pid} · up ${up} min` : st.daemon.state === 'stopped' ? (o.autoStart ? 'starts with the workspace engine or the first artifact' : 'starts with the first artifact') : undefined)}>
      <span className="row" style={{ gap: 6 }}><span className={'dot ' + (st.daemon.state === 'starting' ? 'pulse' : '')} style={{ background: color }} /><span className="mono" style={{ fontSize: 12 }}>{label}</span></span>
    </Row>
    {st.install.installed ? (
      <Row label="Installed" note={<span className="mono">{st.install.path?.replace(/\/apps\/daemon\/dist\/cli\.js$/, '')}</span>}>
        <span className="row" style={{ gap: 6 }}><span className="mono" style={{ fontSize: 12 }}>v{st.install.version}</span>
          <button type="button" className="btn" disabled={st.install.installing} onClick={() => void act(() => post('/api/opendesign/update').then(load), 'Updating Open Design — restart the daemon when it finishes')}>{st.install.installing ? `Updating… ${st.install.progress}%` : 'Update'}</button></span>
      </Row>
    ) : <OpenDesignCard ready={onboard} onReady={(r) => { setOnboard(r); void load(); }} />}
    {st.install.installed && st.install.installing && st.install.log && <pre className="pre" style={{ maxHeight: 120 }}>{st.install.log}</pre>}
    {st.install.error && <span style={{ fontSize: 12, color: 'var(--error-text)' }}>{st.install.error}</span>}
    <Row label="Daemon" note="One per workspace, on 127.0.0.1 only">
      <span className="row" style={{ gap: 6, flexWrap: 'wrap' }}>
        <button type="button" className="btn" disabled={!st.install.installed || st.daemon.state === 'running' || st.daemon.state === 'starting'} onClick={() => daemon('start', 'Open Design started')}>Start</button>
        <button type="button" className="btn" disabled={!st.install.installed || st.daemon.state === 'starting'} onClick={() => daemon('restart', 'Open Design restarted')}>Restart</button>
        <button type="button" className="btn" disabled={st.daemon.state !== 'running'} onClick={() => daemon('stop', 'Open Design stopped')}>Stop</button>
        <button type="button" className="btn" onClick={() => void get<{ text: string }>(`${base}/logs`).then((r) => setLog(r.text.trim() || '(empty)')).catch((e) => toast(e.message, 'error'))}>Logs</button>
      </span>
    </Row>
    {log && <pre className="pre" style={{ maxHeight: 260 }}>{log}</pre>}
    <Row label="Artifact engine" note={o.engine === 'open-design' ? 'Every artifact is saved, versioned and linted by Open Design; a save fails if it can’t be' : 'Harness store only — no lint, no PDF or PowerPoint export through Open Design'}>
      <Choice options={['open-design', 'builtin'] as const} labels={{ 'open-design': 'Open Design', builtin: 'Built-in store' }} value={o.engine} onPick={(engine) => set({ engine }, engine === 'open-design' ? 'Artifacts now go through Open Design' : 'Artifacts now use the built-in store')} />
    </Row>
    <Row label="Start with the workspace" note="Otherwise it starts on the first artifact save"><Switch on={o.autoStart} label="Start with the workspace" onChange={() => set({ autoStart: !o.autoStart }, o.autoStart ? 'Starts on the first artifact' : 'Starts with the workspace')} /></Row>
    <Row label="Lint every version" note="Open Design’s artifact checks; findings show on the artifact"><Switch on={o.lint} disabled={o.engine === 'builtin'} label="Lint every version" onChange={() => set({ lint: !o.lint }, 'Lint ' + (o.lint ? 'off' : 'on'))} /></Row>
    <Row label="Open Design tools for agents" note={<>Also give agents Open Design’s own MCP tools (projects, files, skills, runs). Artifacts still go through <span className="mono">artifact_create</span>.{o.agentTools && !st.agentTools ? ' Applies when the engine restarts.' : ''}</>}>
      <Switch on={o.agentTools} label="Open Design tools for agents" onChange={() => set({ agentTools: !o.agentTools }, o.agentTools ? 'Open Design tools removed — engine restarted' : 'Open Design tools added — engine restarted')} />
    </Row>
    <Row label="Artifacts" note={`${st.artifacts.inOpenDesign} of ${st.artifacts.total} in Open Design · ${st.artifacts.lintFindings} lint finding${st.artifacts.lintFindings === 1 ? '' : 's'} on latest versions · ${st.projects} Open Design project${st.projects === 1 ? '' : 's'}`}>
      <button type="button" className="btn" disabled={!missing || !st.install.installed} onClick={() => void act(() => post<{ synced: number; failed: { title: string; error: string }[] }>(`${base}/resync`).then(async (r) => { await load(); await refreshSnap(); return r; }), (r) => `Resynced ${r.synced}` + (r.failed.length ? ` · ${r.failed.length} failed: ${r.failed[0].error}` : ''))}>{missing ? `Resync ${missing}` : 'All synced'}</button>
    </Row>
    <div className="card" style={{ padding: '12px 16px', display: 'flex', flexDirection: 'column', gap: 8 }}>
      <span className="row" style={{ flexWrap: 'wrap' }}><span className="lbl grow">Design system for new artifacts</span><span className="muted" style={{ fontSize: 12 }}>{st.daemon.state === 'running' ? systems.length + ' available' : 'start Open Design to see its design systems'}</span></span>
      {systems.length > 8 && <input className="inp" aria-label="Filter design systems" placeholder={`Filter ${systems.length} design systems…`} value={dsQuery} onChange={(e) => setDsQuery(e.target.value)} />}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(min(220px, 100%), 1fr))', gap: 8 }}>
        {(systems.length ? systems.filter((d) => d.title === ws.designSystem || (d.title + ' ' + d.summary).toLowerCase().includes(dsQuery.toLowerCase().trim())).slice(0, dsQuery ? 60 : 12) : [{ id: 'default', title: 'Claude Warm Editorial', summary: 'The harness default.', swatches: ['#faf9f5', '#efe9de', '#141413', '#cc785c'], source: 'harness' }]).map((d) => (
          <button key={d.id} type="button" className="tile" aria-pressed={ws.designSystem === d.title} onClick={() => void saveWs({ designSystem: d.title }, 'Design system · ' + d.title)} style={{ gap: 6, ...(ws.designSystem === d.title ? { borderColor: 'var(--accent-p)' } : {}) }}>
            <span className="row" style={{ gap: 3 }}>{d.swatches.slice(0, 5).map((c) => <span key={c} style={{ width: 14, height: 14, borderRadius: 4, background: c, border: '1px solid var(--line)' }} />)}<span className="muted mono" style={{ marginLeft: 'auto', fontSize: 10 }}>{d.source}</span></span>
            <b style={{ fontSize: 13 }}>{d.title}</b><span className="muted" style={{ fontSize: 12 }}>{d.summary}</span>
          </button>
        ))}
      </div>
      {!dsQuery && systems.length > 12 && <span className="muted" style={{ fontSize: 12 }}>Showing 12 of {systems.length} — type above to find the rest.</span>}
    </div>
    <Row label="Data folder" note="Open Design’s own data for this workspace"><span className="mono ellipsis" style={{ fontSize: 11.5, maxWidth: 420 }} title={st.dataDir}>{st.dataDir}</span></Row>
    <Row label="Artifact files" note="Each project’s artifacts live in its folder"><span className="mono" style={{ fontSize: 11.5 }}>&lt;project&gt;/.harness/artifacts</span></Row>
    {st.daemon.health != null && <details className="card" style={{ padding: '10px 14px' }}><summary className="muted" style={{ cursor: 'pointer', fontSize: 12 }}>Health check</summary><pre className="pre" style={{ marginTop: 8, maxHeight: 200 }}>{JSON.stringify(st.daemon.health, null, 2)}</pre></details>}
  </>;
}

interface LeanCtxState { installed: boolean; version: string; bin: string | null; dir: string }
interface LeanCtxGain { tokensSaved: number; netTokensSaved: number; inputTokens: number; outputTokens: number; reductionPct: number; commands: number; turns: number; avoidedUsd: number; overheadPerTurn: number; evidence: string; model: string; byTool: { name: string; calls: number; saved: number; pct: number }[] }
interface LeanCtxOption { key: string; section: string; type: string; description: string; default: unknown; values?: string[]; value: unknown; set: boolean }

/** The options most people change, in the order they matter; every other key is under "All options". */
const LC_MAIN = ['compression_level', 'output_density', 'terse_agent', 'read_redirect', 'shell_approval', 'memory_profile'];
const LC_LABEL: Record<string, string> = { compression_level: 'Compression', output_density: 'Tool output density', terse_agent: 'Terse agent replies', read_redirect: 'Redirect native reads', shell_approval: 'Unlisted shell commands', memory_profile: 'Memory profile' };
const fmtTokens = (n: number) => (n >= 1e6 ? (n / 1e6).toFixed(n >= 1e7 ? 0 : 1) + 'M' : n >= 1e3 ? (n / 1e3).toFixed(n >= 1e4 ? 0 : 1) + 'k' : String(Math.round(n)));

function LeanCtxPanel() {
  const ws = useStore((s) => s.snap)!.workspace;
  const [st, setSt] = useState<LeanCtxState | null>(null);
  const [gain, setGain] = useState<LeanCtxGain | null>(null);
  const [gainErr, setGainErr] = useState('');
  const [period, setPeriod] = useState<'week' | 'month' | 'all'>('all');
  const [opts, setOpts] = useState<{ path: string; options: LeanCtxOption[]; toolProfile: string } | null>(null);
  const [q, setQ] = useState('');
  const [busy, setBusy] = useState('');
  const [out, setOut] = useState('');
  const load = () => get<LeanCtxState>('/api/leanctx').then(setSt).catch((e) => toast(e.message, 'error'));
  const loadGain = (p = period) => get<LeanCtxGain>(`/api/leanctx/gain?period=${p}`).then((g) => { setGain(g); setGainErr(''); }).catch((e) => setGainErr(e.message));
  const loadOpts = () => get<{ path: string; options: LeanCtxOption[]; toolProfile: string }>('/api/leanctx/options').then(setOpts).catch(() => {});
  useEffect(() => { void load(); }, []);
  useEffect(() => { if (st?.installed) { void loadGain(); void loadOpts(); } }, [st?.installed, st?.version]);
  if (!st) return <span className="muted pulse">Checking lean-ctx…</span>;
  const on = ws.leanCtx?.enabled !== false;
  const install = async () => {
    setBusy(st.installed ? 'Updating…' : 'Installing…'); setOut('');
    try {
      const r = await post<{ ok: boolean; output: string; version?: string }>('/api/leanctx/install');
      setOut(r.output);
      if (!r.ok) { toast('lean-ctx did not install — see the output', 'error'); return; }
      if (!on) await patch(`/api/ws/${wsId()}`, { leanCtx: { enabled: true } });
      await refreshSnap(); await load();
      toast(`lean-ctx ${r.version || ''} ${st.installed ? 'updated' : 'installed and connected to this workspace’s agents'}`);
    } catch (e) { toast((e as Error).message, 'error'); } finally { setBusy(''); }
  };
  const setOpt = (key: string, value: unknown) => void act(() => post<{ note: string }>('/api/leanctx/options', { key, value }).then(async (r) => { await loadOpts(); return r; }), (r) => r.note || 'Saved');
  const main = opts ? LC_MAIN.map((k) => opts.options.find((o) => o.key === k)).filter(Boolean) as LeanCtxOption[] : [];
  const needle = q.trim().toLowerCase();
  const rest = opts && needle ? opts.options.filter((o) => (o.key + ' ' + o.description).toLowerCase().includes(needle)).slice(0, 80) : [];
  return <>
    <Row label="Status" note={st.installed ? <span className="mono">{st.bin}</span> : 'Not installed. Installs the official release (verified checksum) into the harness’s own folder; your other AI tools are left alone.'}>
      <span className="row" style={{ gap: 8 }}>
        {st.installed && <span className="mono" style={{ fontSize: 12 }}>v{st.version}</span>}
        <button type="button" className={'btn' + (st.installed ? '' : ' btn-p')} disabled={!!busy} onClick={() => void install()}>{busy || (st.installed ? 'Update' : 'Install lean-ctx')}</button>
      </span>
    </Row>
    {out && <pre className="pre" style={{ maxHeight: 160 }}>{out}</pre>}
    {st.installed && <>
      <Row label="Use in this workspace" note={on ? 'Every agent here gets lean-ctx’s tools and is told to prefer them for reads, searches and long command output' : 'Agents here use their own tools only'}>
        <Switch on={on} label="Use lean-ctx in this workspace" onChange={() => void saveWs({ leanCtx: { enabled: !on } }, on ? 'lean-ctx off — engine restarted' : 'lean-ctx on — engine restarted')} />
      </Row>

      <div className="card" style={{ padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 12 }}>
        <span className="row" style={{ flexWrap: 'wrap' }}>
          <span className="lbl grow">Tokens saved</span>
          <Choice options={['week', 'month', 'all'] as const} labels={{ week: 'Week', month: 'Month', all: 'All time' }} value={period} onPick={(p) => { setPeriod(p); void loadGain(p); }} />
          <button type="button" className="btn" onClick={() => void loadGain()} aria-label="Refresh savings"><Icon name="reload" size={14} /></button>
        </span>
        {gainErr ? <span style={{ fontSize: 12, color: 'var(--error-text)' }}>{gainErr}</span> : !gain ? <span className="muted pulse" style={{ fontSize: 12 }}>Reading lean-ctx gain…</span> : <>
          <div className="lc-stats">
            <div><b>{fmtTokens(gain.tokensSaved)}</b><span>tokens saved</span></div>
            <div><b>{gain.reductionPct.toFixed(gain.reductionPct < 10 ? 1 : 0)}%</b><span>less context</span></div>
            <div><b>{fmtTokens(gain.netTokensSaved)}</b><span>net, after lean-ctx’s own overhead</span></div>
            <div><b>${gain.avoidedUsd.toFixed(gain.avoidedUsd < 10 ? 2 : 0)}</b><span>avoided{gain.model ? ` · ${gain.model}` : ''}</span></div>
          </div>
          <span className="muted" style={{ fontSize: 12 }}>{gain.commands} compressed call{gain.commands === 1 ? '' : 's'} · {gain.turns} turn{gain.turns === 1 ? '' : 's'} · {fmtTokens(gain.inputTokens)} tokens in, {fmtTokens(gain.outputTokens)} out · overhead {fmtTokens(gain.overheadPerTurn)}/turn · {gain.evidence === 'local_estimate' ? 'lean-ctx’s local estimate' : gain.evidence.replace(/_/g, ' ')}</span>
          {gain.byTool.length > 0 && <div style={{ display: 'grid', gridTemplateColumns: '1fr auto auto', gap: '4px 16px', fontSize: 12.5 }}>
            <span className="lbl">Tool</span><span className="lbl">Calls</span><span className="lbl">Saved</span>
            {gain.byTool.map((t) => <div key={t.name} style={{ display: 'contents' }}><span className="mono">{t.name}</span><span>{t.calls}</span><span>{fmtTokens(t.saved)}</span></div>)}
          </div>}
          {!gain.commands && <span className="muted" style={{ fontSize: 12 }}>Nothing yet — savings appear once an agent reads or runs something through lean-ctx.</span>}
        </>}
      </div>

      {opts && <>
        <Row label="Tools agents see" note="lean: 12 core tools (all 49 stay callable) · minimal · standard · power: every tool advertised">
          <Choice options={['lean', 'minimal', 'standard', 'power'] as const} value={(opts.toolProfile as 'lean') || 'lean'} onPick={(p) => void act(() => post<{ note: string }>('/api/leanctx/options', { toolProfile: p }).then(async (r) => { await loadOpts(); return r; }), (r) => r.note)} />
        </Row>
        {main.map((o) => <LcOptionRow key={o.key} o={o} label={LC_LABEL[o.key]} onSet={setOpt} />)}
        <div className="card" style={{ padding: '12px 16px', display: 'flex', flexDirection: 'column', gap: 8 }}>
          <span className="row"><span className="lbl grow">All options</span><span className="muted" style={{ fontSize: 12 }}>{opts.options.length} in lean-ctx’s schema · <span className="mono">{opts.path}</span></span></span>
          <input className="inp" aria-label="Find a lean-ctx option" placeholder="Find an option — e.g. archive, proxy, search, memory…" value={q} onChange={(e) => setQ(e.target.value)} />
          {rest.map((o) => <LcOptionRow key={o.key} o={o} onSet={setOpt} />)}
          {needle && !rest.length && <span className="muted" style={{ fontSize: 12 }}>No option matches.</span>}
        </div>
      </>}
      <Row label="Install folder" note="One install for this computer; on or off per workspace"><span className="mono ellipsis" style={{ fontSize: 11.5, maxWidth: 420 }} title={st.dir}>{st.dir}</span></Row>
    </>}
  </>;
}

function LcOptionRow({ o, label, onSet }: { o: LeanCtxOption; label?: string; onSet: (key: string, value: unknown) => void }) {
  const note = <>{o.description}{o.set ? '' : ' · default'}{label ? <> · <span className="mono">{o.key}</span></> : null}</>;
  const control = o.values ? (o.values.length <= 4 ? <Choice options={o.values} value={String(o.value)} onPick={(v) => onSet(o.key, v)} />
    : <select className="inp" aria-label={label || o.key} value={String(o.value)} onChange={(e) => onSet(o.key, e.target.value)} style={{ width: 180 }}>{o.values.map((v) => <option key={v} value={v}>{v}</option>)}</select>)
    : o.type === 'bool' ? <Switch on={!!o.value} label={label || o.key} onChange={() => onSet(o.key, !o.value)} />
    : /^(u|i|f)\d+|usize|number|float|int/.test(o.type) ? <TextSave value={String(o.value ?? '')} label={label || o.key} mono width={110} onSave={(v) => onSet(o.key, v)} />
    : o.type === 'string' ? <TextSave value={String(o.value ?? '')} label={label || o.key} mono width={200} onSave={(v) => onSet(o.key, v)} />
    : <span className="mono muted" style={{ fontSize: 11 }}>{JSON.stringify(o.value)}</span>;
  return <Row label={label || o.key} note={note}>{control}</Row>;
}

function RoutinesSettings() {
  const ws = useStore((s) => s.snap)!.workspace;
  return <>
    <Row label="Open routines" note="See the week, run history and create new ones"><button type="button" className="btn" onClick={() => go('routines')}>Open</button></Row>
    <Row label="Ask before acting" note="Routines pause at approval steps"><Switch on={ws.routineAskFirst} label="Ask before acting" onChange={() => void saveWs({ routineAskFirst: !ws.routineAskFirst })} /></Row>
    <Row label="If a computer is asleep"><Choice options={['Run when it wakes', 'Wake it', 'Skip'] as const} value={ws.routineIfAsleep} onPick={(routineIfAsleep) => void saveWs({ routineIfAsleep })} /></Row>
  </>;
}

function ComputersSettings() {
  const app = useStore((s) => s.app)!;
  const id = wsId();
  return <>
    {app.computers.map((c) => { const acc = c.access[id]; return <Row key={c.id} label={c.name} note={`${c.os} · ${c.status} · ${acc?.allowed ? 'allowed here' + (acc.folders.length ? ' · ' + acc.folders.map((f) => f.path + ' (' + f.mode + ')').join(', ') : '') : 'not allowed in this workspace'}`} />; })}
    {!app.computers.length && <p className="muted">No computers linked yet.</p>}
    <Row label="Manage computers" note="Permissions, folders, live view and pairing"><button type="button" className="btn" onClick={() => go('computers')}>Open</button></Row>
  </>;
}

function DataRetention() {
  const snap = useStore((s) => s.snap)!;
  const ws = snap.workspace;
  const isOrg = ws.kind === 'org';
  const [archived, setArchived] = useState<Project[]>([]);
  const load = () => get<{ projects: Project[] }>(`/api/ws/${wsId()}/archived`).then((r) => setArchived(r.projects)).catch(() => {});
  useEffect(() => { void load(); }, [snap.projects.length]);
  return <>
    <Row label="Data folder"><span className="mono" style={{ fontSize: 12 }}>~/.harness/ws/{ws.id}</span></Row>
    <Row label="Keep chats" note={isOrg ? 'Set by org policy' : ''} locked={isOrg}><Choice options={['Forever', '1 year', '90 days'] as const} value={ws.keepChats} disabled={isOrg} onPick={(keepChats) => void saveWs({ keepChats })} /></Row>
    <Row label="Export this workspace" note="Chats, artifacts, memory and config as .tar.gz — secrets stay in the keychain"><button type="button" className="btn" onClick={() => void download(`/api/ws/${wsId()}/export`).then(() => toast('Exported ' + ws.slug + '.tar.gz'), (e) => toast(e.message, 'error'))}>Export</button></Row>
    <span className="lbl" style={{ paddingTop: 8 }}>Archived projects</span>
    {archived.map((p) => <Row key={p.id} label={p.name} note={<span className="mono">{p.folder}</span>}><button type="button" className="btn" onClick={() => void act(() => post(`/api/ws/${wsId()}/projects/${p.id}/restore`).then(() => { void load(); return refreshSnap(); }), p.name + ' restored')}>Restore</button></Row>)}
    {!archived.length && <p className="muted" style={{ margin: 0 }}>Nothing archived.</p>}
  </>;
}

function Audit() {
  const snap = useStore((s) => s.snap)!;
  const [events, setEvents] = useState(snap.audit);
  const [q, setQ] = useState('');
  useEffect(() => { setEvents(snap.audit); }, [snap.audit]);
  const shown = events.filter((e) => !q || (e.action + ' ' + e.detail + ' ' + e.actor).toLowerCase().includes(q.toLowerCase()));
  return <>
    <div className="row"><input className="inp grow" aria-label="Filter audit log" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter (e.g. policy, provider, export)" /><button type="button" className="btn" onClick={() => void get<{ events: typeof events }>(`/api/ws/${wsId()}/audit?limit=1000`).then((r) => setEvents(r.events))}>Load all</button></div>
    <div className="card" style={{ padding: '6px 0' }}>
      {shown.map((e, i) => <div key={i} className="row" style={{ fontSize: 12.5, padding: '5px 14px', borderTop: i ? '1px solid var(--bg2)' : 0 }}><span className="mono muted" style={{ width: 150, flex: '0 0 auto' }}>{new Date(e.at).toLocaleString()}</span><b style={{ width: 160, flex: '0 0 auto' }}>{e.action}</b><span className="ellipsis grow">{e.detail}</span><span className="muted">{e.actor}</span></div>)}
      {!shown.length && <p className="muted" style={{ padding: '6px 14px', margin: 0 }}>Nothing yet. Actions you take appear here.</p>}
    </div>
  </>;
}

function EnginePanel() {
  const snap = useStore((s) => s.snap)!;
  const e = snap.workspace.engine;
  const st = snap.engine;
  const isOrg = snap.workspace.kind === 'org';
  const [url, setUrl] = useState(e.url);
  const [username, setUsername] = useState(e.username);
  const [password, setPassword] = useState('');
  const [log, setLog] = useState('');
  const [attach, setAttach] = useState('');
  const base = `/api/ws/${wsId()}/engine`;
  useEffect(() => { get<{ command: string }>(`/api/ws/${wsId()}/attach`).then((r) => setAttach(r.command)).catch(() => setAttach('')); }, [st.url]);
  const run = (path: string, ok: string) => void act(() => post<EngineStatus>(path).then(async () => { await refreshSnap(); }), ok);
  return <>
    <Row label="opencode" note="Start a server for this workspace, or use one you run" locked={isOrg}>
      <Choice options={['managed', 'remote'] as const} labels={{ managed: 'Managed by harness', remote: 'Connect to a server' }} value={e.mode} disabled={isOrg}
        onPick={(mode) => void act(() => post(`${base}/mode`, mode === 'managed' ? { mode } : { mode, url, username, password }).then(() => refreshSnap()), mode === 'managed' ? 'Harness now starts opencode for this workspace' : 'Connected to ' + url)} />
    </Row>
    <div className="card" style={{ padding: '12px 16px', display: 'grid', gridTemplateColumns: '2fr 1fr 1fr auto', gap: 10, alignItems: 'end' }}>
      <label className="field">Server URL<input className="inp mono" value={url} onChange={(ev) => setUrl(ev.target.value)} /></label>
      <label className="field">Username<input className="inp mono" value={username} onChange={(ev) => setUsername(ev.target.value)} /></label>
      <label className="field">Password<input className="inp mono" type="password" value={password} onChange={(ev) => setPassword(ev.target.value)} autoComplete="off" placeholder="kept in keychain" /></label>
      <button type="button" className="btn" disabled={isOrg} onClick={() => void act(() => post(`${base}/mode`, { mode: 'remote', url, username, password }).then(() => refreshSnap()), 'Reconnected · ' + url)}>{e.mode === 'remote' ? 'Reconnect' : 'Test & connect'}</button>
      <span className="muted" style={{ gridColumn: '1 / -1', fontSize: 12 }}>Connecting requires a successful test. Remote servers must use HTTPS or an SSH tunnel to localhost; basic auth with the password from the keychain.</span>
    </div>
    <Row label="Status" note={st.error || (st.url ? st.url : 'starts with the first chat')}><span className="row" style={{ gap: 6 }}><span className={'dot ' + (st.state === 'starting' || st.state === 'restarting' ? 'pulse' : '')} style={{ background: st.state === 'running' || st.state === 'connected' ? 'var(--success)' : st.state === 'error' ? 'var(--error)' : 'var(--dim)' }} /><span className="mono" style={{ fontSize: 12 }}>{st.state}{st.version ? ' · ' + st.version : ''}</span></span></Row>
    {st.sidecars.map((s) => <Row key={s.name} label={s.name} note={s.pid ? 'pid ' + s.pid : undefined}><span className="mono" style={{ fontSize: 12 }}>{s.state}</span></Row>)}
    <Row label="Servers" note="Open chats reconnect on their own"><span className="row" style={{ gap: 6 }}>
      <button type="button" className="btn" onClick={() => run(`${base}/start`, 'Engine started')}>Start</button>
      <button type="button" className="btn" onClick={() => run(`${base}/restart`, 'Servers restarted')}>Restart</button>
      <button type="button" className="btn" onClick={() => run(`${base}/stop`, 'Engine stopped')}>Stop</button>
      <button type="button" className="btn" onClick={() => void get<{ text: string }>(`${base}/logs`).then((r) => setLog(r.text || '(empty)')).catch((err) => toast(err.message, 'error'))}>Logs</button>
    </span></Row>
    <VersionPin current={e.version || 'system'} running={st.version} locked={isOrg} />
    <Row label="Sync sessions both ways" note="TUI sessions appear here; chats here appear in the TUI"><Switch on={e.sync} label="Sync sessions" onChange={() => void saveWs({ engine: { ...e, sync: !e.sync } })} /></Row>
    <Row label="Import sessions" note="Bring in sessions created outside harness; folders become projects"><button type="button" className="btn" onClick={() => void act(() => post<{ chats: number; projects: number }>(`/api/ws/${wsId()}/import-sessions`, {}).then(async (r) => { await refreshSnap(); return r; }), (r) => r.chats ? `Imported ${r.chats} chats into ${r.projects} new projects` : 'Nothing new to import')}>Scan</button></Row>
    <Row label="Attach the TUI" note="Continue any chat in the terminal (the chat menu copies its own command)"><span className="mono ellipsis" style={{ fontSize: 11.5, maxWidth: 420 }} title={attach}>{attach.replace(/OPENCODE_SERVER_PASSWORD='[^']*' /, '') || '—'}</span></Row>
    <Row label="Desktop shell" note="native window and updates"><span className="mono" style={{ fontSize: 12 }}>Tauri 2 · opencode and Open Design start as sidecars</span></Row>
    <Row label="Config"><span className="mono" style={{ fontSize: 12 }}>~/.harness/ws/{snap.workspace.id}/config/opencode.json</span></Row>
    {log && <pre className="pre" style={{ maxHeight: 280 }}>{log}</pre>}
  </>;
}

/** Pin this workspace to an exact opencode release (installed as is from npm), or follow the one on PATH. */
function VersionPin({ current, running, locked }: { current: string; running?: string; locked: boolean }) {
  const [v, setV] = useState(current === 'system' ? '' : current);
  const [busy, setBusy] = useState(false);
  const pin = (version: string) => void act(async () => {
    setBusy(true);
    try { const r = await post<{ version: string }>(`/api/ws/${wsId()}/engine/version`, { version, install: true }); await refreshSnap(); return r; } finally { setBusy(false); }
  }, (r) => (r.version === 'system' ? 'Using opencode from PATH' : 'Pinned to opencode ' + r.version));
  return <Row label="opencode version" note={(current === 'system' ? 'Follows the opencode on PATH' : 'Pinned to ' + current) + (running ? ' · running ' + running : '') + (locked ? ' · held by your organization' : '')} locked={locked}>
    <form className="row" style={{ gap: 6 }} onSubmit={(ev) => { ev.preventDefault(); if (v.trim()) pin(v.trim()); }}>
      <input className="inp mono" aria-label="opencode version to pin" value={v} disabled={locked || busy} onChange={(ev) => setV(ev.target.value)} placeholder={running || '2.2.1'} style={{ width: 110 }} />
      <button type="submit" className="btn" disabled={locked || busy || !v.trim() || v.trim() === current}>{busy ? 'Installing…' : 'Pin'}</button>
      {current !== 'system' && <button type="button" className="btn" disabled={locked || busy} onClick={() => pin('system')}>Use system</button>}
    </form>
  </Row>;
}

function Appearance() {
  const app = useStore((s) => s.app)!;
  const s = app.settings;
  const patchApp = (body: Record<string, unknown>, ok: string) => void act(() => patch('/api/settings', body).then((next) => setState({ app: next })), ok);
  return <>
    <Row label="Theme"><Choice options={['claude', 'opencode'] as const} labels={{ claude: 'Claude', opencode: 'opencode' }} value={s.theme} onPick={(theme) => patchApp({ theme, followSystem: false }, 'Theme · ' + theme)} /></Row>
    <Row label="Follow system light and dark" note="Claude when light, opencode when dark"><Switch on={s.followSystem} label="Follow system" onChange={() => patchApp({ followSystem: !s.followSystem }, s.followSystem ? 'Following your choice' : 'Following the system')} /></Row>
    <Row label="Accent" note="Default accent; each workspace can set its own in General"><Choice options={ACCENTS.map((a) => a.id)} labels={Object.fromEntries(ACCENTS.map((a) => [a.id, a.name]))} value={s.accent} onPick={(accent) => patchApp({ accent }, 'Accent updated')} /></Row>
    <Row label="Reduce motion" note="Also follows your system setting"><Switch on={s.reduceMotion} label="Reduce motion" onChange={() => patchApp({ reduceMotion: !s.reduceMotion }, s.reduceMotion ? 'Motion on' : 'Motion reduced')} /></Row>
  </>;
}

void getState;
