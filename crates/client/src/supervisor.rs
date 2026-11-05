//! Starts the supervisor process and stops it again. Port of `apps/desktop/src-tauri/src/lib.rs`.
//! The supervisor prints `HARNESS_READY {"port":N,"token":"…"}` when it listens; closing its stdin is its cue to
//! shut its sidecars down.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub port: u16,
    pub token: String,
}

pub fn parse_ready(line: &str) -> Result<Info, String> {
    let json = line.trim().strip_prefix("HARNESS_READY ").ok_or("not a ready line")?;
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("ready line is not json: {e}"))?;
    let port = v["port"].as_u64().and_then(|p| u16::try_from(p).ok()).ok_or("the supervisor ready line has no port")?;
    let token = v["token"].as_str().ok_or("the supervisor ready line has no token")?.to_string();
    Ok(Info { port, token })
}

pub struct Supervisor {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Supervisor {
    /// Runs `<node> packages/supervisor/src/main.ts` in `root` and waits up to 30 s for its ready line.
    pub fn start(root: &Path, node: &Path) -> Result<(Info, Supervisor), String> {
        let mut child = Command::new(node)
            .arg("packages/supervisor/src/main.ts")
            .current_dir(root)
            .env("HARNESS_PARENT_STDIN", "1")
            .env_remove("HARNESS_PORT")
            .env_remove("HARNESS_TOKEN")
            .env_remove("HARNESS_DEV")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Could not start the supervisor: {e}"))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or("The supervisor has no stdout")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.starts_with("HARNESS_READY ") {
                    let _ = tx.send(line);
                    break;
                }
            }
            // Keep draining so the supervisor never blocks on a full pipe.
        });
        let sup = Supervisor { child, stdin };
        let line = rx.recv_timeout(Duration::from_secs(30)).map_err(|_| "The supervisor did not become ready".to_string())?;
        let info = parse_ready(&line)?;
        Ok((info, sup))
    }

}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stdin.take(); // the supervisor's cue to stop its sidecars
        unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
        let started = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if started.elapsed() < Duration::from_secs(8) => std::thread::sleep(Duration::from_millis(40)),
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
            }
        }
    }
}
