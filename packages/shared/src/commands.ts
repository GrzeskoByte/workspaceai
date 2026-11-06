// Slash commands in the chat composer. The harness has one command set for every chat, and each command runs as
// the chat's agent's own equivalent (`/status` → Claude Code's `/context`, Grok's `/session-info`, opencode's
// session totals). The agent's own commands are offered as well and passed through as typed.

/** How a harness command runs for one agent. */
export type CommandAction =
  /** Send this slash command to the agent as the turn's text (Claude Code and Grok run them themselves). */
  | { kind: 'native'; command: string }
  /** Run an opencode server command (`POST /session/:id/command`). */
  | { kind: 'oc-command'; name: string }
  /** Compact an opencode session (`POST /session/:id/compact`). */
  | { kind: 'oc-compact' }
  /** Handled in the app: a status card, the model dialog, a new chat, the command list. */
  | { kind: 'ui'; action: 'status' | 'model' | 'effort' | 'new' | 'help' | 'graph'; then?: string };

/** The agent families commands are mapped for. ACP agents not listed get only the app-side commands. */
export type CommandAgent = 'opencode' | 'claude' | 'grok';

export interface HarnessCommand {
  name: string;
  description: string;
  hint?: string;
  aliases?: string[];
  /** Per agent; missing means the agent has no equivalent and the command is not offered there. */
  run: Partial<Record<CommandAgent | '*', CommandAction>>;
  /** Answers in an action sheet over the chat instead of in the conversation (the agent's own output is captured). */
  sheet?: string;
}

const ui = (action: 'status' | 'model' | 'effort' | 'new' | 'help' | 'graph', then?: string): CommandAction => ({ kind: 'ui', action, ...(then ? { then } : {}) });
const native = (command: string): CommandAction => ({ kind: 'native', command });

export const HARNESS_COMMANDS: HarnessCommand[] = [
  { name: 'status', description: 'Agent, model, effort and session — plus the agent’s own status', sheet: 'Status', run: { claude: ui('status', '/context'), grok: ui('status', '/session-info'), opencode: ui('status'), '*': ui('status') } },
  { name: 'context', description: 'How much of the context window is used', sheet: 'Context', run: { claude: native('/context'), grok: native('/context'), opencode: ui('status') } },
  { name: 'usage', description: 'Cost and plan usage for this session', aliases: ['cost'], sheet: 'Usage', run: { claude: native('/usage'), grok: native('/session-info'), opencode: ui('status') } },
  { name: 'compact', description: 'Summarize the conversation to free up context', hint: '[instructions]', run: { claude: native('/compact'), grok: native('/compact'), opencode: { kind: 'oc-compact' } } },
  { name: 'model', description: 'Choose the model', hint: '[model]', aliases: ['models'], run: { '*': ui('model') } },
  { name: 'effort', description: 'Set the reasoning effort', hint: '[low|medium|high|xhigh|max|default]', run: { '*': ui('effort') } },
  { name: 'new', description: 'Start a new chat with the same agent and model', aliases: ['clear'], run: { '*': ui('new') } },
  { name: 'init', description: 'Write the project’s agent instructions file', run: { claude: native('/init'), opencode: { kind: 'oc-command', name: 'init' } } },
  { name: 'review', description: 'Review the current changes', hint: '[target]', run: { claude: native('/code-review'), grok: native('/review'), opencode: { kind: 'oc-command', name: 'review' } } },
  { name: 'goal', description: 'Keep working until an objective is met', hint: '<objective>', run: { claude: native('/goal'), grok: native('/goal'), opencode: { kind: 'oc-command', name: 'goal' } } },
  { name: 'loop', description: 'Run a prompt on a recurring interval', hint: '[interval] <prompt>', run: { claude: native('/loop'), grok: native('/loop') } },
  { name: 'graph', description: 'Run one of the project’s graphs here — its results come back to this chat', hint: '[graph name] [--dry]', aliases: ['run'], run: { '*': ui('graph') } },
  { name: 'help', description: 'List the commands this chat understands', sheet: 'Commands', run: { '*': ui('help') } },
];

