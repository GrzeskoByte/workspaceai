//! Port of `ManagedServer`: one `opencode serve` per workspace on 127.0.0.1, random password in the env only,
//! logs to `<ws>/data/logs/opencode.log`, health poll on `/api/info`, stop by process group, idle watchdog.

use super::client::OpencodeClient;
use crate::jsonfile::append_line;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use workspaceai_shared::types::{EngineStatus, Sidecar};
use workspaceai_shared::util::now_ms;

const START_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_GRACE: Duration = Duration::from_secs(3);

pub struct ManagedOptions {
    pub binary: String,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub log_dir: PathBuf,
    pub idle_minutes: i64,
}

/// State the watchdog and the routes share; killing the process does not need the `Runtime` lock.
pub struct Shared {
    pid: AtomicI32,
    running: AtomicBool,
    stop: AtomicBool,
    last_used: Arc<AtomicI64>,
}

pub struct ManagedServer {
    pub port: u16,
    pub password: String,
    pub version: String,
    child: Option<Child>,
    opts: ManagedOptions,
    shared: Arc<Shared>,
}

pub fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn random_password() -> String {
    use base64::Engine as _;
    let mut buf = [0u8; 24];
    if getrandom::getrandom(&mut buf).is_err() {
        // Never fall back to a guessable password: let the caller fail the start instead.
        return String::new();
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

fn kill_group(pid: i32, signal: i32) {
    if pid > 0 {
        unsafe { libc::kill(-pid, signal) };
    }
}

/// Copies the child's output into the workspace log until the pipe closes.
fn drain(mut reader: impl Read + Send + 'static, log: PathBuf) {
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut chunk = String::new();
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            chunk.push_str(&String::from_utf8_lossy(&buf[..n]));
            while let Some(i) = chunk.find('\n') {
                let line: String = chunk.drain(..=i).collect();
                let _ = append_line(&log, line.trim_end_matches('\n'));
            }
        }
    });
}

/// 1 s, 2 s, 4 s … capped at 30 s (R-42 crash restarts).
pub fn next_backoff(restarts: u32) -> u64 {
    let exp = restarts.saturating_sub(1).min(5);
    (1000u64 * 2u64.pow(exp)).min(30_000)
}

/// The idle watchdog fires only when the workspace set a limit and nothing touched the engine since.
pub fn should_idle_stop(idle_minutes: i64, last_used: i64, now: i64) -> bool {
    idle_minutes > 0 && now.saturating_sub(last_used) >= idle_minutes * 60_000
}

