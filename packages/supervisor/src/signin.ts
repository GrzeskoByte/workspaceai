// Signing a subscription in again from the chat. A workspace keeps its own copy of the agent's login, and that copy
// can expire on its own (Claude Code: "OAuth session expired and could not be refreshed"). A failed turn that says so
// is marked on the message; the chat then offers to sign in again — with the agent's own browser sign-in, which
// gives the workspace a session of its own, or by copying this computer's login again — and retries the message.
import { spawn, type ChildProcess } from 'node:child_process';
import type { SubscriptionId } from '../../shared/src/types.ts';
import { findOnPath } from './acp.ts';
import { baseEnv } from './opencode.ts';
import { agentEnv } from './subscriptions.ts';
import type { WorkspaceRuntime } from './runtime.ts';

const AUTH = /failed to authenticate|oauth (session|token)[^.]*(expired|revoked|invalid|refreshed)|not (logged|signed) in|please (run \/login|log ?in|sign in)|authentication (required|failed)|invalid api key|unauthori[sz]ed|\b401\b|auth(entication)?_required|token (has )?expired|login (has )?expired|re-?authenticate/i;

/** Whether an agent's error (or its whole, short reply) means its sign-in is gone. */
export function isAuthFailure(text: string | undefined): boolean {
  return !!text && AUTH.test(text);
}

export interface SignInState { state: 'idle' | 'waiting' | 'done' | 'failed'; url?: string; error?: string; startedAt?: number }

interface Run extends SignInState { proc?: ChildProcess; output: string; waiters: (() => void)[] }

const runs = new Map<string, Run>();
const key = (rt: WorkspaceRuntime, id: SubscriptionId) => rt.id + ':' + id;

/** The agent's own browser sign-in, into this workspace's agent home. Only Claude Code has one that runs unattended. */
const COMMANDS: Partial<Record<SubscriptionId, string[]>> = { claude: ['claude', 'auth', 'login', '--claudeai'] };
export function canSignIn(id: SubscriptionId): boolean { return !!COMMANDS[id] && !!findOnPath(COMMANDS[id]![0]); }

export function signInState(rt: WorkspaceRuntime, id: SubscriptionId): SignInState {
  const r = runs.get(key(rt, id));
  return r ? { state: r.state, url: r.url, error: r.error, startedAt: r.startedAt } : { state: 'idle' };
}

/**
 * Start the sign-in: the agent opens the browser itself and finishes on its own callback; the link it prints is
 * returned too, for when no browser opens (that page shows a code, which `signInCode` passes on).
 */
export async function startSignIn(rt: WorkspaceRuntime, id: SubscriptionId, onDone: (ok: boolean) => void): Promise<SignInState> {
  const cmd = COMMANDS[id];
  const bin = cmd && findOnPath(cmd[0]);
  if (!cmd || !bin) throw Object.assign(new Error(`Sign in to ${id} from a terminal, then connect it again in Settings › Model providers`), { status: 409 });
  const k = key(rt, id);
  const prev = runs.get(k);
  if (prev?.state === 'waiting' && prev.proc && prev.proc.exitCode === null) return signInState(rt, id);
  const proc = spawn(bin, cmd.slice(1), { env: { ...baseEnv(), ...agentEnv(rt, id) }, stdio: ['pipe', 'pipe', 'pipe'] });
  const run: Run = { state: 'waiting', proc, output: '', waiters: [], startedAt: Date.now() };
  runs.set(k, run);
  const wake = () => { for (const w of run.waiters.splice(0)) w(); };
  const read = (b: Buffer) => {
    run.output = (run.output + b.toString()).slice(-8000);
    const url = /https:\/\/\S+oauth\S+/.exec(run.output.replace(/\x1b\[[0-9;]*m/g, ''))?.[0];
    if (url && !run.url) { run.url = url; wake(); }
  };
  proc.stdout?.on('data', read);
  proc.stderr?.on('data', read);
  proc.on('error', (e) => { run.state = 'failed'; run.error = e.message; wake(); onDone(false); });
  proc.on('exit', (code) => {
    if (run.state !== 'waiting') return;
    run.state = code === 0 ? 'done' : 'failed';
    if (code !== 0) run.error = run.output.replace(/\x1b\[[0-9;]*m/g, '').trim().split('\n').filter(Boolean).slice(-2).join(' ').slice(0, 300) || `Sign-in ended (exit ${code})`;
    run.proc = undefined;
    wake();
    onDone(code === 0);
  });
  // Ten minutes to finish in the browser.
  setTimeout(() => { if (run.state === 'waiting' && run.proc) { run.error = 'Sign-in timed out'; run.state = 'failed'; run.proc.kill(); wake(); } }, 10 * 60000).unref();
  await new Promise<void>((res) => { run.waiters.push(res); setTimeout(res, 15000).unref(); });
  return signInState(rt, id);
}

/** The code the sign-in page shows when the browser could not hand it back by itself. */
export function signInCode(rt: WorkspaceRuntime, id: SubscriptionId, code: string): SignInState {
  const r = runs.get(key(rt, id));
  if (!r?.proc || r.state !== 'waiting') throw Object.assign(new Error('No sign-in is waiting for a code — start it again'), { status: 409 });
  r.proc.stdin?.write(code.trim() + '\n');
  return signInState(rt, id);
}

export function cancelSignIn(rt: WorkspaceRuntime, id: SubscriptionId): SignInState {
  const r = runs.get(key(rt, id));
  if (r?.proc) { r.state = 'idle'; r.proc.kill(); r.proc = undefined; }
  runs.delete(key(rt, id));
  return { state: 'idle' };
}
