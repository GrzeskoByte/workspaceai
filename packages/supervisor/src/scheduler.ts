// Routines fire on their own (R-27): cron schedules with a time zone, one-off times, file changes, git events and
// webhooks. Approval, "if the computer is asleep" and notifications follow the routine's settings.
import { watch, existsSync, type FSWatcher } from 'node:fs';
import { join } from 'node:path';
import { randomBytes } from 'node:crypto';
import { CronExpressionParser } from 'cron-parser';
import type { Routine, RoutineRun } from '../../shared/src/types.ts';
import { uid } from '../../shared/src/util.ts';
import { startGraphRun } from './graphs.ts';
import { expandHome } from './paths.ts';
import type { Supervisor } from './supervisor.ts';
import type { WorkspaceRuntime } from './runtime.ts';
import { startLive, updateLive } from './computers.ts';
import { GraphifyWatcher } from './graphify.ts';

function record(rt: WorkspaceRuntime, routine: Routine, run: RoutineRun): void {
  routine.runs.unshift(run);
  routine.runs = routine.runs.slice(0, 60);
  rt.changed();
  rt.emit({ type: 'routine.run', workspace: rt.id, routine: routine.id, run });
  if ((routine.notify || []).includes('push') && run.status !== 'running') rt.emit({ type: 'toast', text: `${routine.name}: ${run.status} · ${run.note}` });
}

/** Do the routine's work now. */
export async function perform(rt: WorkspaceRuntime, routine: Routine, reason = 'run now', sup?: Supervisor): Promise<Routine> {
  const onComputer = sup && routine.runsOn === 'computer' && routine.computer ? routine.computer : null;
  try {
    if (routine.what.kind === 'graph') {
      const run = await startGraphRun(rt, routine.what.graph, false);
      record(rt, routine, { id: run.id, at: Date.now(), status: 'running', note: `Graph run started (${reason})` });
      if (onComputer) startLive(sup!, onComputer, { workspace: rt.id, title: routine.name, action: 'Starting the graph', ref: { kind: 'graph', id: run.id } });
    } else if (routine.what.kind === 'prompt') {
      const chat = await rt.createChat({ project: routine.project, title: `${routine.name} · ${new Date().toLocaleDateString()}` });
      await rt.send(chat.id, routine.what.prompt);
      record(rt, routine, { id: uid('rr'), at: Date.now(), status: 'ok', note: `Started in a chat (${reason})`, chat: chat.id });
      if (onComputer) startLive(sup!, onComputer, { workspace: rt.id, title: routine.name, action: 'Working on: ' + routine.what.prompt.slice(0, 120), ref: { kind: 'chat', id: chat.id } });
    } else {
      // Artifact refresh: ask the chat that made it (or a new chat in its project) for a new version.
      const a = rt.state.artifacts.find((x) => x.id === (routine.what as { artifact: string }).artifact);
      if (!a) throw new Error('The artifact is gone');
      const chat = (a.chat && rt.chat(a.chat)) || (await rt.createChat({ project: a.project, title: `Refresh · ${a.title}` }));
      await rt.send(chat.id, `[Routine “${routine.name}”] Refresh the artifact “${a.title}” (artifact id ${a.id}) with current information and save it with artifact_update as a new version.`);
      record(rt, routine, { id: uid('rr'), at: Date.now(), status: 'ok', note: `Refresh of “${a.title}” started (${reason})`, chat: chat.id });
    }
  } catch (e) {
    record(rt, routine, { id: uid('rr'), at: Date.now(), status: 'error', note: (e as Error).message });
  }
  return routine;
}

/** A trigger arrived: apply runs-on, sleep and approval rules, then run or wait. */
export async function trigger(sup: Supervisor, rt: WorkspaceRuntime, routine: Routine, reason: string, at = Date.now()): Promise<void> {
  routine.lastFired = at;
  if (routine.runsOn === 'computer' && routine.computer) {
    const pc = sup.computers.find((c) => c.id === routine.computer);
    if (!pc || !pc.access[rt.id]?.allowed) { record(rt, routine, { id: uid('rr'), at: Date.now(), status: 'skipped', note: 'That computer is not allowed in this workspace' }); return; }
    if (pc.status !== 'online') {
      if (routine.ifAsleep === 'Skip') { record(rt, routine, { id: uid('rr'), at: Date.now(), status: 'skipped', note: `${pc.name} is asleep — skipped` }); return; }
      if (routine.ifAsleep === 'Wake it' && pc.capabilities.wake) { try { await sup.wakeComputer(pc.id); } catch { /* recorded below */ } }
      if (sup.computers.find((c) => c.id === pc.id)?.status !== 'online') { record(rt, routine, { id: uid('rr'), at: Date.now(), status: 'waiting', note: `Waiting for ${pc.name} to wake (${reason})` }); return; }
    }
  }
  if (routine.approval === 'ask') { record(rt, routine, { id: uid('rr'), at: Date.now(), status: 'waiting', note: `Needs your approval before it runs (${reason})` }); return; }
  await perform(rt, routine, reason, sup);
}

/** The time a repeating schedule was last due at or before `now`. */
export function lastDue(cron: string, tz: string, now: Date): number | null {
  try { return CronExpressionParser.parse(cron, { tz, currentDate: new Date(now.getTime() + 1000) }).prev().toDate().getTime(); } catch { return null; }
}

