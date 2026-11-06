// A graph run beside the chat: the diagram with where the run is (finished, running, waiting, failed, the paths
// taken), every step with its result, and the controls to continue, pause or resume it.
import { useEffect, useRef, useState } from 'react';
import NavigatedViewer from 'bpmn-js/lib/NavigatedViewer';
import 'bpmn-js/dist/assets/diagram-js.css';
import 'bpmn-js/dist/assets/bpmn-js.css';
import 'bpmn-js/dist/assets/bpmn-font/css/bpmn-embedded.css';
import { useStore, setState, act, wsId, go } from '../store.ts';
import { get, post } from '../api.ts';
import type { GraphMeta, GraphRun } from '../../../../packages/shared/src/types.ts';
import { HARNESS_MODDLE } from '../../../../packages/orchestrator/src/model.ts';
import { Markdown } from '../ui/Markdown.tsx';
import { openChat } from '../shell/Shell.tsx';
import { diagramSteps, paintRun, stepState, type StepState } from './runPaint.ts';
import { Approvals } from './Approvals.tsx';
import { Icon } from '../ui/Icon.tsx';

type Any = any;
const ICON: Record<StepState, string> = { done: 'check', active: 'dot', waiting: 'pause', failed: 'x', idle: 'circle' };
const TONE: Record<string, string> = { completed: 'var(--success-text)', failed: 'var(--error-text)', stopped: 'var(--muted)', waiting: 'var(--warning-text)', paused: 'var(--muted)', running: 'var(--accent-p-text)', done: 'var(--success-text)', active: 'var(--accent-p-text)', idle: 'var(--dim)' };

function outputText(o: unknown): string {
  if (o == null) return '';
  if (typeof o !== 'object') return String(o);
  const x = o as { text?: string; stdout?: string; title?: string; verdict?: string; reason?: string };
  if (x.verdict) return `**${x.verdict}**${x.reason ? ' — ' + x.reason : ''}`;
  return String(x.text ?? x.stdout ?? x.title ?? JSON.stringify(o, null, 2));
}

