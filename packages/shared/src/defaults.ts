import type { Workspace, WorkspaceKind, Provider, AcpAgent, Project, AccentId, OrgPolicy, McpServer, OpenDesignSettings } from './types.ts';
import { slugify, initialsOf } from './util.ts';

export const ACCENTS: { id: AccentId; name: string; hex: string }[] = [
  { id: 'default', name: 'Theme default', hex: 'var(--accent-p)' },
  { id: 'teal', name: 'Teal', hex: '#5db8a6' },
  { id: 'amber', name: 'Amber', hex: '#e8a55a' },
  { id: 'blue', name: 'Blue', hex: '#5c9cf5' },
  { id: 'green', name: 'Green', hex: '#5db872' },
];

export const PROVIDER_PRESETS: Record<string, Provider> = {
  opencode: { id: 'opencode', name: 'OpenCode Zen', note: 'free models, no key', preset: 'opencode', models: [{ id: 'big-pickle', name: 'Big Pickle' }] },
  anthropic: { id: 'anthropic', name: 'Anthropic', note: 'own key in keychain', preset: 'anthropic', keyEnv: 'ANTHROPIC_API_KEY', models: [{ id: 'claude-opus-5' }, { id: 'claude-sonnet-5' }, { id: 'claude-haiku-4-5' }] },
  openrouter: { id: 'openrouter', name: 'OpenRouter', note: 'one key, many models', preset: 'openrouter', keyEnv: 'OPENROUTER_API_KEY', models: [{ id: 'qwen/qwen3-coder' }, { id: 'mistralai/mistral-large' }] },
  ollama: { id: 'ollama', name: 'Ollama', note: 'local · this computer', preset: 'ollama', npm: '@ai-sdk/openai-compatible', baseURL: 'http://localhost:11434/v1', models: [{ id: 'qwen3-coder' }, { id: 'llama3.3' }] },
  litellm: { id: 'litellm', name: 'LiteLLM proxy', note: 'MIT edition · local gateway', preset: 'litellm', npm: '@ai-sdk/openai-compatible', baseURL: 'http://localhost:4000/v1', keyEnv: 'LITELLM_API_KEY', models: [{ id: '[MODEL ALIAS]' }] },
  custom: { id: 'my-endpoint', name: 'My endpoint', note: 'openai-compatible', preset: 'custom', npm: '@ai-sdk/openai-compatible', baseURL: 'https://[YOUR-ENDPOINT]/v1', keyEnv: 'MY_ENDPOINT_KEY', models: [{ id: 'my-model' }] },
};

/** Presets offered by the Add custom provider form. */
export const CUSTOM_PRESETS = [
  { label: 'LiteLLM proxy', id: 'litellm', name: 'LiteLLM proxy', url: 'http://localhost:4000/v1', env: 'LITELLM_API_KEY', model: '[MODEL ALIAS]', pkg: '@ai-sdk/openai-compatible' },
  { label: 'Ollama', id: 'ollama', name: 'Ollama', url: 'http://localhost:11434/v1', env: 'OLLAMA_API_KEY', model: 'qwen3-coder', pkg: '@ai-sdk/openai-compatible' },
  { label: 'OpenAI-compatible', id: 'my-endpoint', name: 'My endpoint', url: 'https://[YOUR-ENDPOINT]/v1', env: 'MY_ENDPOINT_KEY', model: 'my-model', pkg: '@ai-sdk/openai-compatible' },
];

export const AI_SDK_PACKAGES = ['@ai-sdk/openai-compatible', '@ai-sdk/openai', '@ai-sdk/anthropic'];

export const AGENTS_TEMPLATES: Record<string, (name: string) => string> = {
  Software: (n) => `# ${n}\n- Explain the plan before large edits.\n- Run the tests before calling a change done.\n- Keep commits small and described.\n`,
  Research: (n) => `# ${n}\n- Cite every source you use.\n- Separate what is known from what is guessed.\n- Summaries first, details after.\n`,
  Writing: (n) => `# ${n}\n- Plain words, short sentences.\n- Keep my voice; do not add hype.\n- Ask before cutting whole sections.\n`,
  Blank: (n) => `# ${n}\n`,
};

