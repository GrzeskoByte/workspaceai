//! First-run setup: steps, wording, the checks against the supervisor and the `finish` request. Port of the logic
//! in `apps/ui/src/onboarding/Onboarding.tsx` (the window code is in `crates/app/src/onboarding.rs`).

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

/// (key, title, note) of the ten steps, in order.
pub const STEPS: [(&str, &str, &str); 10] = [
    ("welcome", "Welcome", "what this is"),
    ("engine", "opencode", "start or connect"),
    ("agents", "Subscriptions", "Claude, Grok, opencode"),
    ("sessions", "Sessions", "import earlier chats"),
    ("workspace", "Workspace", "your personal space"),
    ("model", "Model", "a provider to start"),
    ("web", "Web search", "look things up"),
    ("memory", "Memory", "facts or a graph"),
    ("project", "Project", "your first project"),
    ("look", "Look", "theme"),
];

/// (title, text) of the three Welcome cards; picking one shows `TOUR[i + 1]`.
pub const CARDS: [(&str, &str); 3] = [("Separate workspaces", "Own chats, memory, models and connectors each"), ("Projects all the way down", "Every chat lives in a project; projects nest"), ("Artifacts and loops", "Docs, decks, prototypes; drag-and-drop agent graphs")];
pub const TOUR: [&str; 4] = [
    "Pick a card to see how it works.",
    "Each workspace runs its own engine with its own config folder. Switching changes which one you talk to; nothing is copied across.",
    "Start a chat and pick the project it belongs to. A sub-project is a folder inside its parent and inherits its instructions.",
    "Ask for a doc, deck or prototype in any chat and it appears beside the conversation. Loops are built by dragging blocks.",
];

/// (kind, title, note) of the model providers (`claude` and `existing` are offered only when available).
pub const PROVIDERS: [(&str, &str, &str); 7] = [
    ("claude", "Claude subscription", "Pro / Max · via Claude Code"),
    ("existing", "From opencode", "reuse its providers"),
    ("opencode", "OpenCode Zen", "free models, no key"),
    ("anthropic", "Anthropic", "API key"),
    ("openrouter", "OpenRouter", "one key, many models"),
    ("ollama", "Ollama", "local, no key"),
    ("custom", "Custom endpoint", "any OpenAI-compatible URL"),
];
pub const OLLAMA_MODELS: [&str; 3] = ["qwen3-coder", "llama3.3", "mistral-small"];
pub const WEB_MODES: [(&str, &str, &str); 4] = [
    ("off", "Off", "No search; pasted links only if page reading is on"),
    ("exa", "Built-in", "opencode’s websearch (Exa), no key"),
    ("mcp", "Search provider", "Tavily, Brave or Firecrawl over MCP"),
    ("searxng", "Self-hosted", "Your SearXNG; queries stay on your network"),
];
pub const MEMORY: [(&str, &str, &str); 4] = [
    ("facts", "Facts", "Short statements you approve, kept as files"),
    ("graph", "Graphify", "A knowledge graph of your project folders, queried over MCP"),
    ("both", "Graphify + facts", "Recommended for code-heavy projects"),
    ("mcp", "Custom MCP", "Any memory MCP server you run"),
];
pub const TEMPLATES: [&str; 4] = ["Software", "Research", "Writing", "Blank"];
pub const THEMES: [(&str, &str, &str); 2] = [("claude", "Claude", "warm cream, serif headings"), ("opencode", "opencode", "dark, monospace accents")];

pub fn agents_template(template: &str, name: &str) -> String {
    let body = match template {
        "Software" => "- Explain the plan before large edits.\n- Run the tests before calling a change done.\n- Keep commits small and described.\n",
        "Research" => "- Cite every source you use.\n- Separate what is known from what is guessed.\n- Summaries first, details after.\n",
        "Writing" => "- Plain words, short sentences.\n- Keep my voice; do not add hype.\n- Ask before cutting whole sections.\n",
        _ => "",
    };
    format!("# {name}\n{body}")
}

pub fn provider_title(kind: &str) -> &'static str {
    PROVIDERS.iter().find(|p| p.0 == kind).map_or("", |p| p.1)
}

