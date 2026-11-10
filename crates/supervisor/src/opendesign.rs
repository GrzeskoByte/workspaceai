//! Open Design: the install, the per-workspace daemon, and the artifact calls that go through it. Port of
//! `opendesign.ts` (B10a). One install per harness home; one daemon per workspace, bound to 127.0.0.1.
//!
//! ponytail: HTTP is blocking (ureq) and runs under the supervisor lock, as `upload_audit` does; the daemon health
//! wait (30 s worst case) holds the lock too. Move to a worker when the scheduler calls these (B11).

use crate::artifacts::{file_base, FileBody, NewArtifact};
use crate::engine::free_port;
use crate::engine::lifecycle::base_env;
use crate::formats::to_html;
use crate::paths::{expand_home, valid_id};
use crate::runtime::Runtime;
use crate::supervisor::ApiError;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use workspaceai_shared::defaults::od_settings;
use workspaceai_shared::types::{Artifact, ArtifactFormat, Artifact as _A, LintFinding, OdEngine, OdRef};
use workspaceai_shared::util::now_ms;

pub const REPO: &str = "https://github.com/nexu-io/open-design.git";
const PNPM: &str = "pnpm@10.33.2";
/// The plain-node entry shipped in this repo (`resources/od-host.mjs`), compiled in as its checked-in path.
const HOST_SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/od-host.mjs");
/// Loads Open Design's deck framework and prints `{directive, skeleton}`; the path comes in `OD_FRAMEWORK`.
const DECK_SCRIPT: &str = "(async()=>{const {pathToFileURL}=require('url');const m=await import(pathToFileURL(process.env.OD_FRAMEWORK).href);\
process.stdout.write(JSON.stringify({directive:String(m.renderDeckFrameworkDirective('text_artifact')).split(/\\n## Final handoff/)[0].trim(),skeleton:String(m.DECK_SKELETON_HTML)}));})().catch(()=>process.exit(1))";

/// What Settings › Open Design shows about the install (`state`: idle | installing | error).
#[derive(Debug, Clone, Default, Serialize)]
pub struct InstallState {
    pub state: String,
    pub step: String,
    pub progress: u32,
    pub error: Option<String>,
}

/// `apps/daemon/dist/prompts/deck-framework.js`, read once: the deck directive and the skeleton page.
#[derive(Debug, Clone)]
pub struct Framework {
    pub directive: String,
    pub skeleton: String,
}

/// A running `node` daemon, spawned in its own process group so stopping it takes its children too.
pub struct OdDaemon {
    child: Child,
    pub url: String,
}

