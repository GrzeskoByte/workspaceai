//! Port of `paths.ts`. The harness home is passed in (not read from the environment per call),
//! so tests in one process can each use their own.

use std::path::{Path, PathBuf};

fn user_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".into())
}

/// `~/.harness`, overridable for tests and dev (`HARNESS_HOME`).
pub fn harness_home() -> PathBuf {
    match std::env::var("HARNESS_HOME") {
        Ok(h) if !h.is_empty() => PathBuf::from(h),
        _ => PathBuf::from(user_home()).join(".harness"),
    }
}

pub fn expand_home(p: &str) -> String {
    if p == "~" {
        return user_home();
    }
    match p.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", user_home()),
        None => p.to_string(),
    }
}

pub fn contract_home(p: &str) -> String {
    let h = user_home();
    match p.strip_prefix(&(h.clone() + "/")) {
        Some(rest) => format!("~/{rest}"),
        None => p.to_string(),
    }
}

/// An id that is safe as one path segment and one folder name: not empty, not `.`/`..`, no separators.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && !id.contains(['/', '\\', '\0'])
}

#[derive(Debug, Clone)]
pub struct WsPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub opencode_json: PathBuf,
    pub harness_json: PathBuf,
    pub policy_json: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub xdg_data: PathBuf,
    pub xdg_config: PathBuf,
    pub xdg_state: PathBuf,
    pub xdg_cache: PathBuf,
    pub open_design: PathBuf,
    pub memory: PathBuf,
    pub audit: PathBuf,
    pub runs: PathBuf,
    pub browser_profile: PathBuf,
    pub logs: PathBuf,
    /// Per-session launch files (`<sid>.mcp.json` for claude).
    pub run: PathBuf,
    /// Per-agent homes (`CLAUDE_CONFIG_DIR`, `GROK_HOME`, Pi's agent folder).
    pub agents: PathBuf,
}

impl WsPaths {
    pub fn new(home: &Path, id: &str) -> Self {
        let root = home.join("ws").join(id);
        let config = root.join("config");
        let data = root.join("data");
        WsPaths {
            opencode_json: config.join("opencode.json"),
            harness_json: config.join("harness.json"),
            policy_json: config.join("policy.json"),
            state: data.join("state.json"),
            xdg_data: data.join("opencode/xdg-data"),
            xdg_config: config.join("xdg"),
            xdg_state: data.join("opencode/xdg-state"),
            xdg_cache: data.join("opencode/xdg-cache"),
            open_design: data.join("open-design"),
            memory: root.join("memory"),
            audit: data.join("audit.jsonl"),
            runs: data.join("runs"),
            browser_profile: data.join("browser-profile"),
            logs: data.join("logs"),
            run: root.join("run"),
            agents: data.join("agents"),
            root,
            config,
            data,
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for d in [&self.config, &self.data, &self.xdg_data, &self.xdg_config, &self.xdg_state, &self.xdg_cache, &self.open_design, &self.memory, &self.runs, &self.browser_profile, &self.logs, &self.run] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}

pub fn app_json_path(home: &Path) -> PathBuf {
    home.join("app.json")
}

pub fn computers_json_path(home: &Path) -> PathBuf {
    home.join("computers.json")
}
