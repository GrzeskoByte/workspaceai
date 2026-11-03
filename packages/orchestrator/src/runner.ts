// Executes a BPMN graph with bpmn-engine (R-38). State is readable after every activity
// so a run can be paused, and recovered after the process stops.
import { EventEmitter } from 'node:events';
import { Engine, type BpmnEngineExecutionState } from 'bpmn-engine';

export interface RunLog { at: number; element: string; text: string }
export interface RunWait { element: string; name: string; prompt: string }
export type RunStatus = 'running' | 'paused' | 'waiting' | 'completed' | 'failed' | 'stopped';

export interface RunUpdate {
  status?: RunStatus;
  log?: RunLog;
  waiting?: RunWait | null;
  completed?: string;
  error?: string;
  state?: BpmnEngineExecutionState | null;
  /** Outputs produced by steps, keyed by element id. */
  outputs?: Record<string, unknown>;
  /** A step the engine entered (it is running until it completes, waits or fails). */
  started?: string;
  /** A sequence flow the run went along. */
  taken?: string;
  /** The step that failed. */
  failedAt?: string;
}

/** One task the engine reached, with its harness attributes and the outputs of earlier steps. */
export interface StepRequest { element: string; name: string; type: string; attrs: Record<string, string>; outputs: Record<string, unknown> }
export type StepExecutor = (step: StepRequest) => Promise<unknown>;

/** harness:* attributes per element id, read straight from the XML (the engine does not surface extension attributes). */
export function harnessAttrsByElement(xml: string): Map<string, { tag: string; name: string; attrs: Record<string, string> }> {
  const out = new Map<string, { tag: string; name: string; attrs: Record<string, string> }>();
  const unesc = (v: string) => v.replace(/&quot;/g, '"').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');
  for (const m of xml.matchAll(/<(?:bpmn:)?([A-Za-z]+)\b([^>]*?)\/?>/g)) {
    const attrText = m[2];
    const id = /\bid="([^"]+)"/.exec(attrText)?.[1];
    if (!id) continue;
    const attrs: Record<string, string> = {};
    for (const a of attrText.matchAll(/\bharness:([A-Za-z]+)="([^"]*)"/g)) attrs[a[1]] = unesc(a[2]);
    out.set(id, { tag: m[1], name: unesc(/\bname="([^"]*)"/.exec(attrText)?.[1] || ''), attrs });
  }
  return out;
}

/** Sample output a dry run uses instead of calling anything. */
export function sampleOutput(step: StepRequest): unknown {
  const t = step.attrs.type || 'agent';
  if (t === 'browser') return { url: step.attrs.url || '[URL]', title: '[PAGE TITLE]', text: '[page text the Browser step would read]' };
  if (t === 'script') return { exitCode: 0, stdout: '[script output]' };
  if (t === 'rule') return { verdict: 'pass', score: 0.8 };
  return { text: '[sample answer for “' + (step.attrs.prompt || step.name) + '”]' };
}

export interface LiveGraph {
  signal(elementId: string): void;
  pause(): Promise<void>;
  resume(): Promise<void>;
  stop(): Promise<BpmnEngineExecutionState | null>;
  state(): Promise<BpmnEngineExecutionState | null>;
  done: Promise<void>;
}

function textOf(api: { id?: string; type?: string; name?: string; content?: { id?: string; name?: string; type?: string } } | undefined, field: 'id' | 'name' | 'type'): string {
  return String(api?.[field] || api?.content?.[field] || '');
}

/**
 * Start or recover a graph. Pass `saved` to recover. Pass `hold` to load a paused run
 * without continuing it. Agent steps call `services.agent`. User tasks wait until `signal`.
 * A dry run continues through user tasks on its own.
 */
