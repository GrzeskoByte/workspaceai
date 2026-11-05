//! Slash commands and `@` references in the chat composer. Port of `packages/shared/src/commands.ts`.
//! The harness has one command set for every chat, and each command runs as the chat's agent's own
//! equivalent (`/status` → Claude Code's `/context`, Grok's `/session-info`, opencode's session totals).
//! Offsets are UTF-8 byte offsets on char boundaries.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// How a harness command runs for one agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CommandAction {
    /// Send this slash command to the agent as the turn's text (Claude Code and Grok run them themselves).
    Native { command: String },
    /// Run an opencode server command (`POST /session/:id/command`).
    OcCommand { name: String },
    /// Compact an opencode session (`POST /session/:id/compact`).
    OcCompact,
    /// Handled in the app: a status card, the model dialog, a new chat, the command list.
    Ui { action: UiAction, then: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiAction {
    Status,
    Model,
    Effort,
    New,
    Help,
    Graph,
}

/// Per-agent actions; a missing agent falls back to `any`, and with no `any` the command is not offered there.
#[derive(Debug, Clone, Default)]
pub struct Run {
    pub opencode: Option<CommandAction>,
    pub claude: Option<CommandAction>,
    pub grok: Option<CommandAction>,
    pub any: Option<CommandAction>,
}

#[derive(Debug, Clone)]
pub struct HarnessCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub hint: Option<&'static str>,
    pub aliases: &'static [&'static str],
    pub run: Run,
    /// Answers in an action sheet over the chat instead of in the conversation.
    pub sheet: Option<&'static str>,
}

fn ui(action: UiAction, then: Option<&str>) -> Option<CommandAction> {
    Some(CommandAction::Ui { action, then: then.map(Into::into) })
}

fn native(command: &str) -> Option<CommandAction> {
    Some(CommandAction::Native { command: command.into() })
}

fn oc(name: &str) -> Option<CommandAction> {
    Some(CommandAction::OcCommand { name: name.into() })
}

fn cmd(name: &'static str, description: &'static str, hint: Option<&'static str>, aliases: &'static [&'static str], sheet: Option<&'static str>, run: Run) -> HarnessCommand {
    HarnessCommand { name, description, hint, aliases, run, sheet }
}

pub static HARNESS_COMMANDS: LazyLock<Vec<HarnessCommand>> = LazyLock::new(|| {
    use UiAction::*;
    vec![
        cmd("status", "Agent, model, effort and session — plus the agent’s own status", None, &[], Some("Status"),
            Run { claude: ui(Status, Some("/context")), grok: ui(Status, Some("/session-info")), opencode: ui(Status, None), any: ui(Status, None) }),
        cmd("context", "How much of the context window is used", None, &[], Some("Context"),
            Run { claude: native("/context"), grok: native("/context"), opencode: ui(Status, None), any: None }),
        cmd("usage", "Cost and plan usage for this session", None, &["cost"], Some("Usage"),
            Run { claude: native("/usage"), grok: native("/session-info"), opencode: ui(Status, None), any: None }),
        cmd("compact", "Summarize the conversation to free up context", Some("[instructions]"), &[], None,
            Run { claude: native("/compact"), grok: native("/compact"), opencode: Some(CommandAction::OcCompact), any: None }),
        cmd("model", "Choose the model", Some("[model]"), &["models"], None, Run { any: ui(Model, None), ..Run::default() }),
        cmd("effort", "Set the reasoning effort", Some("[low|medium|high|xhigh|max|default]"), &[], None, Run { any: ui(Effort, None), ..Run::default() }),
        cmd("new", "Start a new chat with the same agent and model", None, &["clear"], None, Run { any: ui(New, None), ..Run::default() }),
        cmd("init", "Write the project’s agent instructions file", None, &[], None, Run { claude: native("/init"), opencode: oc("init"), ..Run::default() }),
        cmd("review", "Review the current changes", Some("[target]"), &[], None,
            Run { claude: native("/code-review"), grok: native("/review"), opencode: oc("review"), any: None }),
        cmd("goal", "Keep working until an objective is met", Some("<objective>"), &[], None,
            Run { claude: native("/goal"), grok: native("/goal"), opencode: oc("goal"), any: None }),
        cmd("loop", "Run a prompt on a recurring interval", Some("[interval] <prompt>"), &[], None, Run { claude: native("/loop"), grok: native("/loop"), ..Run::default() }),
        cmd("graph", "Run one of the project’s graphs here — its results come back to this chat", Some("[graph name] [--dry]"), &["run"], None, Run { any: ui(Graph, None), ..Run::default() }),
        cmd("help", "List the commands this chat understands", None, &[], Some("Commands"), Run { any: ui(Help, None), ..Run::default() }),
    ]
});

/// Agent commands the harness owns or that make no sense inside a harness chat, so they are not offered.
fn hidden_native(agent: &str) -> &'static [&'static str] {
    match agent {
        "claude" => &["clear", "model", "effort", "rename", "color", "focus", "heapdump", "config", "output-style", "import", "statusline", "__remote-workflow", "workflow-launch-exec"],
        "grok" => &["always-approve", "statusline", "feedback"],
        _ => &[],
    }
}