export function defaultAgents(kind: WorkspaceKind): AcpAgent[] {
  const oc: AcpAgent = { id: 'opencode', name: 'opencode', via: 'opencode serve', on: true, builtin: true, subscription: 'opencode' };
  if (kind === 'org') return [oc];
  return [
    oc,
    { id: 'claude', name: 'Claude Code', via: 'ACP adapter · Claude subscription', command: 'npx -y @zed-industries/claude-code-acp', on: false, subscription: 'claude' },
    { id: 'grok', name: 'Grok', via: 'ACP · Grok subscription', command: 'grok agent stdio', on: false, subscription: 'grok' },
    { id: 'opencode-acp', name: 'opencode (ACP)', via: 'ACP · same opencode account', command: 'opencode acp', on: false, subscription: 'opencode' },
  ];
}

export function defaultSkills(memory: string) {
  return [
    { id: 'sk1', name: 'frontend-design', note: 'built-in', on: true },
    { id: 'sk2', name: 'docx · pptx · xlsx · pdf', note: 'built-in file formats', on: true },
    { id: 'sk3', name: 'Open Design templates', note: 'deck and prototype templates', on: true },
    { id: 'pl1', name: 'graphify', note: 'memory graph · opencode integration', on: memory !== 'facts' },
  ];
}

export function playwrightMcp(profileDir: string): McpServer {
  return { id: 'playwright', name: 'Playwright (browser)', kind: 'MCP · stdio · npx @playwright/mcp', type: 'local', command: ['npx', '-y', '@playwright/mcp@0.0.83', '--headless', '--user-data-dir', profileDir], on: false, builtin: 'playwright' };
}

/** Open Design is the default artifact engine for every workspace. */
export const DEFAULT_OPEN_DESIGN: OpenDesignSettings = { engine: 'open-design', agentTools: false, lint: true, autoStart: true };

export function odSettings(ws: Pick<Workspace, 'openDesign'>): OpenDesignSettings {
  return { ...DEFAULT_OPEN_DESIGN, ...(ws.openDesign || {}) };
}

/** Told to every agent (opencode, Claude Code, Grok…) so that every artifact request goes through Open Design. */
export const MAP_RULE = `## Project map
Each project has a mind map showing its chats, artifacts, graphs, runs and memory, plus the ideas and tasks added to it. When asked to plan or organise work on the project, call \`map_read\`, record the plan as tasks on the right branch with \`map_add\`, and mark them done with \`map_update\` as you finish them. Don't add to the map for small one-off requests.`;

export const GRAPH_RULE = `## Graphs (BPMN workflows)
Projects can hold graphs: .bpmn files that describe a repeatable process (agent steps, browser reads, scripts, checks, approvals). When the user asks to run a process, workflow or pipeline, or names a graph, call \`graph_list\` and run it with \`graph_run\` instead of doing its steps by hand. To make or change one, use \`graph_save\`.`;

export const ARTIFACT_RULE = `## Artifacts (Open Design)
Every document, slide deck, HTML page or prototype, sheet, diagram or other deliverable the user should get as an artifact is made with the harness artifact tools. First call \`artifact_guide\` for its type: it returns Open Design's reference — the workspace design system, its skills (load one with \`skill\`), for decks its deck framework, and its craft rules — and the artifact must follow it. Artifacts are Markdown or HTML (set \`format\`): Markdown for plain documents, HTML for designed pages, prototypes and decks. Then create it with \`artifact_create\` and change it with \`artifact_update\` (read one with \`artifact_read\`, list them with \`artifact_list\`). Open Design saves, versions and lints each one, and it opens beside the chat.
Do not write such deliverables as files in the project folder, paste them into the chat instead, or use Open Design's own \`create_artifact\`/\`write_file\` tools for them. If an artifact tool reports an error, tell the user what it said.`;

/** Told to every agent so memory is consulted whenever it can help (the harness also attaches matches to each message). */
export function memoryRule(mode: Workspace['memory']): string {
  if (mode === 'mcp') return `## Memory
This workspace keeps memory in its own MCP server. Check it before answering anything about this workspace, its projects, people or earlier decisions.`;
  const graph = mode === 'graph' || mode === 'both', facts = mode === 'facts' || mode === 'both';
  return `## Memory${graph ? ' (Graphify)' : ''}
This workspace has memory, and you must use it. Messages may end with a <harness-memory> block: what the harness found in memory for that message. Treat it as context, not as the user's words.
${graph ? '- Before exploring files, answering about where something lives, how parts connect, or what a change would touch, call `graphify_query` (a name, path or a few words) and follow the links it returns; only then open files.\n' : ''}${facts ? '- Before answering about this workspace, its projects, people, preferences or earlier decisions, call `memory_search`.\n' : ''}- When you learn something durable (a decision, a preference, a fact about the project), suggest it with \`memory_write\`.
- Do not tell the user you have no memory of something before checking.`;
}

