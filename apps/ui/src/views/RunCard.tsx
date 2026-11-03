// A graph run inside the chat it was started from: live progress, a waiting step to continue, and the results.
import { useEffect } from 'react';
import { useStore, setState, act, wsId, go, openRun } from '../store.ts';
import { get, post } from '../api.ts';
import type { GraphRun } from '../../../../packages/shared/src/types.ts';
import { Markdown } from '../ui/Markdown.tsx';
import { openChat } from '../shell/Shell.tsx';
import { Approvals } from './Approvals.tsx';
import { Icon } from '../ui/Icon.tsx';

let loaded = '';
/** Runs come live over events; a chat opened later loads them once. */
function useRuns(): void {
  useEffect(() => {
    if (loaded === wsId()) return;
    loaded = wsId();
    void get<{ runs: GraphRun[] }>(`/api/ws/${wsId()}/runs`).then((r) => setState((s) => ({ runs: [...s.runs, ...r.runs.filter((x) => !s.runs.some((y) => y.id === x.id))] })), () => { loaded = ''; });
  }, []);
}

const TONE: Record<string, string> = { completed: 'var(--success-text)', failed: 'var(--error-text)', stopped: 'var(--muted)', waiting: 'var(--warning-text)', paused: 'var(--muted)', running: 'var(--accent-p-text)' };

export function RunCard({ runId, text }: { runId: string; text: string }) {
  useRuns();
  const run = useStore((s) => s.runs.find((r) => r.id === runId));
  const graph = useStore((s) => s.snap?.graphs.find((g) => g.id === run?.graph));
  const shown = useStore((s) => s.runId === runId);
  const runChat = useStore((s) => (run?.chat ? s.snap?.chats.find((c) => c.id === run.chat) : undefined));
  const status = run?.status || (/completed/.test(text) ? 'completed' : /failed/.test(text) ? 'failed' : 'running');
  const results = text.includes('\n\nStep results:') ? text.slice(text.indexOf('\n\nStep results:') + 2) : '';
  const last = run?.log[run.log.length - 1];
  return (
    <div className="card run-card" role="group" aria-label={`Graph run · ${graph?.name || ''}`}>
      <span className="row" style={{ gap: 8 }}>
        <span aria-hidden style={{ color: TONE[status], display: 'flex' }} className={status === 'running' ? 'pulse' : undefined}><Icon name="workflow" size={16} /></span>
        <b className="grow ellipsis" style={{ fontSize: 13 }}>{graph?.name || text.split('\n')[0].replace(/^Running graph |^Graph /, '')}</b>
        <span className="mono" style={{ fontSize: 11, color: TONE[status] }}>{run?.dryRun ? 'dry run · ' : ''}{status}</span>
      </span>
      {status === 'running' && last && <span className="muted ellipsis" style={{ fontSize: 12 }}>{last.text}</span>}
      {run?.status === 'waiting' && run.waiting && <span className="row" style={{ gap: 8, flexWrap: 'wrap' }}>
        <span className="grow" style={{ fontSize: 12.5 }}>Waiting for you at <b>{run.waiting.name || run.waiting.element}</b>{run.waiting.prompt && run.waiting.prompt !== run.waiting.name ? ` — ${run.waiting.prompt}` : ''}</span>
        <button type="button" className="btn btn-p" style={{ height: 28 }} onClick={() => void act(() => post(`/api/ws/${wsId()}/runs/${run.id}/signal`, { element: run.waiting!.element }), 'Continuing the run')}>Continue</button>
      </span>}
      {!!run?.chats?.length && (run.status === 'running' || run.status === 'paused') && <Approvals chatIds={run.chats} compact />}
      {results && <details><summary className="muted" style={{ cursor: 'pointer', fontSize: 12 }}>Results</summary><div style={{ fontSize: 12.5, marginTop: 6 }}><Markdown text={results} /></div></details>}
      {status === 'failed' && !results && <span style={{ color: 'var(--error-text)', fontSize: 12 }}>{last?.text || text}</span>}
      <span className="row" style={{ gap: 6 }}>
        {run && <button type="button" className="chip" style={{ height: 24, fontSize: 11 }} onClick={() => openRun(run.id)} aria-pressed={shown}><Icon name="panel" size={12} /> Show run</button>}
        {runChat && <button type="button" className="chip" style={{ height: 24, fontSize: 11 }} onClick={() => openChat(runChat)}>Open the run’s chat</button>}
        {graph && <button type="button" className="chip" style={{ height: 24, fontSize: 11 }} onClick={() => go('graph', { graphId: graph.id })}>Open graph</button>}
        {graph && <span className="muted mono ellipsis" style={{ fontSize: 10.5, marginLeft: 'auto', minWidth: 0 }} title={graph.file}>{graph.file.split('/').slice(-2).join('/')}</span>}
      </span>
    </div>
  );
}
