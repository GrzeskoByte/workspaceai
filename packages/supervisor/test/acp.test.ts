// R-39: a chat on an ACP agent streams replies, asks before writing files, keeps file and terminal access inside the
// project folder, and gets the same harness MCP servers (memory, artifacts) an opencode chat gets.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-acp-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'none');
process.env.LEAKY_SECRET = 'should-not-pass';

// A scripted ACP agent: streams a chunk, asks to write a file, then probes file and terminal limits.
const agent = join(home, 'fake-acp.cjs');
const record = join(home, 'acp-record.json');
writeFileSync(agent, `
const rl = require('readline').createInterface({ input: process.stdin });
let id = 1000; const waits = new Map(); const rec = { env: { LEAKY_SECRET: process.env.LEAKY_SECRET || null } };
const send = (m) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...m }) + '\\n');
const call = (method, params) => new Promise((r) => { const i = id++; waits.set(i, r); send({ id: i, method, params }); });
const save = () => require('fs').writeFileSync(${JSON.stringify(record)}, JSON.stringify(rec));
rl.on('line', async (line) => {
  const m = JSON.parse(line);
  if (m.id !== undefined && waits.has(m.id)) { waits.get(m.id)(m); waits.delete(m.id); return; }
  if (m.method === 'initialize') { rec.caps = m.params.clientCapabilities; send({ id: m.id, result: { protocolVersion: 1, agentCapabilities: {} } }); }
  else if (m.method === 'session/new') { rec.cwd = m.params.cwd; rec.mcp = m.params.mcpServers; rec.meta = m.params._meta; save(); send({ id: m.id, result: { sessionId: 'S1', models: { availableModels: [{ modelId: 'fast', name: 'Fast', description: 'quick answers' }, { modelId: 'deep', name: 'Deep', _meta: { reasoningEfforts: [{ value: 'low' }, { value: 'high', default: true }] } }], currentModelId: 'fast' }, configOptions: [{ id: 'reasoning_effort', category: 'thought_level', type: 'select', currentValue: 'high' }] } });
    send({ method: 'session/update', params: { sessionId: 'S1', update: { sessionUpdate: 'available_commands_update', availableCommands: [{ name: 'whoami', description: 'Who is signed in', input: { hint: '[detail]' } }] } } }); }
  else if (m.method === 'session/set_model') { rec.setModel = m.params; save(); send({ id: m.id, result: {} }); }
  else if (m.method === 'session/set_config_option') { rec.setEffort = m.params; save(); send({ id: m.id, result: {} }); }
  else if (m.method === 'session/prompt') {
    const sid = m.params.sessionId;
    rec.prompts = [...(rec.prompts || []), m.params.prompt[0].text]; save();
    if (m.params.prompt[0].text === '/context' && process.env.CLAUDE_CONFIG_DIR) {
      // Like Claude Code: a local command's output goes to the session transcript, and the adapter sends no text.
      const dir = require('path').join(process.env.CLAUDE_CONFIG_DIR, 'projects', '-work');
      require('fs').mkdirSync(dir, { recursive: true });
      require('fs').writeFileSync(require('path').join(dir, sid + '.jsonl'), JSON.stringify({ type: 'system', subtype: 'local_command', timestamp: new Date().toISOString(), content: '<local-command-stdout>## Context Usage\\n10.8k / 1m</local-command-stdout>' }) + '\\n');
      send({ id: m.id, result: { stopReason: 'end_turn' } }); return;
    }
    // Like Claude Code when the workspace's copy of its login has expired.
    if (m.params.prompt[0].text.startsWith('signed-out')) { send({ id: m.id, error: { code: -32603, message: 'Internal error: Failed to authenticate: OAuth session expired and could not be refreshed' } }); return; }
    const upd = (u) => send({ method: 'session/update', params: { sessionId: sid, update: u } });
    upd({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Writing notes. ' } });
    upd({ sessionUpdate: 'tool_call', toolCallId: 't1', title: 'Write notes.md', kind: 'edit', status: 'pending' });
    const perm = await call('session/request_permission', { sessionId: sid, toolCall: { toolCallId: 't1', title: 'Write notes.md', rawInput: { path: 'notes.md' } }, options: [{ optionId: 'ok', name: 'Allow once', kind: 'allow_once' }, { optionId: 'no', name: 'Reject', kind: 'reject_once' }] });
    rec.permission = perm.result.outcome;
    if (perm.result.outcome.optionId === 'ok') rec.write = await call('fs/write_text_file', { sessionId: sid, path: rec.cwd + '/notes.md', content: 'wall 8 mm' });
    rec.outside = await call('fs/write_text_file', { sessionId: sid, path: '/tmp/harness-acp-escape.txt', content: 'x' });
    const t = await call('terminal/create', { sessionId: sid, command: 'pwd', args: [] });
    rec.exit = await call('terminal/wait_for_exit', { sessionId: sid, terminalId: t.result.terminalId });
    rec.termOut = await call('terminal/output', { sessionId: sid, terminalId: t.result.terminalId });
    await call('terminal/release', { sessionId: sid, terminalId: t.result.terminalId });
    rec.termOutside = await call('terminal/create', { sessionId: sid, command: 'ls', args: [], cwd: '/' });
    save();
    upd({ sessionUpdate: 'tool_call_update', toolCallId: 't1', status: 'completed' });
    upd({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Done.' } });
    send({ id: m.id, result: { stopReason: 'end_turn' } });
  } else if (m.id !== undefined) send({ id: m.id, result: {} });
});
`);

