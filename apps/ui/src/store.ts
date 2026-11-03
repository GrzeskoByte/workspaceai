// Minimal external store: app state from the supervisor + the active workspace snapshot + UI navigation.
import { useSyncExternalStore } from 'react';
import type { AppState, WorkspaceSnapshot, ChatMessage, ToolStep, GraphRun, ModelGroup, ModelOption } from '../../../packages/shared/src/types.ts';
import { get, post, events } from './api.ts';

export type View = 'home' | 'chat' | 'artifacts' | 'project' | 'settings' | 'graph' | 'map' | 'routines' | 'computers' | 'memory' | 'brief' | 'generate';

export interface LiveTurn { text: string; tools: ToolStep[]; messageId: string }

export interface UiState {
  app: AppState | null;
  snap: WorkspaceSnapshot | null;
  view: View;
  projId: string | null;
  chatId: string | null;
  artId: string | null;
  /** The artifact panel fills the main area (the chat or library beside it is hidden until it narrows again). */
  artWide: boolean;
  /** The browser beside the chat or project (a dev server, any page); stays open while you move around. */
  browser: { url: string } | null;
  browserWide: boolean;
  /** The graph the editor opens on. */
  graphId: string | null;
  /** A graph run shown beside the chat or project: its diagram, live. */
  runId: string | null;
  runWide: boolean;
  /** The project's mind map beside the chat. */
  mapOpen: boolean;
  mapWide: boolean;
  /** Bumped when a map changed or a run moved, so an open map refetches. */
  mapTick: number;
  composeProj: string | null;
  setPage: string;
  ptab: string;
  open: Record<string, boolean>;
  palette: boolean;
  newWs: null | { type: 'team' | 'personal' | 'org'; name: string };
  customProvider: boolean;
  confirm: null | { title: string; body: string; ok: string; run: () => void | Promise<void> };
  toasts: { id: number; text: string; kind?: 'error' }[];
  messages: Record<string, ChatMessage[]>;
  live: Record<string, LiveTurn>;
  permissions: Record<string, { id: string; sid: string; title: string; detail: string }[]>;
  /** Narrow windows: the project sidebar and workspace rail slide in as a drawer. */
  navOpen: boolean;
  homeAgent: string | null;
  homeWeb: boolean | null;
  homeModel: string | null;
  homeEffort: string | null;
  mode: string;
  draft: string;
  models: string[];
  /** Connected opencode providers and every model each serves, with effort levels. */
  modelGroups: ModelGroup[];
  /** ACP agent id → the models its subscription offers and the one it starts on. */
  agentModels: Record<string, AgentModels>;
  runs: GraphRun[];
  ready: boolean;
  error: string | null;
}

let state: UiState = {
  app: null, snap: null, view: 'home', projId: null, chatId: null, artId: null, artWide: false, browser: null, browserWide: false, graphId: null, runId: null, runWide: false, mapOpen: false, mapWide: false, mapTick: 0, composeProj: null, setPage: 'general', ptab: 'chats',
  open: {}, palette: false, newWs: null, customProvider: false, confirm: null, toasts: [], messages: {}, live: {}, permissions: {},
  navOpen: false, homeAgent: null, homeWeb: null, homeModel: null, homeEffort: null, mode: 'chat', draft: '', models: [], modelGroups: [], agentModels: {}, runs: [], ready: false, error: null,
};

export interface AgentModels { models: ModelOption[]; current: string }
const subs = new Set<() => void>();

export function getState(): UiState { return state; }
export function setState(patch: Partial<UiState> | ((s: UiState) => Partial<UiState>)): void {
  const p = typeof patch === 'function' ? patch(state) : patch;
  state = { ...state, ...p };
  for (const s of subs) s();
}
export function useStore<T>(sel: (s: UiState) => T): T {
  return useSyncExternalStore((cb) => { subs.add(cb); return () => subs.delete(cb); }, () => sel(state));
}

