// bpmn-js additions: a harness palette group (agent task, Browser step, script, business rule) on top of the
// standard BPMN elements, so a graph can be built by drag and drop without typing code (R-24, R-37).
const HARNESS_TASKS = [
  { type: 'agent', title: 'Agent task — one opencode prompt', icon: 'bpmn-icon-service-task', name: 'Ask the agent', attrs: { 'harness:agent': 'opencode', 'harness:prompt': '' } },
  { type: 'browser', title: 'Browser step — open a page with Playwright MCP', icon: 'bpmn-icon-send-task', name: 'Read a page', attrs: { 'harness:url': 'https://' } },
  { type: 'script', title: 'Script task — a shell command in the project folder', icon: 'bpmn-icon-script-task', name: 'Run a command', attrs: { 'harness:command': '' } },
  { type: 'rule', title: 'Business rule — judge against a rubric, returns pass/fail', icon: 'bpmn-icon-business-rule-task', name: 'Check the result', attrs: { 'harness:prompt': 'Does the draft meet the brief?' } },
] as const;

type Any = any; // bpmn-js services are untyped JavaScript.

function HarnessPalette(this: Any, palette: Any, create: Any, elementFactory: Any, bpmnFactory: Any) {
  this.getPaletteEntries = () => {
    const entries: Record<string, unknown> = { 'harness-separator': { group: 'harness', separator: true } };
    for (const t of HARNESS_TASKS) {
      const start = (event: Event) => {
        const bo = bpmnFactory.create('bpmn:ServiceTask', { name: t.name, implementation: '${environment.services.agent}' });
        bo.set('harness:type', t.type);
        for (const [k, v] of Object.entries(t.attrs)) bo.set(k, v);
        create.start(event, elementFactory.createShape({ type: 'bpmn:ServiceTask', businessObject: bo }));
      };
      entries['harness-' + t.type] = { group: 'harness', className: t.icon + ' harness-entry harness-' + t.type, title: t.title, action: { dragstart: start, click: start } };
    }
    return entries;
  };
  palette.registerProvider(this);
}
(HarnessPalette as Any).$inject = ['palette', 'create', 'elementFactory', 'bpmnFactory'];

export const harnessPaletteModule = { __init__: ['harnessPalette'], harnessPalette: ['type', HarnessPalette] };
export const HARNESS_TASK_TYPES = HARNESS_TASKS.map((t) => t.type);