/** Agent commands the harness owns or that make no sense inside a harness chat, so they are not offered. */
const HIDDEN_NATIVE: Record<string, string[]> = {
  claude: ['clear', 'model', 'effort', 'rename', 'color', 'focus', 'heapdump', 'config', 'output-style', 'import', 'statusline', '__remote-workflow', 'workflow-launch-exec'],
  grok: ['always-approve', 'statusline', 'feedback'],
};

function family(agent: string): CommandAgent | '*' {
  return agent === 'opencode' || agent === 'claude' || agent === 'grok' ? agent : '*';
}

/** What a harness command does for this agent, or null when the agent has no equivalent. */
export function commandAction(cmd: HarnessCommand, agent: string): CommandAction | null {
  return cmd.run[family(agent)] || cmd.run['*'] || null;
}

export function findHarnessCommand(name: string): HarnessCommand | undefined {
  const n = name.toLowerCase();
  return HARNESS_COMMANDS.find((c) => c.name === n || c.aliases?.includes(n));
}

/** The `@` reference being typed at the caret, if any. An `@` inside a word (an email address) is not one. */
export function mentionAt(text: string, caret: number): { start: number; query: string } | null {
  const m = /(^|\s)@([^\s@"]*)$/.exec(text.slice(0, caret));
  return m ? { start: m.index + m[1].length, query: m[2] } : null;
}

/** Replaces the reference typed from `start` to `caret` with `path`; `open` leaves it unfinished to keep completing. */
export function insertMention(text: string, start: number, caret: number, path: string, open = false): { text: string; caret: number } {
  const ref = '@' + (/\s/.test(path) ? `"${path}"` : path) + (open ? '' : ' ');
  return { text: text.slice(0, start) + ref + text.slice(caret), caret: start + ref.length };
}

/** A command as the composer lists it. `runsAs` says what actually runs for this chat's agent. */
export interface ChatCommand { name: string; description: string; hint?: string; source: 'harness' | 'agent'; runsAs?: string; sheet?: boolean }

export interface NativeCommand { name: string; description?: string; hint?: string }

/** WorkspaceAI commands mapped for `agent`, then the agent's own commands that the harness does not already cover. */
export function chatCommands(agent: string, agentName: string, nativeCmds: NativeCommand[]): ChatCommand[] {
  const out: ChatCommand[] = [];
  const covered = new Set<string>();
  for (const c of HARNESS_COMMANDS) {
    const a = commandAction(c, agent);
    if (!a) continue;
    covered.add(c.name); c.aliases?.forEach((x) => covered.add(x));
    const runsAs = a.kind === 'native' ? `${agentName} ${a.command}`
      : a.kind === 'oc-command' ? `opencode /${a.name}`
      : a.kind === 'oc-compact' ? 'opencode compaction'
      : a.then ? `status card + ${agentName} ${a.then}` : undefined;
    out.push({ name: c.name, description: c.description, ...(c.hint ? { hint: c.hint } : {}), source: 'harness', ...(runsAs ? { runsAs } : {}), ...(c.sheet ? { sheet: true } : {}) });
  }
  const hidden = new Set(HIDDEN_NATIVE[agent] || []);
  for (const n of nativeCmds) {
    const name = n.name.replace(/^\//, '');
    if (!name || covered.has(name) || hidden.has(name)) continue;
    out.push({ name, description: n.description || '', ...(n.hint ? { hint: n.hint } : {}), source: 'agent' });
  }
  return out;
}

/** Split composer text into a slash command and its arguments; null when the text is not a command. */
export function parseSlash(text: string): { name: string; args: string } | null {
  const m = /^\/([A-Za-z0-9][\w:.-]*)(?:\s+([\s\S]*))?$/.exec(text.trim());
  return m ? { name: m[1], args: (m[2] || '').trim() } : null;
}