/// The note under a provider card; Claude's depends on whether Claude Code is ready.
pub fn provider_note(kind: &str, claude_ready: bool, claude_installed: bool) -> &'static str {
    match kind {
        "claude" if claude_ready => "Pro / Max · via Claude Code",
        "claude" if claude_installed => "run “claude auth login” first",
        "claude" => "needs Claude Code installed",
        _ => PROVIDERS.iter().find(|p| p.0 == kind).map_or("", |p| p.2),
    }
}

/// The provider in effect: the picked one, else "From opencode" when sessions exist, else OpenCode Zen.
pub fn provider_kind(picked: Option<&str>, oc_has: bool, sessions: usize) -> String {
    picked.map(str::to_string).unwrap_or_else(|| if oc_has && sessions > 0 { "existing" } else { "opencode" }.into())
}

// ---- text helpers (same results as packages/shared/src/util.ts) ----

pub fn slugify(name: &str, fallback: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() { fallback.into() } else { out.into() }
}

pub fn initials_of(name: &str) -> String {
    let parts: Vec<&str> = name.split_whitespace().collect();
    match parts.len() {
        0 => "?".into(),
        1 => parts[0].chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(),
        _ => parts.iter().filter_map(|p| p.chars().next()).collect::<String>().chars().take(2).collect::<String>().to_uppercase(),
    }
}

pub fn title_case_segment(dir: &str) -> String {
    let seg = dir.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(dir);
    let mut c = seg.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

pub fn time_ago(ms: i64, now_ms: i64) -> String {
    let s = ((now_ms - ms).max(0) as f64 / 1000.0).round() as i64;
    if s < 60 {
        return "just now".into();
    }
    let m = (s as f64 / 60.0).round() as i64;
    if m < 60 {
        return format!("{m} min ago");
    }
    let h = (m as f64 / 60.0).round() as i64;
    if h < 24 {
        return format!("{h} h ago");
    }
    let d = (h as f64 / 24.0).round() as i64;
    match d {
        1 => "yesterday".into(),
        2..=6 => format!("{d} days ago"),
        7..=13 => "last week".into(),
        14..=30 => format!("{} weeks ago", (d as f64 / 7.0).round() as i64),
        31..=61 => "last month".into(),
        _ => format!("{} months ago", (d as f64 / 30.0).round() as i64),
    }
}

/// Each folder with its closest enclosing folder from the same list (shortest first).
pub fn nest_folders(dirs: &[String]) -> Vec<(String, Option<String>)> {
    let mut sorted: Vec<String> = Vec::new();
    for d in dirs {
        if !sorted.contains(d) {
            sorted.push(d.clone());
        }
    }
    sorted.sort_by_key(|d| d.len());
    sorted
        .iter()
        .map(|d| {
            let parent = sorted.iter().filter(|o| *o != d && d.starts_with(&format!("{}/", o.trim_end_matches('/')))).max_by_key(|o| o.len()).cloned();
            (d.clone(), parent)
        })
        .collect()
}

// ---- sessions ----

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Session {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub updated: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub dir: String,
    pub parent: Option<String>,
    /// Newest first.
    pub items: Vec<Session>,
}

pub fn group_sessions(sessions: &[Session]) -> Vec<Group> {
    let dirs: Vec<String> = sessions.iter().map(|s| s.dir.clone()).collect();
    nest_folders(&dirs)
        .into_iter()
        .map(|(dir, parent)| {
            let mut items: Vec<Session> = sessions.iter().filter(|s| s.dir == dir).cloned().collect();
            items.sort_by_key(|s| std::cmp::Reverse(s.updated));
            Group { dir, parent, items }
        })
        .collect()
}

/// Reply of `POST /api/onboarding/scan`: the sessions and an optional error.
pub fn parse_scan(reply: &Value) -> (Vec<Session>, String) {
    (serde_json::from_value(reply["sessions"].clone()).unwrap_or_default(), reply["error"].as_str().unwrap_or("").to_string())
}

pub fn target_text(dir: &str, parent: Option<&str>, on: bool) -> String {
    match (on, parent) {
        (false, _) => "skipped".into(),
        (_, Some(p)) => format!("sub-project “{}” inside {}", title_case_segment(dir), title_case_segment(p)),
        _ => format!("project “{}”", title_case_segment(dir)),
    }
}

/// Which folders and sessions the user switched off.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    dir_off: HashSet<String>,
    sess_off: HashSet<String>,
}

