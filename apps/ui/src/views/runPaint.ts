// Draws a run on a bpmn-js diagram (the run panel's viewer and the editor): finished steps, the step running now,
// one waiting for someone, the one that failed, and the paths the run took.
import type { GraphRun } from '../../../../packages/shared/src/types.ts';

type Any = any;
const MARKERS = ['run-done', 'run-active', 'run-waiting', 'run-failed', 'run-taken', 'run-next'];

export type StepState = 'done' | 'active' | 'waiting' | 'failed' | 'idle';

export function stepState(run: GraphRun | undefined, id: string): StepState {
  if (!run) return 'idle';
  if (run.failedAt === id) return 'failed';
  if (run.waiting?.element === id) return 'waiting';
  if (run.active?.includes(id)) return 'active';
  if (run.completed.includes(id)) return 'done';
  return 'idle';
}

// Badge glyphs as small inline SVGs, in the app's icon style.
const svg = (d: string) => `<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;
const BADGE: Record<Exclude<StepState, 'idle'>, string> = { done: svg('<path d="M20 6 9 17l-5-5"/>'), active: '', waiting: svg('<path d="M9 5v14"/><path d="M15 5v14"/>'), failed: svg('<path d="M18 6 6 18"/><path d="m6 6 12 12"/>') };

export function paintRun(viewer: Any, run: GraphRun | undefined): void {
  if (!viewer) return;
  let canvas: Any, registry: Any, overlays: Any;
  try { canvas = viewer.get('canvas'); registry = viewer.get('elementRegistry'); overlays = viewer.get('overlays'); } catch { return; }
  overlays.remove({ type: 'run' });
  for (const el of registry.getAll()) for (const m of MARKERS) canvas.removeMarker(el, m);
  if (!run) return;
  for (const el of registry.getAll()) {
    if (el.type === 'label' || el.type === 'bpmn:Process') continue;
    if (el.waypoints) { if (run.taken?.includes(el.id)) canvas.addMarker(el, 'run-taken'); continue; }
    const st = stepState(run, el.id);
    if (st === 'idle') continue;
    canvas.addMarker(el, 'run-' + st);
    // Events are small circles: the colour says enough. Tasks and gateways get a badge.
    if (/Event$/.test(el.type)) continue;
    const html = document.createElement('span');
    html.className = 'run-badge run-badge-' + st;
    html.innerHTML = BADGE[st];
    overlays.add(el.id, 'run', { position: { top: -9, right: 9 }, html });
  }
}

/** The tasks of a diagram in reading order (left to right, then top to bottom), for the step list. */
export function diagramSteps(viewer: Any): { id: string; name: string; type: string }[] {
  try {
    return viewer.get('elementRegistry').getAll()
      .filter((el: Any) => !el.waypoints && el.type !== 'label' && /Task$|^bpmn:CallActivity$|^bpmn:SubProcess$/.test(el.type))
      .sort((a: Any, b: Any) => a.x - b.x || a.y - b.y)
      .map((el: Any) => ({ id: el.id, name: el.businessObject?.name || el.id, type: el.businessObject?.$attrs?.['harness:type'] || el.businessObject?.get?.('harness:type') || (el.type === 'bpmn:UserTask' ? 'you' : 'task') }));
  } catch { return []; }
}