export function RunPanel() {
  const runId = useStore((s) => s.runId)!;
  const run = useStore((s) => s.runs.find((r) => r.id === runId));
  const graph = useStore((s) => s.snap?.graphs.find((g) => g.id === run?.graph));
  const runChat = useStore((s) => (run?.chat ? s.snap?.chats.find((c) => c.id === run.chat) : undefined));
  const wide = useStore((s) => s.runWide);
  const host = useRef<HTMLDivElement>(null);
  const viewer = useRef<Any>(null);
  const [steps, setSteps] = useState<{ id: string; name: string; type: string }[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [loadedGraph, setLoadedGraph] = useState('');

  // The run may have started before this window knew of it.
  useEffect(() => {
    if (run) return;
    void get<{ runs: GraphRun[] }>(`/api/ws/${wsId()}/runs`).then((r) => setState((s) => ({ runs: [...r.runs.filter((x) => !s.runs.some((y) => y.id === x.id)), ...s.runs] })), () => {});
  }, [runId, !!run]);

  useEffect(() => {
    if (!host.current) return;
    const v: Any = new (NavigatedViewer as Any)({ container: host.current, moddleExtensions: { harness: HARNESS_MODDLE } });
    viewer.current = v;
    v.on('element.click', (e: Any) => { if (!e.element?.waypoints && /Task$|Gateway$|Event$/.test(e.element?.type || '')) setOpen(e.element.id); });
    return () => { v.destroy(); viewer.current = null; };
  }, []);

  useEffect(() => {
    const gid = run?.graph;
    if (!gid || gid === loadedGraph || !viewer.current) return;
    let stop = false;
    get<{ meta: GraphMeta; xml: string }>(`/api/ws/${wsId()}/graphs/${gid}`).then(async (r) => {
      if (stop || !viewer.current) return;
      await viewer.current.importXML(r.xml);
      viewer.current.get('canvas').zoom('fit-viewport', 'auto');
      setSteps(diagramSteps(viewer.current));
      setLoadedGraph(gid);
    }).catch(() => {});
    return () => { stop = true; };
  }, [run?.graph]);

  // Repaint whenever the run moves.
  useEffect(() => { if (loadedGraph) paintRun(viewer.current, run); }, [loadedGraph, run]);
  // Follow the step that is running or waiting.
  const current = run?.waiting?.element || run?.active?.[run.active.length - 1] || run?.failedAt || '';
  useEffect(() => { if (current && run?.status !== 'completed') setOpen(current); }, [current]);

  const base = `/api/ws/${wsId()}/runs/${runId}`;
  const status = run?.status || 'loading';
  const done = steps.filter((s) => run?.completed.includes(s.id)).length;
  const btn = { height: 28, width: 28, padding: 0, justifyContent: 'center' } as const;
  return (
    <section aria-label="Graph run" className="rise art-panel run-panel" style={{ flex: wide ? '1 1 auto' : '0 0 var(--right-w, 560px)' }}>
      <header className="row" style={{ gap: 8, padding: '12px 14px', borderBottom: '1px solid var(--bg2)' }}>
        <span aria-hidden className={status === 'running' ? 'pulse' : undefined} style={{ color: TONE[status], display: 'flex' }}><Icon name="workflow" size={18} /></span>
        <span className="grow" style={{ display: 'flex', flexDirection: 'column', minWidth: 0 }}>
          <b className="ellipsis" style={{ fontSize: 14 }}>{graph?.name || 'Graph run'}</b>
          <span className="mono" style={{ fontSize: 11, color: TONE[status] }}>{run?.dryRun ? 'dry run · ' : ''}{status}{steps.length ? ` · ${done}/${steps.length} steps` : ''}{run ? ' · ' + new Date(run.startedAt).toLocaleTimeString() : ''}</span>
        </span>
        {(status === 'running' || status === 'waiting') && <button type="button" className="btn" style={{ height: 28 }} onClick={() => void act(() => post(`${base}/pause`), 'Paused · state saved')}>Pause</button>}
        {status === 'paused' && <button type="button" className="btn" style={{ height: 28 }} onClick={() => void act(() => post(`${base}/resume`), 'Resumed')}>Resume</button>}
        <button type="button" className="btn wide-toggle" style={btn} onClick={() => setState({ runWide: !wide })} aria-pressed={wide} aria-label={wide ? 'Show the chat beside the run' : 'Expand the run'}><Icon name={wide ? 'collapse' : 'expand'} size={15} /></button>
        <button type="button" className="btn" style={btn} onClick={() => setState({ runId: null, runWide: false })} aria-label="Close run"><Icon name="x" size={15} /></button>
      </header>

      {run?.waiting && <div className="row run-wait">
        <span className="grow" style={{ fontSize: 13 }}>Waiting for you at <b>{run.waiting.name || run.waiting.element}</b>{run.waiting.prompt && run.waiting.prompt !== run.waiting.name ? ` — ${run.waiting.prompt}` : ''}</span>
        <button type="button" className="btn btn-p" style={{ height: 30 }} onClick={() => void act(() => post(`${base}/signal`, { element: run.waiting!.element }), 'Continuing the run')}>Continue</button>
      </div>}

      {!!run?.chats?.length && <Approvals chatIds={run.chats} />}

      <div className="run-diagram" style={{ height: wide ? '52vh' : 300 }}>
        <div ref={host} style={{ position: 'absolute', inset: 0 }} />
        <span className="run-legend">
          {(['done', 'active', 'waiting', 'failed'] as StepState[]).map((k) => <span key={k}><i style={{ background: TONE[k === 'waiting' ? 'waiting' : k === 'failed' ? 'failed' : k] }} />{k === 'active' ? 'running' : k}</span>)}
        </span>
      </div>

      <div className="run-steps">
        {steps.map((s) => {
          const st = stepState(run, s.id);
          const out = outputText(run?.outputs?.[s.id]);
          const lines = (run?.log || []).filter((l) => l.element === s.id);
          const isOpen = open === s.id;
          return (
            <div key={s.id} className={'run-step run-step-' + st} aria-current={isOpen ? 'step' : undefined}>
              <button type="button" className="row navrow" onClick={() => setOpen(isOpen ? null : s.id)} style={{ gap: 10, width: '100%' }}>
                <span className={'run-ico' + (st === 'active' ? ' pulse' : '')} style={{ color: TONE[st === 'waiting' ? 'waiting' : st === 'failed' ? 'failed' : st], display: 'inline-flex' }}><Icon name={ICON[st]} size={st === 'idle' ? 12 : 14} stroke={2.25} /></span>
                <span className="grow ellipsis" style={{ fontSize: 13, textAlign: 'left' }}>{s.name}</span>
                <span className="muted mono" style={{ fontSize: 10.5 }}>{s.type}</span>
              </button>
              {isOpen && <div className="run-out rise">
                {out ? <Markdown text={out.length > 6000 ? out.slice(0, 6000) + '\n\n…' : out} /> : <span className="muted" style={{ fontSize: 12 }}>{st === 'active' ? 'Running…' : st === 'idle' ? 'Not reached yet.' : 'No output.'}</span>}
                {!!lines.length && <pre className="pre" style={{ marginTop: 6, maxHeight: 120, fontSize: 11 }}>{lines.map((l) => new Date(l.at).toLocaleTimeString() + '  ' + l.text).join('\n')}</pre>}
              </div>}
            </div>
          );
        })}
        {status === 'failed' && !run?.failedAt && <span style={{ color: 'var(--error-text)', fontSize: 12.5 }}>{run?.log[run.log.length - 1]?.text}</span>}
      </div>

      <div className="row" style={{ gap: 6, padding: '10px 14px', borderTop: '1px solid var(--bg2)', flexWrap: 'wrap' }}>
        {graph && <button type="button" className="chip" style={{ height: 24, fontSize: 11 }} onClick={() => go('graph', { graphId: graph.id })}>Open in the editor</button>}
        {runChat && <button type="button" className="chip" style={{ height: 24, fontSize: 11 }} onClick={() => openChat(runChat)}>Open the run’s chat</button>}
        {graph && <span className="muted mono ellipsis" style={{ fontSize: 10.5, marginLeft: 'auto', minWidth: 0 }} title={graph.file}>{graph.file.split('/').slice(-2).join('/')}</span>}
      </div>
    </section>
  );
}