impl Selection {
    pub fn dir_on(&self, dir: &str) -> bool {
        !self.dir_off.contains(dir)
    }
    pub fn is_on(&self, s: &Session) -> bool {
        self.dir_on(&s.dir) && !self.sess_off.contains(&s.id)
    }
    pub fn toggle_dir(&mut self, dir: &str) {
        if !self.dir_off.remove(dir) {
            self.dir_off.insert(dir.to_string());
        }
    }
    pub fn toggle_session(&mut self, id: &str) {
        if !self.sess_off.remove(id) {
            self.sess_off.insert(id.to_string());
        }
    }
    pub fn select_all(&mut self) {
        self.dir_off.clear();
        self.sess_off.clear();
    }
    pub fn start_fresh(&mut self, sessions: &[Session]) {
        self.dir_off = sessions.iter().map(|s| s.dir.clone()).collect();
    }
    pub fn count(&self, sessions: &[Session]) -> usize {
        sessions.iter().filter(|s| self.is_on(s)).count()
    }
    pub fn ids(&self, sessions: &[Session], on: bool) -> Vec<String> {
        sessions.iter().filter(|s| self.is_on(s) == on).map(|s| s.id.clone()).collect()
    }
}

pub fn sessions_lead(oc_has: bool, remote: bool, url: &str, scanned: bool, found: usize, folders: usize) -> String {
    if !oc_has {
        return if remote { "Connect to a server on the previous step to see its sessions.".into() } else { "WorkspaceAI looks for sessions opencode already saved on this computer.".into() };
    }
    if !scanned {
        return "Looking for sessions…".into();
    }
    if found == 0 {
        return "No earlier sessions were found.".into();
    }
    let place = if remote { format!("on {url}") } else { "in this computer’s opencode data".into() };
    format!("Found {found} sessions in {folders} folders {place}. Each folder becomes a project (nested by path), and each session becomes a chat in it with its full history.")
}

pub fn import_count_text(n: usize) -> String {
    if n == 1 { "1 chat will be imported".into() } else { format!("{n} chats will be imported") }
}

// ---- checks against the supervisor ----

pub fn detect_path(binary: &str) -> String {
    format!("/api/onboarding/detect?binary={}", crate::api::pct(binary))
}
pub const SCAN_PATH: &str = "/api/onboarding/scan";
pub const TEST_REMOTE_PATH: &str = "/api/onboarding/test-remote";
pub const TEST_PROVIDER_PATH: &str = "/api/onboarding/test-provider";
pub const TEST_WEB_PATH: &str = "/api/onboarding/test-web";
pub const FINISH_PATH: &str = "/api/onboarding/finish";
pub const DEMO_PATH: &str = "/api/onboarding/demo";
pub const SUBS_PATH: &str = "/api/subscriptions?force=1";
pub const OD_STATUS_PATH: &str = "/api/opendesign/status";

pub fn scan_body(remote: bool, binary: &str, url: &str, user: &str, pass: &str) -> Value {
    if remote { json!({ "mode": "remote", "url": url, "username": user, "password": pass }) } else { json!({ "mode": "managed", "binary": binary }) }
}

pub fn remote_body(url: &str, user: &str, pass: &str) -> Value {
    json!({ "url": url, "username": user, "password": pass })
}

/// (ok, text) of `test-remote`.
pub fn remote_result(r: &Value) -> (bool, String) {
    if r["ok"].as_bool() == Some(true) {
        let n = |k: &str| r[k].as_i64().unwrap_or(0);
        (true, format!("Connected · opencode {} · {} sessions in {} folders · {} providers configured", r["version"].as_str().unwrap_or(""), n("sessions"), n("folders"), n("providers")))
    } else {
        (false, r["error"].as_str().filter(|e| !e.is_empty()).unwrap_or("Can’t reach that server").into())
    }
}