impl ManagedServer {
    pub fn new(opts: ManagedOptions, last_used: Arc<AtomicI64>) -> Self {
        ManagedServer {
            port: 0,
            password: String::new(),
            version: String::new(),
            child: None,
            opts,
            shared: Arc::new(Shared { pid: AtomicI32::new(0), running: AtomicBool::new(false), stop: AtomicBool::new(false), last_used }),
        }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn client(&self) -> OpencodeClient {
        OpencodeClient::new(&self.url(), "opencode", &self.password)
    }

    pub fn log_path(&self) -> PathBuf {
        self.opts.log_dir.join("opencode.log")
    }

    pub fn pid(&self) -> Option<i32> {
        let pid = self.shared.pid.load(Ordering::SeqCst);
        (pid > 0).then_some(pid)
    }

    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::SeqCst) && self.pid().is_some_and(pid_alive)
    }

    /// `EngineStatus` as the UI expects it, derived from the live process.
    pub fn status(&self) -> EngineStatus {
        if self.is_running() {
            let pid = self.pid();
            return EngineStatus {
                state: "running".into(),
                url: self.url(),
                port: self.port as i64,
                version: self.version.clone(),
                pid: pid.map(|p| p as i64),
                error: None,
                sidecars: vec![Sidecar { name: "opencode serve".into(), state: "running".into(), pid: pid.map(|p| p as i64) }],
            };
        }
        EngineStatus { state: "stopped".into(), url: self.url(), port: self.port as i64, version: self.version.clone(), ..Default::default() }
    }

    /// Spawns and waits (max 30 s) until `/api/info` answers.
    pub fn start(&mut self) -> Result<(), String> {
        self.shared.stop.store(false, Ordering::SeqCst);
        self.port = free_port().map_err(|e| e.to_string())?;
        self.password = random_password();
        if self.password.is_empty() {
            return Err("could not generate the opencode server password".into());
        }
        std::fs::create_dir_all(&self.opts.log_dir).map_err(|e| e.to_string())?;
        let log = self.log_path();
        OpenOptions::new().create(true).append(true).open(&log).map_err(|e| e.to_string())?;

        let mut cmd = Command::new(&self.opts.binary);
        cmd.args(["serve", "--hostname", "127.0.0.1", "--port", &self.port.to_string()])
            .current_dir(&self.opts.cwd)
            .env_clear()
            .envs(&self.opts.env)
            .env("OPENCODE_SERVER_PASSWORD", &self.password)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Detached: the whole process group is stopped together (opencode spawns children).
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = cmd.spawn().map_err(|e| format!("opencode did not start ({}): {e}", self.opts.binary))?;
        let pid = child.id() as i32;
        if let Some(out) = child.stdout.take() {
            drain(out, log.clone());
        }
        if let Some(err) = child.stderr.take() {
            drain(err, log.clone());
        }
        self.child = Some(child);
        self.shared.pid.store(pid, Ordering::SeqCst);
        self.shared.running.store(true, Ordering::SeqCst);
        self.spawn_watchdog();

        let client = self.client();
        let deadline = Instant::now() + START_TIMEOUT;
        let mut last = "no answer".to_string();
        while Instant::now() < deadline {
            if let Some(ch) = self.child.as_mut() {
                if let Ok(Some(_)) = ch.try_wait() {
                    self.shared.running.store(false, Ordering::SeqCst);
                    self.child = None;
                    return Err(format!("opencode exited during start; see {}", log.display()));
                }
            }
            match client.info() {
                Ok(info) => {
                    self.version = info.version;
                    return Ok(());
                }
                Err(e) => last = e.message,
            }
            thread::sleep(Duration::from_millis(300));
        }
        self.stop();
        Err(format!("opencode did not become healthy: {last}"))
    }

    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let Some(mut child) = self.child.take() else {
            self.shared.running.store(false, Ordering::SeqCst);
            return;
        };
        let pid = child.id() as i32;
        kill_group(pid, libc::SIGTERM);
        let deadline = Instant::now() + STOP_GRACE;
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        if child.try_wait().ok().flatten().is_none() {
            kill_group(pid, libc::SIGKILL);
            let _ = child.wait();
        }
        self.shared.running.store(false, Ordering::SeqCst);
    }

    fn spawn_watchdog(&self) {
        let shared = self.shared.clone();
        let minutes = self.opts.idle_minutes;
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            if shared.stop.load(Ordering::SeqCst) || !shared.running.load(Ordering::SeqCst) {
                return;
            }
            if !should_idle_stop(minutes, shared.last_used.load(Ordering::SeqCst), now_ms()) {
                continue;
            }
            let pid = shared.pid.load(Ordering::SeqCst);
            kill_group(pid, libc::SIGTERM);
            thread::sleep(Duration::from_millis(1000));
            kill_group(pid, libc::SIGKILL);
            shared.running.store(false, Ordering::SeqCst);
            return;
        });
    }
}

impl Drop for ManagedServer {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn pid_alive(pid: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

/// Reads a file's last `n` lines, as `/engine/logs` returns them.
pub fn tail(path: &Path, n: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_watchdog_respects_the_limit() {
        assert!(!should_idle_stop(0, 0, i64::MAX / 2));
        assert!(!should_idle_stop(15, 1_000, 1_000 + 14 * 60_000));
        assert!(should_idle_stop(15, 1_000, 1_000 + 15 * 60_000));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(next_backoff(1), 1_000);
        assert_eq!(next_backoff(2), 2_000);
        assert_eq!(next_backoff(20), 30_000);
    }

    #[test]
    fn a_bufreader_is_not_needed_for_tail() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.log");
        std::fs::write(&file, "a\nb\nc").unwrap();
        assert_eq!(tail(&file, 2), "b\nc");
    }
}