export function dueNow(routine: Routine, now: Date): boolean {
  if (routine.paused || routine.trigger.kind !== 'schedule') return false;
  const since = routine.lastFired ?? routine.createdAt;
  if (routine.trigger.once) return routine.trigger.once <= now.getTime() && !routine.lastFired;
  const due = lastDue(routine.trigger.cron, routine.trigger.tz, now);
  return due != null && due > since;
}

export class Scheduler {
  private timer: NodeJS.Timeout | null = null;
  private watchers = new Map<string, FSWatcher[]>();
  private sup: Supervisor;
  readonly graphify = new GraphifyWatcher();
  constructor(sup: Supervisor) {
    this.sup = sup;
    // Live view: graph progress on a computer updates its "next action"; a finished run ends the session.
    sup.on((e) => {
      if (e.type === 'graph.run') {
        for (const c of sup.computers) {
          if (c.live?.ref.kind !== 'graph' || c.live.ref.id !== e.run.id || c.live.state === 'stopped') continue;
          const last = e.run.log[e.run.log.length - 1];
          if (last) updateLive(sup, c.id, last.text);
          if (['completed', 'failed', 'stopped'].includes(e.run.status)) { updateLive(sup, c.id, 'Finished: ' + e.run.status); c.live.state = 'stopped'; sup.saveComputers(); }
        }
      }
      if (e.type === 'chat.done') {
        for (const c of sup.computers) if (c.live?.ref.kind === 'chat' && c.live.ref.id === e.chat && c.live.state === 'running') { updateLive(sup, c.id, 'Finished'); c.live!.state = 'stopped'; sup.saveComputers(); }
      }
    });
  }

  start(intervalMs = 20000): void {
    this.timer = setInterval(() => void this.tick(), intervalMs);
    this.timer.unref();
    void this.tick();
  }

  stop(): void { this.graphify.stop(); if (this.timer) clearInterval(this.timer); for (const ws of this.watchers.values()) for (const w of ws) w.close(); this.watchers.clear(); }

  async tick(now = new Date()): Promise<void> {
    for (const rt of this.sup.runtimes.values()) {
      for (const routine of rt.state.routines) {
        if (dueNow(routine, now)) {
          await trigger(this.sup, rt, routine, 'scheduled', now.getTime());
          if (routine.trigger.kind === 'schedule' && routine.trigger.once) routine.paused = true;
          rt.save();
        }
        // A routine waiting for its computer runs once the computer is back.
        const waiting = routine.runs[0];
        if (waiting?.status === 'waiting' && /^Waiting for .* to wake/.test(waiting.note) && this.sup.computers.find((c) => c.id === routine.computer)?.status === 'online') {
          waiting.status = 'ok'; waiting.note = 'Computer woke — ran';
          await perform(rt, routine, 'computer woke');
        }
      }
      this.syncWatchers(rt);
      this.graphify.sync(rt);
      void rt.uploadAudit(now.getTime()).catch((e) => rt.audit('audit.upload.error', String((e as Error).message), 'harness'));
    }
  }

  /** File-change and git triggers watch the routine's project folder. */
  private syncWatchers(rt: WorkspaceRuntime): void {
    for (const routine of rt.state.routines) {
      const key = rt.id + '/' + routine.id;
      const want = !routine.paused && routine.trigger.kind === 'event' && ['file.changed', 'git.push', 'git.commit'].includes(routine.trigger.event);
      if (!want) { this.watchers.get(key)?.forEach((w) => w.close()); this.watchers.delete(key); continue; }
      if (this.watchers.has(key)) continue;
      const proj = rt.project(routine.project);
      if (!proj) continue;
      const folder = expandHome(proj.folder);
      const t = routine.trigger as { event: string; detail: string };
      let debounce: NodeJS.Timeout | null = null;
      const fire = (why: string) => { if (debounce) clearTimeout(debounce); debounce = setTimeout(() => void trigger(this.sup, rt, routine, why), 1500); };
      const list: FSWatcher[] = [];
      try {
        if (t.event === 'file.changed') {
          const sub = t.detail && t.detail !== '.' ? join(folder, t.detail.replace(/^~\/?/, '')) : folder;
          if (existsSync(sub)) list.push(watch(sub, { recursive: true }, (_e, f) => { if (f && !String(f).startsWith('.harness') && !String(f).startsWith('.git')) fire('file changed: ' + f); }));
        } else {
          const refs = join(folder, '.git', 'refs', 'heads');
          const branch = t.event === 'git.push' && t.detail && t.detail !== '*' ? t.detail : null;
          if (existsSync(refs)) list.push(watch(refs, { recursive: true }, (_e, f) => { if (!branch || String(f) === branch) fire(`${t.event === 'git.push' ? 'push to' : 'commit on'} ${f}`); }));
        }
      } catch { /* folder vanished */ }
      this.watchers.set(key, list);
    }
  }
}

/** Per-routine secret for its webhook URL. */
export function webhookKey(routine: Routine): string {
  if (!routine.hookKey) routine.hookKey = randomBytes(18).toString('base64url');
  return routine.hookKey;
}