pub fn provider_body(kind: &str, key: &str, url: &str, model: &str) -> Value {
    json!({ "kind": kind, "key": key, "url": url, "model": model })
}

pub fn provider_result(r: &Value, kind: &str, ollama_picked: usize) -> (bool, String) {
    if r["ok"].as_bool() == Some(true) {
        let ms = r["ms"].as_i64().filter(|m| *m > 0);
        let n = r["models"].as_array().map_or(0, |m| m.len());
        (true, if kind == "ollama" { format!("Connected · {} models ready", if n > 0 { n } else { ollama_picked }) } else { format!("Connected{}", ms.map(|m| format!(" · {m} ms")).unwrap_or_default()) })
    } else {
        (false, r["error"].as_str().filter(|e| !e.is_empty()).unwrap_or("That doesn’t look right — check the key and try again.").into())
    }
}

pub fn web_body(mode: &str, provider: &str, key: &str, url: &str) -> Value {
    json!({ "mode": mode, "provider": provider, "key": key, "url": url })
}

pub fn web_result(r: &Value) -> (bool, String) {
    (r["ok"].as_bool() == Some(true), crate::workspace::check_text(r))
}

/// What the detect reply says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Detect {
    #[serde(default)]
    pub found: bool,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

pub fn detect_text(d: Option<&Detect>) -> String {
    match d {
        None => "looking for opencode…".into(),
        Some(d) if d.found => format!("found at {} · {}", d.path.as_deref().unwrap_or(""), d.version.as_deref().unwrap_or("")),
        Some(_) => "not found on this computer".into(),
    }
}

/// `GET /api/opendesign/status` before any workspace exists.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct OdStatus {
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub installing: bool,
    #[serde(default)]
    pub progress: Option<f64>,
    #[serde(default)]
    pub log: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl OdStatus {
    pub fn pct(&self) -> u32 {
        if self.installed { 100 } else { self.progress.unwrap_or(0.0).clamp(0.0, 100.0) as u32 }
    }
    pub fn text(&self, later: bool) -> String {
        if self.installed {
            format!("installed{}, one daemon per workspace", self.version.as_deref().filter(|v| !v.is_empty()).map(|v| format!(" · {v}")).unwrap_or_default())
        } else if self.installing {
            format!("installing… {}%", self.pct())
        } else if later {
            "will install later — agents can’t save artifacts until it is installed".into()
        } else {
            "not installed".into()
        }
    }
}

// ---- the wizard's gate and labels ----

/// What the Continue button needs to decide.
#[derive(Debug, Clone, Default)]
pub struct Gate {
    pub remote: bool,
    /// opencode found (managed) or connected (remote).
    pub oc_has: bool,
    pub od_ready: bool,
    pub ws_name: String,
    pub proj_name: String,
    pub proj_folder: String,
}

/// The hint that explains why the step cannot continue, or `None` when it can.
pub fn blocked(key: &str, g: &Gate) -> Option<&'static str> {
    match key {
        "engine" if g.remote && !g.oc_has => Some("Connect to the server first"),
        "engine" if !g.remote && !g.oc_has => Some("Install opencode, or connect to a server"),
        "engine" if !g.od_ready => Some("Install Open Design, or choose Install later"),
        "workspace" if g.ws_name.trim().is_empty() => Some("Give the workspace a name"),
        "project" if g.proj_name.trim().is_empty() || g.proj_folder.trim().is_empty() => Some("Name your project and pick a folder"),
        _ => None,
    }
}

pub fn next_label(step: usize, tested: bool, found: usize, selected: usize, web_off: bool) -> &'static str {
    let key = STEPS[step].0;
    if step == STEPS.len() - 1 {
        "Open WorkspaceAI"
    } else if step == 0 {
        "Get started"
    } else if key == "model" && !tested {
        "Set up later"
    } else if key == "sessions" && found > 0 && selected == 0 {
        "Start fresh"
    } else if key == "web" && web_off {
        "Skip"
    } else {
        "Continue"
    }
}