let base = '', token = '', sup: any, close = async () => {};
const ok = async (m: string, p: string, b?: unknown) => {
  const r = await fetch(base + p, { method: m, headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
  const t = await r.text(); if (r.status >= 300) throw new Error(`${m} ${p} → ${r.status} ${t}`); return t ? JSON.parse(t) : {};
};
const until = async (fn: () => Promise<boolean> | boolean, ms: number) => { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return true; await new Promise((r) => setTimeout(r, 150)); } return false; };

beforeAll(async () => {
  const { Supervisor } = await import('../src/supervisor.ts');
  const { startServer } = await import('../src/server.ts');
  const { buildRouter } = await import('../src/routes.ts');
  const { installExtensions } = await import('../src/extensions.ts');
  sup = new Supervisor();
  const router = buildRouter();
  const server = await startServer(sup, router, {});
  installExtensions(sup, router, server);
  base = `http://127.0.0.1:${server.port}`; token = server.token;
  close = async () => { await sup.shutdown(); await server.close(); };
});
afterAll(async () => { await close(); });

describe('ACP agents (R-39)', () => {
  it('streams, asks before writing, and stays inside the project folder', async () => {
    const W = (await ok('POST', '/api/workspaces', { name: 'A', kind: 'personal' })).id;
    const P = (await ok('POST', `/api/ws/${W}/projects`, { name: 'Castform', parent: null })).id;
    const rt = sup.rt(W);
    await ok('PATCH', `/api/ws/${W}`, { agents: [...rt.ws.agents, { id: 'fake', name: 'Fake agent', via: 'ACP', command: `${process.execPath} ${agent}`, on: true }] });
    const chat = await ok('POST', `/api/ws/${W}/chats`, { project: P, title: 'ACP chat', agent: 'fake' });
    const folder = rt.project(P).folder;
    const events: any[] = [];
    const off = sup.on((e: any) => { if (e.chat === chat.id) events.push(e); });

    const r = JSON.parse(readFileSync(record, 'utf8'));
    expect(r.caps).toMatchObject({ fs: { readTextFile: true, writeTextFile: true }, terminal: true });
    expect(r.cwd).toBe(folder);
    const names = r.mcp.map((m: any) => m.name);
    expect(names).toEqual(expect.arrayContaining(['harness-memory', 'harness-artifacts']));
    const mem = r.mcp.find((m: any) => m.name === 'harness-memory');
    expect(mem.env.find((e: any) => e.name === 'HARNESS_SUPERVISOR').value).toBe(base);
    expect(mem.env.find((e: any) => e.name === 'HARNESS_SUPERVISOR_TOKEN').value).toBe(token);
    expect(r.env.LEAKY_SECRET).toBeNull();

    await ok('POST', `/api/ws/${W}/chats/${chat.id}/send`, { text: 'Note the wall thickness' });
    expect(await until(() => events.some((e) => e.type === 'chat.permission'), 10000)).toBe(true);
    expect(events.some((e) => e.type === 'chat.delta' && /Writing notes/.test(e.delta))).toBe(true);
    expect(existsSync(join(folder, 'notes.md'))).toBe(false);
    const req = events.find((e) => e.type === 'chat.permission').request;
    expect(req.title).toBe('Write notes.md');
    await ok('POST', `/api/ws/${W}/chats/${chat.id}/permission/${req.id}`, { decision: 'once' });
    expect(await until(() => events.some((e) => e.type === 'chat.done'), 10000)).toBe(true);
    off();

    const done = JSON.parse(readFileSync(record, 'utf8'));
    expect(done.permission).toEqual({ outcome: 'selected', optionId: 'ok' });
    expect(readFileSync(join(folder, 'notes.md'), 'utf8')).toBe('wall 8 mm');
    expect(done.outside.error.message).toMatch(/limited to this project folder/);
    expect(existsSync('/tmp/harness-acp-escape.txt')).toBe(false);
    expect(done.exit.result.exitCode).toBe(0);
    expect(done.termOut.result.output.trim()).toBe(folder);
    expect(done.termOutside.error.message).toMatch(/limited to this project folder/);
    const msgs = (await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`)).messages;
    expect(msgs.at(-1).text).toBe('Writing notes. Done.');

    // With "Ask before file writes and commands" off, the same request is answered without a card.
    await ok('PATCH', `/api/ws/${W}`, { acpAskFirst: false });
    const seen: any[] = [];
    const off2 = sup.on((e: any) => { if (e.chat === chat.id) seen.push(e); });
    await ok('POST', `/api/ws/${W}/chats/${chat.id}/send`, { text: 'Again' });
    expect(await until(() => seen.some((e) => e.type === 'chat.done'), 10000)).toBe(true);
    off2();
    expect(seen.some((e) => e.type === 'chat.permission')).toBe(false);
    expect(JSON.parse(readFileSync(record, 'utf8')).permission).toEqual({ outcome: 'selected', optionId: 'ok' });

    // A graph agent task with harness:runner="acp:fake" runs in that agent and returns its reply.
    const G = (await ok('POST', `/api/ws/${W}/graphs`, { project: P, name: 'On ACP' })).id;
    await ok('PUT', `/api/ws/${W}/graphs/${G}`, { xml: `<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI" xmlns:dc="http://www.omg.org/spec/DD/20100524/DC" xmlns:di="http://www.omg.org/spec/DD/20100524/DI" xmlns:harness="http://harness.local/schema/bpmn" id="D" targetNamespace="http://bpmn.io/schema/bpmn">
<bpmn:process id="Pr" name="On ACP" isExecutable="true"><bpmn:startEvent id="S" name="Start" /><bpmn:serviceTask id="A" name="Take notes" implementation="\${environment.services.agent}" harness:type="agent" harness:runner="acp:fake" harness:prompt="Take notes" /><bpmn:endEvent id="E" name="End" />
<bpmn:sequenceFlow id="F1" sourceRef="S" targetRef="A" /><bpmn:sequenceFlow id="F2" sourceRef="A" targetRef="E" /></bpmn:process>
<bpmndi:BPMNDiagram id="DI"><bpmndi:BPMNPlane id="PL" bpmnElement="Pr"><bpmndi:BPMNShape id="S_di" bpmnElement="S"><dc:Bounds x="60" y="100" width="36" height="36" /></bpmndi:BPMNShape><bpmndi:BPMNShape id="A_di" bpmnElement="A"><dc:Bounds x="140" y="78" width="100" height="80" /></bpmndi:BPMNShape><bpmndi:BPMNShape id="E_di" bpmnElement="E"><dc:Bounds x="300" y="100" width="36" height="36" /></bpmndi:BPMNShape>
<bpmndi:BPMNEdge id="F1_di" bpmnElement="F1"><di:waypoint x="96" y="118" /><di:waypoint x="140" y="118" /></bpmndi:BPMNEdge><bpmndi:BPMNEdge id="F2_di" bpmnElement="F2"><di:waypoint x="240" y="118" /><di:waypoint x="300" y="118" /></bpmndi:BPMNEdge></bpmndi:BPMNPlane></bpmndi:BPMNDiagram></bpmn:definitions>` });
    const run = await ok('POST', `/api/ws/${W}/graphs/${G}/run`, { dryRun: false });
    const runOf = async () => (await ok('GET', `/api/ws/${W}/runs`)).runs.find((x: any) => x.id === run.id);
    expect(await until(async () => ['completed', 'failed'].includes((await runOf())?.status), 20000)).toBe(true);
    const finished = await runOf();
    expect(finished.status, JSON.stringify(finished.log)).toBe('completed');
    expect(finished.outputs.A.text).toBe('Writing notes. Done.');
    const acpChat = sup.rt(W).chat(finished.outputs.A.chat);
    expect(acpChat.agent).toBe('fake');
    expect(finished.chats).toEqual([acpChat.id]);

    // A step marked "Ask first" stops on its approval, which shows on the run; approving it lets the run go on.
    await ok('PATCH', `/api/ws/${W}`, { acpAskFirst: true });
    const xml = (await ok('GET', `/api/ws/${W}/graphs/${G}`)).xml.replace('harness:prompt="Take notes"', 'harness:prompt="Take notes" harness:askFirst="true"');
    await ok('PUT', `/api/ws/${W}/graphs/${G}`, { xml });
    const asked = await ok('POST', `/api/ws/${W}/graphs/${G}/run`, { dryRun: false });
    const askedOf = async () => (await ok('GET', `/api/ws/${W}/runs`)).runs.find((x: any) => x.id === asked.id);
    let pending: any[] = [];
    expect(await until(async () => { const r = await askedOf(); if (!r?.chats?.length) return false; pending = (await ok('GET', `/api/ws/${W}/chats/${r.chats[0]}/permissions`)).permissions; return pending.length > 0; }, 15000)).toBe(true);
    expect((await askedOf()).status).toBe('running');
    await ok('POST', `/api/ws/${W}/chats/${(await askedOf()).chats[0]}/permission/${pending[0].id}`, { decision: 'once' });
    expect(await until(async () => (await askedOf())?.status === 'completed', 20000)).toBe(true);

    // Unmarked steps run without asking, even where the workspace asks first.
    await ok('PUT', `/api/ws/${W}/graphs/${G}`, { xml: xml.replace(' harness:askFirst="true"', '') });
    const free = await ok('POST', `/api/ws/${W}/graphs/${G}/run`, { dryRun: false });
    expect(await until(async () => (await ok('GET', `/api/ws/${W}/runs`)).runs.find((x: any) => x.id === free.id)?.status === 'completed', 20000)).toBe(true);
  });

  it('lists only connected models: the ACP agent\'s own, and opencode providers that have a key', async () => {
    const W = (await ok('POST', `/api/workspaces`, { name: 'M', kind: 'personal' })).id;
    const rt = sup.rt(W);
    const { PROVIDER_PRESETS } = await import('../../shared/src/defaults.ts');
    rt.ws.providers.push({ ...PROVIDER_PRESETS.anthropic });
    await ok('PATCH', `/api/ws/${W}`, { agents: [...rt.ws.agents, { id: 'fake', name: 'Fake agent', via: 'ACP', command: `${process.execPath} ${agent}`, on: true }] });

    // Before any chat, asking for models probes the agent once (a session, no prompt) and announces the result.
    const first = await ok('GET', `/api/ws/${W}/models`);
    expect(first.agents.fake.models).toEqual([]);
    expect(await until(async () => (await ok('GET', `/api/ws/${W}/models`)).agents.fake.models.length === 2, 10000)).toBe(true);
    const got = await ok('GET', `/api/ws/${W}/models`);
    expect(got.agents.fake).toEqual({ models: [{ id: 'fast', name: 'Fast', description: 'quick answers' }, { id: 'deep', name: 'Deep', efforts: ['low', 'high'], defaultEffort: 'high' }], current: 'fast' });
    // Anthropic is configured but has no key in the keychain, so none of its models are offered.
    expect(got.models.some((m: string) => m.startsWith('anthropic/'))).toBe(false);
    const { keychain } = await import('../src/keychain.ts');
    keychain.set(W, 'ANTHROPIC_API_KEY', 'sk-ant-test-key-not-real');
    const keyed = await ok('GET', `/api/ws/${W}/models`);
    expect(keyed.models).toEqual(expect.arrayContaining(PROVIDER_PRESETS.anthropic.models.map((m) => 'anthropic/' + m.id)));
    expect(keyed.groups.find((g: any) => g.provider === 'anthropic').models.map((m: any) => m.id)).toEqual(expect.arrayContaining(PROVIDER_PRESETS.anthropic.models.map((m) => 'anthropic/' + m.id)));

    // A chat's model and effort picks are applied to the agent's session before the next turn.
    const chat = await ok('POST', `/api/ws/${W}/chats`, { title: 'Pick a model', agent: 'fake' });
    await ok('PATCH', `/api/ws/${W}`, { acpAskFirst: false });
    await ok('PATCH', `/api/ws/${W}/chats/${chat.id}`, { model: 'deep', effort: 'low' });
    const seen: any[] = [];
    const off = sup.on((e: any) => { if (e.chat === chat.id) seen.push(e); });
    await ok('POST', `/api/ws/${W}/chats/${chat.id}/send`, { text: 'Think hard' });
    expect(await until(() => seen.some((e) => e.type === 'chat.done'), 10000)).toBe(true);
    off();
    expect(JSON.parse(readFileSync(record, 'utf8')).setModel).toEqual({ sessionId: 'S1', modelId: 'deep' });
    expect(JSON.parse(readFileSync(record, 'utf8')).setEffort).toEqual({ sessionId: 'S1', configId: 'reasoning_effort', value: 'low' });
    // Agents without a system-prompt hook get the harness instructions (with the artifact rule) on the first turn only.
    const firstPrompt = JSON.parse(readFileSync(record, 'utf8')).prompts.at(-1);
    expect(firstPrompt.startsWith('Harness instructions')).toBe(true);
    expect(firstPrompt).toContain('artifact_create');
    expect(firstPrompt.endsWith('Think hard')).toBe(true);
    expect((await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`)).messages[0].text).toBe('Think hard');

    // Slash commands: the harness set mapped for this agent, plus the agent's own (announced after session/new).
    const { commands } = await ok('GET', `/api/ws/${W}/chats/${chat.id}/commands`);
    const names = commands.map((c: any) => c.name);
    expect(names).toEqual(expect.arrayContaining(['status', 'model', 'effort', 'new', 'help', 'whoami']));
    expect(names).not.toContain('compact'); // no equivalent known for an unknown agent
    expect(commands.find((c: any) => c.name === 'whoami')).toMatchObject({ source: 'agent', hint: '[detail]' });
    const status = await ok('POST', `/api/ws/${W}/chats/${chat.id}/command`, { text: '/status' });
    expect(status).toMatchObject({ ui: 'sheet', sheet: { command: 'status', title: 'Status', status: { agentName: 'Fake agent', model: 'deep', effort: 'low' } } });
    expect(status.sheet.ran).toBeUndefined(); // an unknown agent has no own status command to run
    expect(await ok('POST', `/api/ws/${W}/chats/${chat.id}/command`, { text: '/effort high' })).toEqual({ ui: 'effort', args: 'high' });
    const res = await fetch(`${base}/api/ws/${W}/chats/${chat.id}/command`, { method: 'POST', headers: { 'content-type': 'application/json', 'x-harness-token': token }, body: JSON.stringify({ text: '/compact' }) });
    expect(res.ok).toBe(false);
    // The agent's own command reaches it exactly as typed — no harness note appended, even with web search off.
    await ok('PATCH', `/api/ws/${W}/chats/${chat.id}`, { web: false });
    seen.length = 0;
    const off2 = sup.on((e: any) => { if (e.chat === chat.id) seen.push(e); });
    expect(await ok('POST', `/api/ws/${W}/chats/${chat.id}/command`, { text: '/whoami full' })).toEqual({ ran: '/whoami full' });
    expect(await until(() => seen.some((e) => e.type === 'chat.done'), 10000)).toBe(true);
    off2();
    expect(JSON.parse(readFileSync(record, 'utf8')).prompts.at(-1)).toBe('/whoami full');
  });

  it('makes Open Design the artifact engine: saves fail clearly without it, the built-in store is opt-in', async () => {
    const W = (await ok('POST', `/api/workspaces`, { name: 'A', kind: 'personal' })).id;
    expect(sup.rt(W).ws.openDesign).toEqual({ engine: 'open-design', agentTools: false, lint: true, autoStart: true });
    const st = await ok('GET', `/api/ws/${W}/opendesign`);
    expect(st).toMatchObject({ daemon: { state: 'not-installed' }, settings: { engine: 'open-design' }, artifacts: { total: 0 } });
    const r = await fetch(`${base}/api/ws/${W}/agent/artifacts`, { method: 'POST', headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: JSON.stringify({ type: 'doc', title: 'Plan', content: '# Plan' }) });
    expect(r.ok).toBe(false);
    expect((await r.json()).error).toMatch(/Open Design is not installed/);
    expect(sup.rt(W).state.artifacts).toHaveLength(0); // nothing is kept outside Open Design behind the user's back
    await ok('PATCH', `/api/ws/${W}`, { openDesign: { engine: 'builtin' } });
    const a = await ok('POST', `/api/ws/${W}/agent/artifacts`, { type: 'doc', title: 'Plan', content: '# Plan' });
    expect(a.versions[0].od).toBeNull();
    // From a chat, the agent reads the design reference first; the save then opens beside that chat.
    const chat = await ok('POST', `/api/ws/${W}/chats`, { title: 'Deck' });
    const unguided = await fetch(`${base}/api/ws/${W}/agent/artifacts`, { method: 'POST', headers: { 'x-harness-token': token, 'content-type': 'application/json' }, body: JSON.stringify({ chat: chat.id, type: 'deck', title: 'Pitch', content: '# Hi' }) });
    expect((await unguided.json()).error).toMatch(/call artifact_guide with type "deck"/);
    const { guide } = await ok('POST', `/api/ws/${W}/agent/guide`, { chat: chat.id, type: 'deck' });
    expect(guide).toContain('## Design system: Claude Warm Editorial');
    const seen: any[] = [];
    const off = sup.on((e: any) => { if (e.type === 'artifact') seen.push(e); });
    const deck = await ok('POST', `/api/ws/${W}/agent/artifacts`, { chat: chat.id, type: 'deck', title: 'Pitch', content: '# Hi' });
    off();
    expect(seen).toEqual([{ type: 'artifact', workspace: W, chat: chat.id, artifact: deck.id, version: 1 }]);
    // Every agent is told to make artifacts with the harness tools.
    expect(sup.rt(W).inheritedPrefix(sup.rt(W).general().id)).toContain('artifact_create');
  });

  it('shows Claude Code local command output that its ACP adapter drops', async () => {
    const W = (await ok('POST', `/api/workspaces`, { name: 'C', kind: 'personal' })).id;
    const rt = sup.rt(W);
    await ok('PATCH', `/api/ws/${W}`, { acpAskFirst: false, agents: rt.ws.agents.map((a: any) => (a.id === 'claude' ? { ...a, on: true, command: `${process.execPath} ${agent}` } : a)) });
    const chat = await ok('POST', `/api/ws/${W}/chats`, { title: 'Status', agent: 'claude' });
    const seen: any[] = [];
    const off = sup.on((e: any) => { if (e.chat === chat.id) seen.push(e); });
    // /status answers in an action sheet: the harness's status plus Claude Code's own /context, run quietly.
    expect(await ok('POST', `/api/ws/${W}/chats/${chat.id}/command`, { text: '/status' })).toMatchObject({ ui: 'sheet', sheet: { command: 'status', ran: '/context', text: '## Context Usage\n10.8k / 1m', status: { agentName: 'Claude Code' } } });
    off();
    // Nothing of it reaches the chat: no streamed tokens, no transcript entries.
    expect(seen.filter((e) => e.type === 'chat.delta' || e.type === 'chat.done')).toEqual([]);
    const { messages } = await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`);
    expect(messages).toEqual([]);
    // Claude Code gets the harness instructions, artifact rule included, as a system prompt addition.
    expect(JSON.parse(readFileSync(record, 'utf8')).meta.systemPrompt.append).toContain('artifact_create');
  });
});

describe('Signed out and memory', () => {
  it('asks to sign in again in the chat when the agent\'s login has expired', async () => {
    const W = (await ok('POST', `/api/workspaces`, { name: 'S', kind: 'personal' })).id;
    const rt = sup.rt(W);
    await ok('PATCH', `/api/ws/${W}`, { acpAskFirst: false, agents: rt.ws.agents.map((a: any) => (a.id === 'claude' ? { ...a, on: true, command: `${process.execPath} ${agent}` } : a)) });
    const chat = await ok('POST', `/api/ws/${W}/chats`, { title: 'Expired', agent: 'claude' });
    const done: any[] = [];
    const off = sup.on((e: any) => { if (e.chat === chat.id && e.type === 'chat.done') done.push(e); });
    await ok('POST', `/api/ws/${W}/chats/${chat.id}/send`, { text: 'signed-out hello' });
    expect(await until(() => done.length > 0, 10000)).toBe(true);
    off();
    expect(done[0].outcome).toBe('signin');
    const { messages } = await ok('GET', `/api/ws/${W}/chats/${chat.id}/messages`);
    expect(messages.at(-1)).toMatchObject({ role: 'assistant', signIn: 'claude', error: 'Claude Code is signed out in this workspace.' });
    expect(await ok('GET', `/api/ws/${W}/subscriptions/claude/signin`)).toMatchObject({ state: 'idle' });
  });

  it('starts every session from memory, unless memory is turned off', async () => {
    const W = (await ok('POST', `/api/workspaces`, { name: 'M', kind: 'personal' })).id;
    const rt = sup.rt(W);
    await ok('PATCH', `/api/ws/${W}`, { acpAskFirst: false, agents: rt.ws.agents.map((a: any) => (a.id === 'claude' ? { ...a, on: true, command: `${process.execPath} ${agent}` } : a)) });
    await ok('POST', `/api/ws/${W}/facts`, { text: 'Walls are 8 mm thick', scope: 'ws', source: 'test' });
    const open = async () => {
      const chat = await ok('POST', `/api/ws/${W}/chats`, { title: 'Mem', agent: 'claude' });
      await ok('POST', `/api/ws/${W}/chats/${chat.id}/command`, { text: '/status' });
      return JSON.parse(readFileSync(record, 'utf8'));
    };
    const on = await open();
    expect(on.meta.systemPrompt.append).toContain('## Memory');
    expect(on.meta.systemPrompt.append).toContain('Walls are 8 mm thick');
    expect(on.mcp.map((m: any) => m.name)).toContain('harness-memory');
    await ok('PATCH', `/api/ws/${W}`, { useMemory: false });
    const off = await open();
    expect(off.meta.systemPrompt.append).not.toContain('## Memory');
    expect(off.mcp.map((m: any) => m.name)).not.toContain('harness-memory');
  });
});

describe('Claude Code model catalog', () => {
  it('reads each model\'s effort levels from the CLI without sending a prompt', async () => {
    const { claudeEffortCatalog } = await import('../src/acp.ts');
    const { chmodSync } = await import('node:fs');
    const dir = mkdtempSync(join(tmpdir(), 'claude-cli-'));
    const cli = join(dir, 'claude');
    // Answers the stream-json `initialize` control request the way Claude Code does, and records anything else it gets.
    writeFileSync(cli, `#!${process.execPath}
let buf = '';
process.stdin.on('data', (d) => { buf += d; let i; while ((i = buf.indexOf('\\n')) >= 0) { const m = JSON.parse(buf.slice(0, i)); buf = buf.slice(i + 1);
  if (m.request?.subtype === 'initialize') process.stdout.write(JSON.stringify({ type: 'control_response', response: { request_id: m.request_id, response: { models: [{ value: 'opus', supportedEffortLevels: ['low', 'high', 'max'] }, { value: 'haiku-old' }] } } }) + '\\n');
  else require('fs').writeFileSync(${JSON.stringify(join(dir, 'unexpected'))}, JSON.stringify(m)); } });
`);
    chmodSync(cli, 0o755);
    const cat = await claudeEffortCatalog({ CLAUDE_CODE_EXECUTABLE: cli }, 10000);
    expect(Object.fromEntries(cat)).toEqual({ opus: ['low', 'high', 'max'], 'haiku-old': [] });
    expect(existsSync(join(dir, 'unexpected'))).toBe(false);
  });
});
