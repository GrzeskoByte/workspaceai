// Desktop window. Starts the supervisor, tells the UI its port and token, and
// stops that process on quit so workspace sidecars stop with the app.
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, RunEvent};

#[derive(Clone, serde::Serialize)]
struct SupervisorInfo {
    port: u16,
    token: String,
}

struct SupervisorProc {
    child: Child,
    stdin: Option<ChildStdin>,
    stopped: bool,
}

impl SupervisorProc {
    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        // Closing stdin is the supervisor's cue to shut its sidecars down.
        self.stdin.take();
        let pid = self.child.id() as i32;
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let started = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if started.elapsed() < Duration::from_secs(8) => {
                    std::thread::sleep(Duration::from_millis(40));
                }
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
            }
        }
    }
}

impl Drop for SupervisorProc {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Where the supervisor sources live: HARNESS_ROOT, the repo this binary was compiled from (dev builds), or the
/// bundled `app/` resource of a packaged build.
fn runtime_root(resource_dir: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Ok(given) = std::env::var("HARNESS_ROOT") {
        return Ok(PathBuf::from(given));
    }
    let compiled = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    // `tauri dev` also copies the last staged bundle next to the binary; a dev build must run the repo's
    // current sources, not that stale copy.
    if cfg!(debug_assertions) && compiled.join("packages/supervisor/src/main.ts").is_file() {
        return Ok(compiled);
    }
    if let Some(res) = resource_dir {
        let bundled = res.join("app");
        if bundled.join("packages/supervisor/src/main.ts").is_file() {
            return Ok(bundled);
        }
    }
    if compiled.join("packages/supervisor/src/main.ts").is_file() {
        return Ok(compiled);
    }
    Err("Cannot find the Harness runtime. Set HARNESS_ROOT.".into())
}

/// The Node that runs the supervisor: HARNESS_NODE, the `node` sidecar shipped next to this binary, or PATH.
fn node_binary() -> PathBuf {
    if let Ok(given) = std::env::var("HARNESS_NODE") {
        return PathBuf::from(given);
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let sidecar = dir.join("node");
        if sidecar.is_file() {
            return sidecar;
        }
    }
    PathBuf::from("node")
}

fn start_supervisor(root: &std::path::Path) -> Result<(SupervisorInfo, SupervisorProc), String> {
    let mut child = Command::new(node_binary())
        .arg("packages/supervisor/src/main.ts")
        .current_dir(root)
        .env("HARNESS_PARENT_STDIN", "1")
        .env_remove("HARNESS_DEV")
        .env_remove("HARNESS_PORT")
        .env_remove("HARNESS_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Could not start the supervisor: {e}"))?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().ok_or("The supervisor has no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut sent = false;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if !sent {
                if let Some(rest) = line.strip_prefix("HARNESS_READY ") {
                    sent = true;
                    let _ = tx.send(rest.to_string());
                }
            }
        }
    });
    let ready = rx
        .recv_timeout(Duration::from_secs(30))
        .map_err(|_| "The supervisor did not become ready".to_string())?;
    let value: serde_json::Value = serde_json::from_str(&ready).map_err(|e| e.to_string())?;
    let port = value["port"].as_u64().ok_or("The supervisor ready line has no port")? as u16;
    let token = value["token"]
        .as_str()
        .ok_or("The supervisor ready line has no token")?
        .to_string();
    Ok((
        SupervisorInfo { port, token },
        SupervisorProc { child, stdin, stopped: false },
    ))
}

#[tauri::command]
fn supervisor_info(info: tauri::State<SupervisorInfo>) -> SupervisorInfo {
    info.inner().clone()
}

pub fn run() {
    let proc: Arc<Mutex<Option<SupervisorProc>>> = Arc::new(Mutex::new(None));
    let started = proc.clone();
    let on_exit = proc.clone();
    tauri::Builder::default()
        .setup(move |app| {
            let root = runtime_root(app.path().resource_dir().ok())?;
            let (info, running) = start_supervisor(&root)?;
            app.manage(info);
            *started.lock().unwrap() = Some(running);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![supervisor_info])
        .build(tauri::generate_context!())
        .expect("Could not build the Harness window")
        .run(move |_app, event| {
            if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
                if let Some(mut running) = on_exit.lock().ok().and_then(|mut g| g.take()) {
                    running.stop();
                }
            }
        });
}