impl OdDaemon {
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// SIGTERM to the group, 3 s to exit, then SIGKILL. Idempotent: a reaped child returns at once.
    pub fn stop(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        let pid = self.child.id() as i32;
        // SAFETY: `pid` is our child, spawned with process_group(0), so `-pid` names its own group.
        unsafe { libc::kill(-pid, libc::SIGTERM) };
        for _ in 0..30 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // SAFETY: as above.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

impl Drop for OdDaemon {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The install and the cached deck framework. Lives on `Core`; the daemons live on each `Runtime`.
#[derive(Default)]
pub struct OdManager {
    install: Arc<Mutex<InstallState>>,
    framework: Option<Framework>,
}

/// `HARNESS_OD_DIR`, else `<home>/vendor/open-design`.
pub fn install_dir(home: &Path) -> PathBuf {
    match std::env::var("HARNESS_OD_DIR").ok().filter(|v| !v.is_empty()) {
        Some(v) => PathBuf::from(expand_home(&v)),
        None => home.join("vendor/open-design"),
    }
}

/// `OD_BIN`, else the built CLI in the checkout.
pub fn cli_path(home: &Path) -> PathBuf {
    match std::env::var("OD_BIN").ok().filter(|v| !v.is_empty()) {
        Some(v) => PathBuf::from(v),
        None => install_dir(home).join("apps/daemon/dist/cli.js"),
    }
}

pub fn installed(home: &Path) -> bool {
    cli_path(home).exists()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs one install step, appending its output to the install log. Errors name the command.
fn run(log: &Path, cwd: Option<&Path>, program: &str, args: &[&str]) -> Result<(), String> {
    let mut out = std::fs::OpenOptions::new().create(true).append(true).open(log).map_err(|e| e.to_string())?;
    let _ = writeln!(out, "$ {program} {}", args.join(" "));
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(program);
    cmd.args(args).env_clear().envs(base_env()).stdin(Stdio::null()).stdout(out).stderr(err);
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    let status = cmd.status().map_err(|e| format!("{program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} exited with {status}", args.join(" ")))
    }
}

/// Clone or pull, then pnpm install and the daemon build. Progress: 25 / 75 / 98, then 100 on success.
fn build(dir: &Path, log: &Path, slot: &Mutex<InstallState>) -> Result<(), String> {
    let step = |name: &str, pct: u32| {
        let mut s = lock(slot);
        s.step = name.into();
        s.progress = pct;
    };
    if dir.join(".git").exists() {
        step("pulling", 25);
        run(log, Some(dir), "git", &["pull", "--ff-only"])?;
    } else {
        step("cloning", 25);
        if dir.exists() {
            return Err(format!("{} exists and is not an Open Design checkout", dir.display()));
        }
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        run(log, None, "git", &["clone", "--depth", "1", REPO, &dir.to_string_lossy()])?;
    }
    step("installing", 75);
    run(log, Some(dir), "npx", &["-y", PNPM, "install", "--frozen-lockfile"])?;
    step("building", 98);
    run(log, Some(dir), "npx", &["-y", PNPM, "--filter", "@open-design/daemon", "build"])
}

/// One JSON call to the daemon. Non-2xx answers carry the daemon's `error` text.
fn call(url: &str, path: &str, body: Option<Value>, secs: u64) -> Result<Value, ApiError> {
    let full = format!("{url}{path}");
    let timeout = Duration::from_secs(secs);
    let res = match &body {
        None => ureq::get(&full).timeout(timeout).call(),
        Some(b) => ureq::post(&full).timeout(timeout).set("content-type", "application/json").send_string(&b.to_string()),
    };
    match res {
        Ok(r) => {
            let text = r.into_string().map_err(|e| ApiError::new(502, e.to_string()))?;
            if text.trim().is_empty() {
                return Ok(Value::Null);
            }
            serde_json::from_str(&text).map_err(|e| ApiError::new(502, format!("Open Design sent bad JSON: {e}")))
        }
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"].as_str().map(str::to_string));
            Err(ApiError::bad_request(msg.unwrap_or_else(|| format!("Open Design answered {code}"))))
        }
        Err(e) => Err(ApiError::new(502, format!("Open Design is unreachable: {e}"))),
    }
}

/// A GET that answers `None` on any failure (`od()` in `design-guide.ts`).
fn get_text(url: &str, path: &str) -> Option<String> {
    ureq::get(&format!("{url}{path}")).timeout(Duration::from_secs(8)).call().ok()?.into_string().ok()
}

fn get_json(url: &str, path: &str) -> Option<Value> {
    serde_json::from_str(&get_text(url, path)?).ok()
}

fn health_ok(url: &str) -> bool {
    ureq::get(&format!("{url}/api/health")).timeout(Duration::from_secs(2)).call().is_ok()
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

impl OdManager {
    pub fn install_state(&self) -> InstallState {
        lock(&self.install).clone()
    }

    fn installing(&self) -> bool {
        self.install_state().state == "installing"
    }

    /// Starts the clone-or-pull and build on a worker thread. Refused while one is running.
    pub fn start_install(&self, home: &Path) -> Result<(), ApiError> {
        if self.installing() {
            return Err(ApiError::bad_request("Open Design is already installing"));
        }
        *lock(&self.install) = InstallState { state: "installing".into(), step: "starting".into(), progress: 0, error: None };
        let (dir, log, slot) = (install_dir(home), home.join("open-design-install.log"), self.install.clone());
        std::thread::spawn(move || {
            let result = build(&dir, &log, &slot);
            let mut s = lock(&slot);
            *s = match result {
                Ok(()) => InstallState { state: "idle".into(), step: "ready".into(), progress: 100, error: None },
                Err(e) => InstallState { state: "error".into(), step: s.step.clone(), progress: s.progress, error: Some(e) },
            };
        });
        Ok(())
    }

    /// `update`: a pull of an existing checkout, so it refuses when nothing is installed.
    pub fn start_update(&self, home: &Path) -> Result<(), ApiError> {
        if !install_dir(home).join(".git").exists() {
            return Err(ApiError::bad_request("Open Design is not installed — install it first"));
        }
        self.start_install(home)
    }

    /// The install state without a workspace (`GET /api/opendesign/status`).
    pub fn global_status(&self, home: &Path) -> Value {
        json!({ "installed": installed(home), "installDir": install_dir(home), "install": self.install_state() })
    }

    /// Settings › Open Design for one workspace: install, engine choice, and the daemon's state.
    pub fn ws_status(&self, rt: &mut Runtime) -> Value {
        let daemon = if !installed(&rt.home) {
            "not-installed"
        } else if rt.od_url().is_some() {
            "running"
        } else if rt.od_error.is_some() {
            "error"
        } else {
            "stopped"
        };
        json!({
            "installed": installed(&rt.home),
            "installDir": install_dir(&rt.home),
            "install": self.install_state(),
            "engine": od_settings(&rt.ws).engine,
            "lint": od_settings(&rt.ws).lint,
            "state": daemon,
            "url": rt.od_url(),
            "error": rt.od_error,
        })
    }

    /// The deck framework, read once from the install (`deckFramework`). None when nothing is installed.
    pub fn deck_framework(&mut self, home: &Path) -> Option<Framework> {
        if let Some(f) = &self.framework {
            return Some(f.clone());
        }
        let entry = install_dir(home).join("apps/daemon/dist/prompts/deck-framework.js");
        if !entry.exists() {
            return None;
        }
        let out = Command::new("node").arg("-e").arg(DECK_SCRIPT).env_clear().envs(base_env()).env("OD_FRAMEWORK", &entry).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let v: Value = serde_json::from_slice(&out.stdout).ok()?;
        let f = Framework { directive: v["directive"].as_str()?.to_string(), skeleton: v["skeleton"].as_str()?.to_string() };
        self.framework = Some(f.clone());
        Some(f)
    }

    /// The deck skeleton for `to_html`, or None when the framework is not installed.
    pub fn skeleton(&mut self, rt: &Runtime) -> Option<String> {
        self.deck_framework(&rt.home).map(|f| f.skeleton)
    }

    /// Open Design's base URL for an artifact call, starting the daemon when needed (`require`).
    pub fn require(&self, rt: &mut Runtime) -> Result<String, ApiError> {
        if self.installing() {
            return Err(ApiError::bad_request("Open Design is still installing — try again when Settings › Open Design shows it ready"));
        }
        if !installed(&rt.home) {
            return Err(ApiError::bad_request("Open Design is not installed. Install it in Settings › Open Design (or switch this workspace to the built-in artifact store there)."));
        }
        rt.od_ensure().map_err(|e| ApiError::bad_request(format!("Open Design did not start: {}", e.message)))
    }

    /// Artifact writes wait for Open Design when this workspace uses it; the built-in store never does.
    fn ready(&self, rt: &mut Runtime) -> Result<(), ApiError> {
        if od_settings(&rt.ws).engine == OdEngine::OpenDesign {
            self.require(rt)?;
        }
        Ok(())
    }

    /// `artifact_create` / the library: ready, save, then sync the version to Open Design.
    pub fn create(&mut self, rt: &mut Runtime, o: NewArtifact) -> Result<Artifact, ApiError> {
        self.ready(rt)?;
        let a = rt.create_artifact(o)?;
        self.sync(rt, &a.id)?;
        Ok(rt.artifact(&a.id)?.clone())
    }

    pub fn revise(&mut self, rt: &mut Runtime, id: &str, content: &str, chat: Option<String>, note: Option<String>, title: Option<String>, format: Option<String>) -> Result<Artifact, ApiError> {
        self.ready(rt)?;
        rt.revise_artifact(id, content, chat, note, title, format)?;
        self.sync(rt, id)?;
        Ok(rt.artifact(id)?.clone())
    }

    pub fn restore(&mut self, rt: &mut Runtime, id: &str, v: i64) -> Result<Artifact, ApiError> {
        self.ready(rt)?;
        rt.restore_artifact(id, v)?;
        self.sync(rt, id)?;
        Ok(rt.artifact(id)?.clone())
    }

    /// Saves the latest version to Open Design (`syncToOpenDesign`). Ok(false) on the built-in store, which keeps
    /// `od: null`. A failure keeps the harness copy, records `od: null` and says how to resync.
    pub fn sync(&mut self, rt: &mut Runtime, aid: &str) -> Result<bool, ApiError> {
        if od_settings(&rt.ws).engine != OdEngine::OpenDesign {
            rt.set_latest_od(aid, None)?;
            return Ok(false);
        }
        let url = self.require(rt)?;
        let (a, v, content, format) = rt.read_artifact(aid, None)?;
        let skeleton = self.skeleton(rt);
        let html = to_html(&a, &content, false, &format, skeleton.as_deref());
        let lint_on = od_settings(&rt.ws).lint;
        let markdown = (format == ArtifactFormat::Markdown).then_some(content.as_str());
        match save_version(&url, rt, &a, &html, markdown, lint_on) {
            Ok(od) => {
                let n = od.lint.len();
                let title = a.title.clone();
                rt.set_latest_od(aid, Some(od))?;
                rt.audit("open-design.save", &format!("{title} v{v} · {n} lint finding(s)"), "workspaceai");
                rt.changed()?;
                Ok(true)
            }
            Err(e) => {
                rt.set_latest_od(aid, None)?;
                let msg = format!(
                    "Open Design could not save “{}” v{v}: {}. WorkspaceAI kept a copy; resync it from Settings › Open Design.",
                    a.title, e.message
                );
                rt.audit("open-design.error", &msg, "workspaceai");
                Err(ApiError::bad_request(msg))
            }
        }
    }

    /// Syncs every artifact whose latest version has no Open Design copy (`resyncOpenDesign`).
    pub fn resync(&mut self, rt: &mut Runtime) -> Result<Value, ApiError> {
        self.require(rt)?;
        let pending: Vec<(String, String)> = rt
            .state
            .artifacts
            .iter()
            .filter(|a| a.versions.iter().find(|v| v.v == a.version).and_then(|v| v.od.clone()).flatten().is_none())
            .map(|a| (a.id.clone(), a.title.clone()))
            .collect();
        let (mut synced, mut failed) = (0, Vec::new());
        for (id, title) in pending {
            match self.sync(rt, &id) {
                Ok(_) => synced += 1,
                Err(e) => failed.push(json!({ "title": title, "error": e.message })),
            }
        }
        Ok(json!({ "synced": synced, "failed": failed }))
    }

    /// Export through Open Design when the latest version has its copy there (html, pdf, pptx); otherwise the
    /// harness export. Returns the file and whether Open Design made it.
    pub fn export(&mut self, rt: &mut Runtime, aid: &str, format: &str, v: Option<i64>) -> Result<(FileBody, bool), ApiError> {
        let a = rt.artifact(aid)?.clone();
        let od_ref = a.versions.iter().find(|x| x.v == a.version).and_then(|x| x.od.clone().flatten());
        let latest = v.is_none() || v == Some(a.version);
        if let (Some(r), true, true) = (od_ref, latest, matches!(format, "html" | "pdf" | "pptx")) {
            let url = self.require(rt)?;
            let base = file_base(&a.title);
            let ext = if format == "html" { "html" } else { format };
            let name = format!("{base}-v{}.{ext}", a.version);
            let body = json!({ "fileName": name, "title": a.title, "deck": a.kind == workspaceai_shared::types::ArtifactType::Deck });
            let res = call(&url, &format!("/api/projects/{}/export/{format}", r.project), Some(body), 180)?;
            if res["ok"] == json!(false) {
                return Err(ApiError::bad_request(res["error"].as_str().unwrap_or("Open Design could not export this").to_string()));
            }
            let bytes = if let Some(html) = res["html"].as_str() {
                html.as_bytes().to_vec()
            } else if let Some(path) = res["path"].as_str() {
                std::fs::read(path)?
            } else {
                return Err(ApiError::new(502, "Open Design returned no file"));
            };
            let mime = match format {
                "pdf" => "application/pdf",
                "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                _ => "text/html",
            };
            rt.audit("artifact.export", &format!("{name} via open-design"), "you");
            return Ok((FileBody { filename: name, mime: mime.into(), body: bytes }, true));
        }
        Ok((rt.export_artifact(aid, format, v, self.skeleton(rt).as_deref())?, false))
    }
}

/// Imports the artifact folder as an Open Design project once; reuses it while its `baseDir` still matches.
fn project_for(url: &str, rt: &mut Runtime, project: &str) -> Result<String, ApiError> {
    let p = rt.project(project).ok_or_else(|| ApiError::bad_request("Project not found"))?;
    let name = p.name.clone();
    let base = PathBuf::from(expand_home(&p.folder)).join(".harness/artifacts");
    std::fs::create_dir_all(&base)?;
    let base = base.to_string_lossy().into_owned();
    if let Some(id) = rt.state.od_projects.as_ref().and_then(|m| m.get(project)).cloned() {
        let meta = get_json(url, &format!("/api/projects/{id}"));
        if meta.and_then(|m| m["metadata"]["baseDir"].as_str().map(str::to_string)).as_deref() == Some(base.as_str()) {
            return Ok(id);
        }
    }
    let res = call(url, "/api/import/folder", Some(json!({ "baseDir": base, "name": name })), 30)?;
    let id = res["project"]["id"].as_str().ok_or_else(|| ApiError::new(502, "Open Design did not return a project"))?.to_string();
    rt.state.od_projects.get_or_insert_with(Default::default).insert(project.to_string(), id.clone());
    rt.save()?;
    Ok(id)
}

/// Lints the page (when on), then writes `<artifact>/index.html` (and `source.md` for Markdown) into the project.
fn save_version(url: &str, rt: &mut Runtime, a: &Artifact, html: &str, markdown: Option<&str>, lint_on: bool) -> Result<OdRef, ApiError> {
    let project = project_for(url, rt, &a.project)?;
    let lint: Vec<LintFinding> = if lint_on {
        let res = call(url, "/api/artifacts/lint", Some(json!({ "html": html })), 30)?;
        let list = if res["findings"].is_array() { res["findings"].clone() } else { res };
        serde_json::from_value(list).unwrap_or_default()
    } else {
        Vec::new()
    };
    let file = format!("{}/index.html", a.id);
    put_file(url, &project, &file, html)?;
    if let Some(md) = markdown {
        put_file(url, &project, &format!("{}/source.md", a.id), md)?;
    }
    Ok(OdRef { project, file, lint })
}

fn put_file(url: &str, project: &str, name: &str, content: &str) -> Result<(), ApiError> {
    call(url, &format!("/api/projects/{project}/files"), Some(json!({ "name": name, "content": content, "encoding": "utf8", "versionSource": "workspaceai" })), 30)?;
    Ok(())
}

impl Runtime {
    /// The running daemon's URL. A daemon that died is forgotten here.
    pub fn od_url(&mut self) -> Option<String> {
        if let Some(d) = self.od_daemon.as_mut() {
            if d.alive() {
                return Some(d.url.clone());
            }
        }
        self.od_daemon = None;
        None
    }

    /// The running daemon, or a new one: `node od-host.mjs <dir> <port>` when the daemon build is there, else the CLI.
    pub fn od_ensure(&mut self) -> Result<String, ApiError> {
        if let Some(url) = self.od_url() {
            return Ok(url);
        }
        match self.od_spawn() {
            Ok(d) => {
                let url = d.url.clone();
                self.od_daemon = Some(d);
                self.od_error = None;
                self.audit("open-design.start", &url, "workspaceai");
                Ok(url)
            }
            Err(e) => {
                self.od_error = Some(e.clone());
                self.audit("open-design.error", &e, "workspaceai");
                Err(ApiError::bad_request(e))
            }
        }
    }

    /// Spawns the daemon and waits up to 30 s (60 × 500 ms) for `/api/health`. A daemon that never answers is stopped.
    fn od_spawn(&self) -> Result<OdDaemon, String> {
        let dir = install_dir(&self.home);
        let port = free_port().map_err(|e| e.to_string())?;
        let url = format!("http://127.0.0.1:{port}");
        let log_path = self.paths.logs.join("open-design.log");
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| e.to_string())?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let host = PathBuf::from(HOST_SCRIPT);
        let mut cmd = Command::new("node");
        if host.exists() && dir.join("apps/daemon/dist/daemon-startup.js").exists() {
            cmd.arg(&host).arg(&dir).arg(port.to_string());
        } else {
            cmd.arg(cli_path(&self.home)).args(["--port", &port.to_string(), "--host", "127.0.0.1"]);
        }
        let mut env = base_env();
        env.insert("OD_DATA_DIR".into(), self.paths.open_design.to_string_lossy().into_owned());
        env.insert("OD_HOST".into(), "127.0.0.1".into());
        env.insert("OD_PORT".into(), port.to_string());
        cmd.env_clear().envs(env).current_dir(&dir).stdin(Stdio::null()).stdout(log).stderr(err).process_group(0);
        let child = cmd.spawn().map_err(|e| format!("node: {e}"))?;
        let mut d = OdDaemon { child, url: url.clone() };
        for _ in 0..60 {
            if !d.alive() {
                return Err("The daemon exited at startup — see its log".into());
            }
            if health_ok(&url) {
                return Ok(d);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        d.stop();
        Err("The daemon did not become healthy within 30 s — see its log".into())
    }

    /// Stops this workspace's daemon, if any. Also runs when the runtime is dropped.
    pub fn stop_od_daemon(&mut self) {
        if let Some(mut d) = self.od_daemon.take() {
            d.stop();
        }
    }
}

/// Used by the OD routes: the log's last lines.
pub fn od_log(rt: &Runtime) -> String {
    crate::engine::server::tail(&rt.paths.logs.join("open-design.log"), 500)
}

/// Where a shared or exported page's timestamp comes from; kept here so OD exports and harness exports agree.
pub fn stamp() -> i64 {
    now_ms()
}

/// `Artifact` re-export guard: the artifact type is referenced by the export path only.
#[allow(dead_code)]
fn _kind(a: &Artifact) -> &workspaceai_shared::types::ArtifactType {
    &a.kind
}

/// Ids the routes accept for skills: the same safe segment rule as workspace ids.
pub fn safe_skill_id(id: &str) -> bool {
    valid_id(id)
}