/// What a harness command does for this agent, or `None` when the agent has no equivalent.
pub fn command_action(cmd: &HarnessCommand, agent: &str) -> Option<CommandAction> {
    let own = match agent {
        "opencode" => &cmd.run.opencode,
        "claude" => &cmd.run.claude,
        "grok" => &cmd.run.grok,
        _ => &None,
    };
    own.clone().or_else(|| cmd.run.any.clone())
}

pub fn find_harness_command(name: &str) -> Option<&'static HarnessCommand> {
    let n = name.to_lowercase();
    HARNESS_COMMANDS.iter().find(|c| c.name == n || c.aliases.contains(&n.as_str()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub start: usize,
    pub query: String,
}

/// The `@` reference being typed at the caret, if any. An `@` inside a word (an email address) is not one.
pub fn mention_at(text: &str, caret: usize) -> Option<Mention> {
    static AT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:^|\s)@([^\s@"]*)$"#).unwrap());
    let c = AT.captures(text.get(..caret)?)?;
    let q = c.get(1)?;
    Some(Mention { start: q.start() - 1, query: q.as_str().into() })
}

/// Replaces the reference typed from `start` to `caret` with `path`; `open` leaves it unfinished to keep completing.
/// Offsets that are not char boundaries leave the text unchanged.
pub fn insert_mention(text: &str, start: usize, caret: usize, path: &str, open: bool) -> (String, usize) {
    let (Some(head), Some(tail)) = (text.get(..start), text.get(caret..)) else { return (text.to_string(), caret) };
    if start > caret {
        return (text.to_string(), caret);
    }
    let name = if path.contains(char::is_whitespace) { format!("\"{path}\"") } else { path.to_string() };
    let reference = format!("@{name}{}", if open { "" } else { " " });
    (format!("{head}{reference}{tail}"), start + reference.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandSource {
    Harness,
    Agent,
}

/// A command as the composer lists it. `runs_as` says what actually runs for this chat's agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCommand {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub hint: Option<String>,
    pub source: CommandSource,
    #[serde(default)]
    pub runs_as: Option<String>,
    #[serde(default)]
    pub sheet: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCommand {
    pub name: String,
    pub description: Option<String>,
    pub hint: Option<String>,
}

/// WorkspaceAI commands mapped for `agent`, then the agent's own commands that the harness does not already cover.
pub fn chat_commands(agent: &str, agent_name: &str, native_cmds: &[NativeCommand]) -> Vec<ChatCommand> {
    let mut out = Vec::new();
    let mut covered: Vec<&str> = Vec::new();
    for c in HARNESS_COMMANDS.iter() {
        let Some(action) = command_action(c, agent) else { continue };
        covered.push(c.name);
        covered.extend(c.aliases);
        let runs_as = match action {
            CommandAction::Native { command } => Some(format!("{agent_name} {command}")),
            CommandAction::OcCommand { name } => Some(format!("opencode /{name}")),
            CommandAction::OcCompact => Some("opencode compaction".to_string()),
            CommandAction::Ui { then: Some(then), .. } => Some(format!("status card + {agent_name} {then}")),
            CommandAction::Ui { then: None, .. } => None,
        };
        out.push(ChatCommand {
            name: c.name.into(),
            description: c.description.into(),
            hint: c.hint.map(Into::into),
            source: CommandSource::Harness,
            runs_as,
            sheet: c.sheet.is_some(),
        });
    }
    let hidden = hidden_native(agent);
    for n in native_cmds {
        let name = n.name.strip_prefix('/').unwrap_or(&n.name);
        if name.is_empty() || covered.contains(&name) || hidden.contains(&name) {
            continue;
        }
        out.push(ChatCommand {
            name: name.into(),
            description: n.description.clone().unwrap_or_default(),
            hint: n.hint.clone().filter(|h| !h.is_empty()),
            source: CommandSource::Agent,
            runs_as: None,
            sheet: false,
        });
    }
    out
}

/// Split composer text into a slash command and its arguments; `None` when the text is not a command.
pub fn parse_slash(text: &str) -> Option<(String, String)> {
    static SLASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)^/([A-Za-z0-9][\w:.-]*)(?:\s+(.*))?$").unwrap());
    let c = SLASH.captures(text.trim())?;
    Some((c[1].to_string(), c.get(2).map_or("", |g| g.as_str()).trim().to_string()))
}