let toastSeq = 0;
export function toast(text: string, kind?: 'error'): void {
  const id = ++toastSeq;
  setState((s) => ({ toasts: [...s.toasts, { id, text, kind }].slice(-3) }));
  setTimeout(() => setState((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), kind === 'error' ? 5000 : 2800);
}

/** Run an action; show its error as a toast instead of throwing. */
export async function act<T>(fn: () => Promise<T>, ok?: string | ((r: T) => string)): Promise<T | undefined> {
  try {
    const r = await fn();
    if (ok) toast(typeof ok === 'function' ? ok(r) : ok);
    return r;
  } catch (e) {
    toast((e as Error).message || String(e), 'error');
    return undefined;
  }
}

export function ws() { return state.snap?.workspace ?? null; }
export function wsId(): string { return state.app?.settings.activeWorkspace || ''; }

export async function refreshApp(): Promise<void> { setState({ app: await get<AppState>('/api/state') }); }

/** Same JSON → same object, so a refreshed snapshot only re-renders what actually changed. */
function same<T>(a: T, b: T): boolean { return a === b || JSON.stringify(a) === JSON.stringify(b); }
function shareList<T extends { id: string }>(prev: T[], next: T[]): T[] {
  const byId = new Map(prev.map((x) => [x.id, x]));
  let changed = prev.length !== next.length;
  const out = next.map((x, i) => { const o = byId.get(x.id); if (o && same(o, x)) { if (prev[i] !== o) changed = true; return o; } changed = true; return x; });
  return changed ? out : prev;
}
function shareSnapshot(prev: WorkspaceSnapshot, next: WorkspaceSnapshot): WorkspaceSnapshot {
  const out = { ...next } as Record<string, unknown>;
  let changed = false;
  for (const k of Object.keys(next) as (keyof WorkspaceSnapshot)[]) {
    const a = prev[k] as unknown, b = next[k] as unknown;
    const v = Array.isArray(a) && Array.isArray(b) && b.every((x) => x && typeof x === 'object' && 'id' in x) ? shareList(a as { id: string }[], b as { id: string }[]) : same(a, b) ? a : b;
    out[k] = v;
    if (v !== a) changed = true;
  }
  return changed ? (out as unknown as WorkspaceSnapshot) : prev;
}

export async function refreshSnap(): Promise<void> {
  const id = wsId();
  if (!id) { setState({ snap: null }); return; }
  const fresh = await get<WorkspaceSnapshot>(`/api/ws/${id}/snapshot`);
  if (wsId() !== id) return; // switched meanwhile
  const snap = state.snap && state.snap.workspace.id === id ? shareSnapshot(state.snap, fresh) : fresh;
  if (snap === state.snap) return; // nothing changed: no re-render at all
  setState((s) => ({ snap, projId: s.projId && snap.projects.some((p) => p.id === s.projId) ? s.projId : (snap.projects.find((p) => !p.isGeneral)?.id || snap.projects[0]?.id || null) }));
}

export async function refreshModels(): Promise<void> {
  const id = wsId(); if (!id) return;
  const r = await get<{ models: string[]; groups?: ModelGroup[]; agents?: Record<string, AgentModels> }>(`/api/ws/${id}/models`).catch(() => ({ models: [], groups: [], agents: {} }));
  if (wsId() !== id) return;
  setState({ models: r.models, modelGroups: r.groups || [], agentModels: r.agents || {} });
}

export async function loadMessages(chatId: string): Promise<void> {
  const id = wsId();
  const r = await get<{ messages: ChatMessage[]; streaming: boolean }>(`/api/ws/${id}/chats/${chatId}/messages`).catch((e) => { toast(e.message, 'error'); return null; });
  if (!r) return;
  setState((s) => ({ messages: { ...s.messages, [chatId]: r.messages }, live: r.streaming ? s.live : omit(s.live, chatId) }));
}

/** Reconcile the permission cards with the engine (survives app restarts and missed SSE events). */
export async function loadPermissions(chatId: string): Promise<void> {
  const id = wsId();
  const r = await get<{ permissions: { id: string; sid: string; title: string; detail: string }[] }>(`/api/ws/${id}/chats/${chatId}/permissions`).catch(() => null);
  if (!r) return;
  setState((s) => ({ permissions: { ...s.permissions, [chatId]: r.permissions } }));
}

function omit<T>(o: Record<string, T>, k: string): Record<string, T> { const c = { ...o }; delete c[k]; return c; }

export async function switchWorkspace(id: string): Promise<void> {
  // Switching never stops the other workspace's server: its chats keep running (R-05).
  const app = await post<AppState>(`/api/workspaces/${id}/activate`);
  setState({ app, snap: null, view: 'home', chatId: null, artId: null, composeProj: null, projId: null, messages: {}, live: {}, models: [], modelGroups: [], agentModels: {}, runs: [] });
  await refreshSnap();
  refreshModels();
  const name = app.workspaces.find((w) => w.id === id)?.name;
  if (name) toast('Switched to ' + name);
}

export function go(view: View, extra: Partial<UiState> = {}): void {
  setState({ view, palette: false, navOpen: false, artWide: false, browserWide: false, runWide: false, mapWide: false, ...extra });
  if (view === 'chat' && (extra.chatId || state.chatId)) loadMessages((extra.chatId || state.chatId) as string);
}

let pending: Record<string, { messageId: string; text: string }> = {};
let frame = 0;
function flushDeltas(): void {
  frame = 0;
  const batch = pending; pending = {};
  setState((s) => {
    const live = { ...s.live };
    for (const [chat, d] of Object.entries(batch)) { const t = live[chat] || { text: '', tools: [], messageId: d.messageId }; live[chat] = { ...t, text: t.text + d.text }; }
    return { live };
  });
}
function queueDelta(chat: string, messageId: string, delta: string): void {
  const p = pending[chat] || (pending[chat] = { messageId, text: '' });
  p.text += delta;
  if (!frame) frame = typeof requestAnimationFrame === 'function' && !document.hidden ? requestAnimationFrame(flushDeltas) : window.setTimeout(flushDeltas, 50);
}

export function startEvents(): () => void {
  let snapTimer: number | undefined;
  return events((e) => {
    switch (e.type) {
      case 'state': refreshApp().then(() => { if (state.snap && state.snap.workspace.id !== wsId()) { setState({ snap: null, chatId: null, artId: null, messages: {}, live: {} }); refreshSnap(); } }); break;
      case 'snapshot': if (e.workspace === wsId()) { clearTimeout(snapTimer); snapTimer = window.setTimeout(() => refreshSnap(), 120); } break;
      case 'engine': if (e.workspace === wsId()) setState((s) => (s.snap ? { snap: { ...s.snap, engine: e.status } } : {})); if (e.status.state === 'running' || e.status.state === 'connected') refreshModels(); break;
      case 'models': if (e.workspace === wsId()) refreshModels(); break;
      // Tokens arrive far faster than frames: gather them and render once per frame.
      case 'chat.delta': if (e.workspace === wsId()) queueDelta(e.chat, e.messageId, e.delta); break;
      case 'chat.tool': if (e.workspace === wsId()) setState((s) => {
        const t = s.live[e.chat] || { text: '', tools: [], messageId: e.messageId };
        const tools = t.tools.some((x) => x.detail === e.tool.detail) ? t.tools.map((x) => (x.detail === e.tool.detail ? e.tool : x)) : [...t.tools, e.tool];
        return { live: { ...s.live, [e.chat]: { ...t, tools } } };
      }); break;
      case 'chat.done': if (e.workspace === wsId()) { if (pending[e.chat]) flushDeltas(); loadMessages(e.chat).then(() => setState((s) => ({ live: omit(s.live, e.chat), permissions: omit(s.permissions, e.chat) }))); if (e.outcome === 'failed') toast('The model call failed — see the chat for details', 'error'); } break;
      case 'chat.permission': if (e.workspace === wsId()) {
        // A graph run's step asking: say so, since its chat is usually not on screen.
        const run = state.runs.find((r) => r.chats?.includes(e.chat));
        if (run && !(state.view === 'chat' && state.chatId === e.chat)) toast(`${state.snap?.graphs.find((g) => g.id === run.graph)?.name || 'A graph run'} is waiting for your approval: ${e.request.title}`);
        setState((s) => ({ permissions: { ...s.permissions, [e.chat]: [...(s.permissions[e.chat] || []).filter((p) => p.id !== e.request.id), e.request] } }));
      } break;
      case 'chat.permission.clear': if (e.workspace === wsId()) setState((s) => ({ permissions: { ...s.permissions, [e.chat]: e.requestId ? (s.permissions[e.chat] || []).filter((p) => p.id !== e.requestId) : [] } })); break;
      // An agent saved an artifact: open it beside the chat it came from, if that chat is on screen.
      case 'artifact': if (e.workspace === wsId()) {
        refreshSnap().then(() => {
          if (state.view === 'chat' && state.chatId === e.chat) setState({ artId: e.artifact });
          else toast(`${state.snap?.artifacts.find((a) => a.id === e.artifact)?.title || 'An artifact'} is ready${e.version > 1 ? ` · v${e.version}` : ''}`);
        });
      } break;
      case 'map': if (e.workspace === wsId()) setState((s) => ({ mapTick: s.mapTick + 1 })); break;
      case 'toast': toast(e.text); break;
      // A run started from the chat on screen opens beside it, so it can be watched step by step.
      case 'graph.run': if (e.workspace === wsId()) setState((s) => {
        const fresh = !s.runs.some((r) => r.id === e.run.id);
        const before = s.runs.find((r) => r.id === e.run.id);
        if (e.run.status === 'failed' && before?.status !== 'failed') {
          const name = s.snap?.graphs.find((g) => g.id === e.run.graph)?.name || 'Graph';
          const why = [...e.run.log].reverse().find((l) => /failed|error/i.test(l.text))?.text || e.run.log[e.run.log.length - 1]?.text || '';
          window.setTimeout(() => toast(`${name} failed${why ? ': ' + why.replace(/^\w+ · [^·]+ · failed: /, '') : ''}`, 'error'), 0);
        }
        const show = fresh && e.run.from && s.view === 'chat' && s.chatId === e.run.from;
        return { mapTick: s.mapTick + (before?.status !== e.run.status ? 1 : 0), runs: [e.run, ...s.runs.filter((r) => r.id !== e.run.id)].slice(0, 40), ...(show ? { runId: e.run.id, artId: null, artWide: false } : {}) };
      }); break;
      default: break;
    }
  });
}

/** Open a page in the browser beside the chat (closes an open artifact, which would cover it). */
export function openBrowser(url = ''): void {
  setState((s) => ({ browser: { url: url || s.browser?.url || '' }, artId: null, artWide: false }));
}

/** Show a graph run beside the chat or project. */
export function openRun(id: string): void { setState({ runId: id, artId: null, artWide: false }); }