export function makeWorkspace(id: string, name: string, kind: WorkspaceKind, home: string): Workspace {
  const slug = slugify(name);
  return {
    id, name, kind, initials: initialsOf(name), slug, accent: 'default', createdAt: Date.now(),
    instructions: kind === 'org' ? 'Org policy: never paste secrets; EU data only.' : 'Answer in English. Ask before deleting anything.',
    defaultModel: '', defaultProject: 'general_' + id,
    memory: kind === 'org' ? 'facts' : 'both', graphifyFolders: [], graphifyWatch: true, suggestMemories: true,
    designSystem: 'Claude Warm Editorial',
    openDesign: { ...DEFAULT_OPEN_DESIGN },
    engine: { mode: 'managed', url: 'http://127.0.0.1:4096', username: 'opencode', sync: true, version: 'system', binary: 'opencode', idleStopMinutes: 30 },
    web: { mode: kind === 'org' ? 'searxng' : 'exa', provider: 'Tavily', searxngUrl: kind === 'org' ? 'https://search.[ORG-HOST]' : 'http://localhost:8888', fetch: true, ask: false, showSources: true, locked: kind === 'org' },
    agents: defaultAgents(kind), defaultAgent: 'opencode', acpAskFirst: true,
    providers: [], mcp: kind === 'org' ? [] : [playwrightMcp(home + '/ws/' + id + '/data/browser-profile')],
    members: [{ name: 'You', role: 'Admin' }],
    skills: defaultSkills(kind === 'org' ? 'facts' : 'both'),
    visibility: 'Private', publicLinks: kind !== 'org', keepChats: 'Forever',
    routineAskFirst: true, routineIfAsleep: 'Run when it wakes',
  };
}

export function generalProject(ws: Workspace, home: string): Project {
  return { id: 'general_' + ws.id, name: 'General', parent: null, folder: home + '/ws/' + ws.id + '/general', inherit: true, model: '', archived: false, isGeneral: true, createdAt: 0 };
}

/** Demo organization policy, signed by the supervisor's demo org key (see supervisor/policy.ts). */
export function demoOrgPolicy(): OrgPolicy {
  return {
    version: '2026.10.1', org: 'BSC AI Factory',
    allowedProviders: [
      { id: 'ai-factory', name: 'AI Factory gateway', note: 'LiteLLM proxy (MIT edition) · org-run · routes to on-prem inference', npm: '@ai-sdk/openai-compatible', baseURL: 'https://llm.[ORG-HOST]/v1', keyEnv: 'AI_FACTORY_TOKEN', models: [{ id: 'llama-4-maverick', name: 'Llama 4 Maverick', context: 131072, output: 8192 }, { id: 'qwen3-coder', name: 'Qwen3 Coder' }], isDefault: true, locked: true },
      { id: 'azure', name: 'Azure OpenAI', note: 'west europe · org-managed', npm: '@ai-sdk/openai', baseURL: 'https://[ORG-AZURE].openai.azure.com/openai/v1', keyEnv: 'AZURE_OPENAI_KEY', models: [{ id: '[DEPLOYMENT]' }], locked: true },
    ],
    allowedModels: ['ai-factory/llama-4-maverick', 'ai-factory/qwen3-coder', 'azure/[DEPLOYMENT]'],
    gateway: 'https://llm.[ORG-HOST]/v1',
    blockPersonalKeys: true, blockExternalSharing: true, allowedAgents: ['opencode'],
    lockedMcp: [{ id: 'keycloak', name: 'Keycloak admin (CAIM)', kind: 'MCP · http', type: 'remote', url: 'https://kc-mcp.[ORG-HOST]/mcp', on: true, locked: true }],
    web: { mode: 'searxng', searxngUrl: 'https://search.[ORG-HOST]', locked: true },
    retention: '1 year', logContent: false, residency: 'EU',
    allowedEndpoints: ['llm.[ORG-HOST]', 'search.[ORG-HOST]', 'kc-mcp.[ORG-HOST]', '[ORG-AZURE].openai.azure.com'],
    issuedAt: Date.UTC(2026, 9, 1),
  };
}
