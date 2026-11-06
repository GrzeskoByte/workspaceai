#!/usr/bin/env node
// Artifact tools for every agent (stdio MCP). Open Design saves each artifact, and artifact_guide hands the agent
// Open Design's design system, craft rules and skills to follow (R-04). Calls back into the supervisor,
// which places the artifact in the project of the chat that called the tool (R-13).
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';

const SUP = process.env.HARNESS_SUPERVISOR;
const TOKEN = process.env.HARNESS_SUPERVISOR_TOKEN;
const WS = process.env.HARNESS_WS;

async function call(method: string, path: string, body?: unknown): Promise<any> {
  if (!SUP || !TOKEN || !WS) throw new Error('WorkspaceAI supervisor is not reachable');
  const r = await fetch(`${SUP}/api/ws/${WS}${path}`, { method, headers: { 'x-harness-token': TOKEN, 'content-type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || 'request failed ' + r.status);
  return j;
}

const text = (t: string) => ({ content: [{ type: 'text' as const, text: t }] });
const server = new McpServer({ name: 'harness-artifacts', version: '0.1.0' });

const FORMAT_HELP = 'Set format: doc = "markdown" or "html" (a designed page); deck = "html" on Open Design’s deck framework (preferred) or "markdown" slides separated by a line with only ---; prototype = "html", one complete self-contained file (inline CSS/JS, no external network); sheet = "csv" with a header row, or "html"; design-system = "markdown", a DESIGN.md the workspace can then pick as its design system. Content is the complete document.';
const format = z.enum(['markdown', 'html', 'csv']).optional().describe('What content is written in; see the tool description');

server.registerTool('artifact_guide', {
  description: 'Read Open Design’s reference before making or revising an artifact: the workspace design system (DESIGN.md — authoritative colours, type, spacing, components), Open Design’s craft rules, its skills and, for decks, its deck framework. Call it once per chat for each artifact type, before artifact_create or artifact_update, and follow it.',
  inputSchema: { type: z.enum(['doc', 'deck', 'prototype', 'sheet', 'design-system']), skill: z.string().optional().describe('An Open Design skill id from the list this tool returns, to load its workflow, seed template and example') },
}, async (a) => text((await call('POST', '/agent/guide', a)).guide));

server.registerTool('artifact_create', {
  description: 'Create a document, slide deck, HTML prototype, sheet or design system as a first-class artifact, saved by Open Design and opened beside the chat. Call artifact_guide for the type first and follow its design system. ' + FORMAT_HELP,
  inputSchema: { type: z.enum(['doc', 'deck', 'prototype', 'sheet', 'design-system']), title: z.string(), format, content: z.string(), note: z.string().optional() },
}, async (a) => {
  const r = await call('POST', '/agent/artifacts', a);
  return text(`Created ${a.type} “${r.title}” (id ${r.id}, v${r.version}, ${r.versions?.[0]?.format || a.format || 'markdown'}) in ${r.projectPath}. Open Design saved it and it is open beside the chat.`);
});

server.registerTool('artifact_update', {
  description: 'Save a new version of an existing artifact (every save is a version). ' + FORMAT_HELP,
  inputSchema: { id: z.string(), format, content: z.string(), note: z.string().optional(), title: z.string().optional() },
}, async (a) => {
  const r = await call('POST', `/agent/artifacts/${encodeURIComponent(a.id)}`, a);
  return text(`Saved “${r.title}” v${r.version}.`);
});

server.registerTool('artifact_read', {
  description: 'Read the latest version of an artifact.',
  inputSchema: { id: z.string() },
}, async ({ id }) => { const r = await call('GET', `/artifacts/${encodeURIComponent(id)}/content`); return text(`# ${r.artifact.title} (${r.artifact.type}, v${r.version}, ${r.format})\n\n${r.content}`); });

server.registerTool('artifact_list', {
  description: 'List artifacts in this workspace (id, type, title, project).',
  inputSchema: {},
}, async () => { const r = await call('GET', '/snapshot'); return text((r.artifacts as any[]).map((a) => `- ${a.id} · ${a.type} · ${a.title} · v${a.version}`).join('\n') || 'No artifacts yet.'); });

// ---------- graphs: the project's BPMN workflows, as files in the project folder ----------
const enc = (s: string) => encodeURIComponent(s);
const runText = (r: any) => `${r.summary}${r.run ? `\n\n(run ${r.run}, status ${r.status})` : ''}`;

server.registerTool('graph_list', {
  description: 'List the BPMN graphs (runnable workflows) in this workspace: .bpmn files in project folders and graphs made in the app. Shows each graph’s id, name, file and steps; graphs of the current project come first.',
  inputSchema: {},
}, async () => {
  const r = await call('GET', '/agent/graphs');
  return text((r.graphs as any[]).map((g) => `- ${g.id} · ${g.name}${g.here ? ' (this project)' : ' · ' + g.project} · ${g.file}${g.steps.length ? '\n  steps: ' + g.steps.join(' → ') : ''}`).join('\n') || 'No graphs yet. Create one with graph_save, or add a .bpmn file to the project folder.');
});

server.registerTool('graph_read', {
  description: 'Read a graph’s BPMN XML, by id, name or file path.',
  inputSchema: { graph: z.string() },
}, async ({ graph }) => { const r = await call('GET', `/agent/graphs/${enc(graph)}`); return text(`# ${r.name} (${r.id}) — ${r.file}\n\n${r.xml}`); });

server.registerTool('graph_save', {
  description: 'Create a graph in the current project (saved as graphs/<name>.bpmn in its folder), or save a new version of one by passing `graph`. The XML must be BPMN 2.0 with xmlns:harness="http://harness.local/schema/bpmn"; steps are tasks with harness:type = agent (harness:prompt; optional harness:runner="acp:claude" to run in Claude Code), browser (harness:url), script (harness:command, runs in the project folder), service (harness:mcp tool) or rule (harness:prompt, judges the work); bpmn:userTask waits for the user. Include the bpmndi diagram so it opens in the editor. It must pass the model check.',
  inputSchema: { xml: z.string(), name: z.string().optional(), graph: z.string().optional().describe('An existing graph to update (id, name or file)') },
}, async (a) => { const r = await call('POST', '/agent/graphs', a); return text(`${r.created ? 'Created' : 'Saved'} graph “${r.name}” (${r.id}) at ${r.file}.`); });

server.registerTool('graph_run', {
  description: 'Run a graph (by id, name or file) for this chat. The run shows in the chat; by default this waits for it and returns each step’s result. A run that reaches a user task waits for the user — say so and stop. Use dryRun to walk the path without doing anything.',
  inputSchema: { graph: z.string(), dryRun: z.boolean().optional(), wait: z.boolean().optional().describe('Default true'), timeout: z.number().optional().describe('Seconds to wait, default 280') },
}, async (a) => text(runText(await call('POST', `/agent/graphs/${enc(a.graph)}/run`, { dryRun: a.dryRun, wait: a.wait, timeout: a.timeout }))));

server.registerTool('graph_status', {
  description: 'How a graph run is going, and its results once done. Optionally wait up to `wait` seconds for it to finish.',
  inputSchema: { run: z.string(), wait: z.number().optional() },
}, async (a) => text(runText(await call('GET', `/agent/runs/${enc(a.run)}${a.wait ? '?wait=' + a.wait : ''}`))));

// ---------- the project's mind map ----------
const mapOps = async (ops: unknown[], project?: string) => { const r = await call('POST', '/agent/map/ops', { ops, project }); return r; };

server.registerTool('map_read', {
  description: 'Read the project’s mind map as an outline: its chats, artifacts, graphs, runs, memory and routines, plus the ideas, tasks and notes added to it, each with the node id to use in the other map tools. Use it to see how the project’s work fits together before planning.',
  inputSchema: {},
}, async () => text((await call('GET', '/agent/map')).outline));

server.registerTool('map_add', {
  description: 'Add to the project’s mind map under a node (an id from map_read; the map’s centre for a new branch). Give `text` for one idea, task or note, or `markdown` for a whole plan as a nested bullet list, where "- [ ] step" makes a task. It appears on the map straight away.',
  inputSchema: { parent: z.string(), text: z.string().optional(), kind: z.enum(['idea', 'task', 'note']).optional(), markdown: z.string().optional() },
}, async (a) => {
  if (!a.text && !a.markdown) throw new Error('Give text or markdown');
  const r = await mapOps([a.markdown ? { op: 'import', parent: a.parent, markdown: a.markdown } : { op: 'add', parent: a.parent, text: a.text, kind: a.kind || 'idea' }]);
  return text(`Added ${r.added.length} node${r.added.length === 1 ? '' : 's'} (${r.added.join(', ')}).\n\n${r.outline}`);
});

server.registerTool('map_update', {
  description: 'Change a node on the project’s mind map. Text and status apply to ideas, tasks and notes made on the map (status: todo, doing, done — mark tasks as you finish them). `parent` moves any node, including chats and artifacts, to another branch; a node can’t move under itself.',
  inputSchema: { id: z.string(), text: z.string().optional(), status: z.enum(['todo', 'doing', 'done']).optional(), parent: z.string().optional() },
}, async (a) => {
  const ops: unknown[] = [];
  if (a.text !== undefined || a.status) ops.push({ op: 'update', id: a.id, ...(a.text !== undefined ? { text: a.text } : {}), ...(a.status ? { status: a.status } : {}) });
  if (a.parent) ops.push({ op: 'move', id: a.id, parent: a.parent });
  if (!ops.length) throw new Error('Nothing to change');
  await mapOps(ops);
  return text('Updated the map.');
});

server.registerTool('map_link', {
  description: 'Draw a link between two nodes on different branches of the project’s mind map, for example a task and the artifact it produces.',
  inputSchema: { from: z.string(), to: z.string(), label: z.string().optional() },
}, async (a) => { await mapOps([{ op: 'link', ...a }]); return text('Linked.'); });

server.registerTool('map_remove', {
  description: 'Remove an idea, task or note made on the project’s mind map. What hung off it moves up a level. Chats, artifacts and other items can’t be removed from here.',
  inputSchema: { id: z.string() },
}, async ({ id }) => { await mapOps([{ op: 'remove', id }]); return text('Removed.'); });

await server.connect(new StdioServerTransport());