/// Everything `finish` sends.
#[derive(Debug, Clone)]
pub struct Draft {
    pub remote: bool,
    pub binary: String,
    pub url: String,
    pub user: String,
    pub pass: String,
    pub ws_name: String,
    pub accent: String,
    pub provider: String,
    pub key: String,
    pub endpoint: String,
    pub model: String,
    pub ollama: Vec<String>,
    pub web_mode: String,
    pub web_provider: String,
    pub web_key: String,
    pub searx: String,
    pub web_fetch: bool,
    pub web_ask: bool,
    pub memory: String,
    pub folders: Vec<String>,
    pub watch: bool,
    pub proj_name: String,
    pub proj_folder: String,
    pub agents: String,
    pub import_ids: Vec<String>,
    pub skipped_ids: Vec<String>,
    pub sync: bool,
    pub theme: String,
    pub reduce_motion: bool,
    pub follow_system: bool,
    pub subscriptions: Vec<String>,
}

pub fn finish_body(d: &Draft) -> Value {
    let claude = d.provider == "claude";
    let mut subs: Vec<String> = Vec::new();
    for s in d.subscriptions.iter().cloned().chain(claude.then(|| "claude".to_string())) {
        if !subs.contains(&s) {
            subs.push(s);
        }
    }
    let mut body = json!({
        "workspaceName": d.ws_name.trim(), "accent": d.accent,
        "engine": { "mode": if d.remote { "remote" } else { "managed" }, "url": d.url, "username": d.user, "password": d.pass, "binary": d.binary },
        "provider": if claude { json!({ "kind": "opencode" }) } else { json!({ "kind": d.provider, "key": d.key, "url": d.endpoint, "model": d.model, "ollamaModels": d.ollama }) },
        "web": { "mode": d.web_mode, "provider": d.web_provider, "searxngUrl": d.searx, "fetch": d.web_fetch, "ask": d.web_ask, "showSources": true, "locked": false, "key": d.web_key },
        "memory": d.memory, "graphifyFolders": if d.memory == "facts" { Vec::new() } else { d.folders.clone() }, "graphifyWatch": d.watch,
        "project": { "name": d.proj_name.trim(), "folder": d.proj_folder.trim(), "agents": d.agents },
        "importIds": d.import_ids, "skippedIds": d.skipped_ids, "sync": d.sync,
        "theme": d.theme, "reduceMotion": d.reduce_motion, "followSystem": d.follow_system,
        "subscriptions": subs,
    });
    if claude {
        body["defaultAgent"] = json!("claude");
    }
    body
}

pub fn finish_toast(reply: &Value) -> String {
    let n = reply["imported"]["chats"].as_i64().unwrap_or(0);
    if n > 0 { format!("Setup done — {n} chats imported") } else { "Setup done — welcome to WorkspaceAI".into() }
}

pub fn failed_toast(reply: &Value) -> Option<String> {
    let n = reply["imported"]["failed"].as_array().map_or(0, |f| f.len());
    (n > 0).then(|| format!("{n} sessions could not be copied"))
}

/// The Look step's summary rows.
pub fn summary(d: &Draft, remote_url: &str, od_ready: bool, selected: usize, provider_tested: bool) -> Vec<(&'static str, String)> {
    let web = match d.web_mode.as_str() { "off" => "off".to_string(), "exa" => "built-in (Exa)".into(), "mcp" => d.web_provider.clone(), _ => "SearXNG".into() };
    let mem = MEMORY.iter().find(|m| m.0 == d.memory).map_or("", |m| m.1).to_string();
    vec![
        ("Workspace", if d.ws_name.trim().is_empty() { "Personal".into() } else { d.ws_name.trim().into() }),
        ("opencode", if d.remote { format!("connected · {remote_url}") } else { "started by WorkspaceAI".into() }),
        ("Subscriptions", if d.subscriptions.is_empty() { "none yet".into() } else { d.subscriptions.join(", ") }),
        ("Open Design", if od_ready { "ready" } else { "install later" }.into()),
        ("Imported chats", selected.to_string()),
        ("Model", format!("{}{}", provider_title(&d.provider), if provider_tested { " · tested" } else { "" })),
        ("Web search", web),
        ("Memory", mem),
        ("First project", format!("{} · {}", d.proj_name, d.proj_folder)),
    ]
}