export function launchGraph(opts: {
  source: string;
  saved?: BpmnEngineExecutionState | null;
  dryRun: boolean;
  hold?: boolean;
  /** Does the real work of agent, browser, script, service and rule steps. Not called in a dry run. */
  exec?: StepExecutor;
  /** Outputs of steps finished before a recovery. */
  outputs?: Record<string, unknown>;
  onUpdate: (u: RunUpdate) => void;
}): LiveGraph {
  const attrsById = harnessAttrsByElement(opts.source);
  const outputs: Record<string, unknown> = { ...(opts.outputs || {}) };
  let resolveDone = () => {};
  const done = new Promise<void>((resolve) => { resolveDone = resolve; });
  let settled = false;
  const emit = (u: RunUpdate) => {
    opts.onUpdate(u);
    if (!settled && (u.status === 'completed' || u.status === 'failed' || u.status === 'stopped')) {
      settled = true;
      resolveDone();
    }
  };

  const listener = new EventEmitter();
  const engine = new Engine({
    name: 'harness-graph',
    source: opts.source,
    listener,
    services: {
      // bpmn-engine resolves ${environment.services.agent} and calls it as
      // agent(scope, next). The activity stays open until next(err, output).
      // This records the step. It does not call a model.
      agent(scope: { id?: string; content?: { id?: string; name?: string } }, next?: (err: Error | null, output?: unknown) => void) {
        const element = String(scope?.content?.id || scope?.id || 'step');
        const info = attrsById.get(element);
        const step: StepRequest = { element, name: info?.name || scope?.content?.name || element, type: info?.attrs.type || 'agent', attrs: info?.attrs || {}, outputs: { ...outputs } };
        const label = step.type === 'agent' ? 'agent' : step.type;
        const finish = (err: Error | null, output?: unknown) => {
          if (!err) { outputs[element] = output; emit({ outputs: { [element]: output } }); }
          if (typeof next === 'function') next(err, output);
        };
        if (opts.dryRun || !opts.exec) {
          emit({ log: { at: Date.now(), element, text: `dry run · ${label} · ${step.name}` } });
          finish(null, sampleOutput(step));
          return;
        }
        emit({ log: { at: Date.now(), element, text: `${label} · ${step.name} · started` } });
        opts.exec(step).then((output) => {
          const summary = typeof output === 'object' && output ? String((output as { text?: string; title?: string; stdout?: string }).title || (output as { text?: string }).text || (output as { stdout?: string }).stdout || '').replace(/\s+/g, ' ').slice(0, 160) : String(output ?? '');
          emit({ log: { at: Date.now(), element, text: `${label} · ${step.name} · ${summary || 'done'}` } });
          finish(null, output);
        }, (err: Error) => {
          emit({ failedAt: element, log: { at: Date.now(), element, text: `${label} · ${step.name} · failed: ${err?.message || err}` } });
          finish(err instanceof Error ? err : new Error(String(err)));
        });
      },
    },
  });

  const remember = () => { void engine.getState().then((state) => emit({ state })).catch(() => {}); };

  const fail = (error: string) => {
    void engine.getState().then((state) => emit({ status: 'failed', error, state, waiting: null })).catch(() => emit({ status: 'failed', error, waiting: null }));
  };

  // Where the run is, for the diagram: steps it entered and the paths it took.
  listener.on('activity.start', (api: { id?: string; content?: { id?: string } }) => { const element = textOf(api, 'id'); if (element) emit({ started: element }); });
  listener.on('flow.take', (api: { id?: string; content?: { id?: string } }) => { const flow = textOf(api, 'id'); if (flow) emit({ taken: flow }); });

  listener.on('activity.end', (api: { id?: string; content?: { id?: string; name?: string } }) => {
    const element = textOf(api, 'id');
    if (!element) return;
    emit({ completed: element, log: { at: Date.now(), element, text: 'finished ' + (textOf(api, 'name') || element) } });
    remember();
  });

  listener.on('activity.wait', (api: { id?: string; type?: string; content?: { id?: string; type?: string; name?: string } }) => {
    const type = textOf(api, 'type');
    if (!type.includes('UserTask') && !type.includes('ReceiveTask')) return;
    const element = textOf(api, 'id');
    const name = textOf(api, 'name') || element;
    const waiting: RunWait = { element, name, prompt: name };
    emit({ status: 'waiting', waiting, log: { at: Date.now(), element, text: (opts.dryRun ? 'dry run · ' : 'waiting · ') + name } });
    remember();
    if (opts.dryRun) queueMicrotask(() => engine.execution?.signal({ id: element }));
  });

  engine.on('end', () => {
    void engine.getState().then((state) => emit({ status: 'completed', waiting: null, state })).catch(() => emit({ status: 'completed', waiting: null }));
  });
  engine.on('error', (err: Error) => fail(err?.message || 'Graph failed'));

  const go = (saved: boolean) => {
    const run = saved ? engine.recover(opts.saved).resume() : engine.execute();
    void Promise.resolve(run).catch((err: Error) => fail(err?.message || 'Graph failed'));
  };

  if (!opts.hold) go(!!opts.saved);

  return {
    signal(elementId: string) {
      const execution = engine.execution;
      if (!execution) throw new Error('This run is not waiting');
      emit({ log: { at: Date.now(), element: elementId, text: 'continued' }, waiting: null, status: 'running' });
      execution.signal({ id: elementId });
    },
    async pause() {
      await engine.stop();
      const state = await engine.getState().catch(() => null);
      emit({ status: 'paused', state });
    },
    async resume() {
      emit({ status: 'running' });
      if (opts.saved && !engine.execution) go(true);
      else void Promise.resolve(engine.resume()).catch((err: Error) => fail(err?.message || 'Graph failed'));
    },
    async stop() {
      await engine.stop()?.catch(() => {});
      const state = await engine.getState().catch(() => null);
      emit({ status: 'stopped', state });
      return state;
    },
    state() { return engine.getState().catch(() => null); },
    done,
  };
}
