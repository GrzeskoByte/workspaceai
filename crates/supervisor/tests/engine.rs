//! Port of `test/engine.test.ts` and the `remoteUrlAllowed` cases of `security.test.ts` (R-03, R-35).
//! Tests that need a real binary skip with a printed reason when `opencode` is not on PATH; every wait is bounded.

mod common;

use common::{create, setup};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};
use workspaceai_supervisor::engine::detect::{find_opencode, pinned_opencode_bin};
use workspaceai_supervisor::engine::remote::remote_url_allowed;
use workspaceai_supervisor::engine::server::pid_alive;
use workspaceai_supervisor::paths::WsPaths;

fn system_version() -> Option<String> {
    match find_opencode("opencode") {
        Some(found) => Some(found.version),
        None => {
            eprintln!("skipping: opencode is not on PATH (engine tests need the real binary)");
            None
        }
    }
}

fn wait_for_death(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// SSRF table from `security.test.ts` — no binary needed.
#[test]
fn remote_servers_must_be_https_or_loopback() {
    assert!(remote_url_allowed("https://oc.example.com").is_ok());
    assert!(remote_url_allowed("http://127.0.0.1:4096").is_ok());
    assert!(remote_url_allowed("http://localhost:4096").is_ok());
    assert!(remote_url_allowed("http://10.0.0.5:4096").is_err());
    assert!(remote_url_allowed("http://example.com").is_err());
    assert!(remote_url_allowed("ftp://example.com").is_err());
    assert!(remote_url_allowed("http://user:pw@example.com").is_err());
    assert_eq!(remote_url_allowed("http://10.0.0.5:4096").unwrap_err(), "Remote servers must be reached over HTTPS or an SSH tunnel to localhost");
}

#[test]
fn a_pin_that_is_not_installed_is_refused_then_the_pinned_binary_runs() {
    let Some(version) = system_version() else { return };
    // `opencode --version` prints `v2.2.5`; the pin stores the bare release.
    let version = version.trim_start_matches('v').to_string();
    let f = setup();
    let ws = create(&f, "Pin", "personal");
    let route = format!("/api/ws/{ws}/engine/version");

    let err = f.call("POST", &route, Some(json!({ "version": version }))).unwrap_err();
    assert_eq!(err.status, 409);
    assert_eq!(err.message, format!("opencode {version} is not installed yet"));

    // Stand in for `npm install opencode-ai@<version>`: the same unmodified binary at the pinned path.
    let found = find_opencode("opencode").expect("checked above");
    let bin = pinned_opencode_bin(f.path(), &version);
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&found.path, &bin).unwrap();
    let pinned = f.ok("POST", &route, Some(json!({ "version": version })));
    assert_eq!(pinned["version"], version);

    let status = f.ok("POST", &format!("/api/ws/{ws}/engine/start"), None);
    assert_eq!(status["state"], "running");
    assert_eq!(status["version"], version);
    assert!(status["pid"].as_i64().unwrap() > 0);

    f.ok("POST", &route, Some(json!({ "version": "system" })));
    let back = f.ok("GET", &format!("/api/ws/{ws}/snapshot"), None);
    assert_eq!(back["workspace"]["engine"]["version"], "system");
    f.sup.shutdown().unwrap();
}

#[test]
fn two_workspaces_get_their_own_xdg_dirs_and_ports() {
    if system_version().is_none() {
        return;
    }
    let f = setup();
    let a = create(&f, "One", "personal");
    let b = create(&f, "Two", "personal");
    let sa = f.ok("POST", &format!("/api/ws/{a}/engine/start"), None);
    let sb = f.ok("POST", &format!("/api/ws/{b}/engine/start"), None);
    assert_eq!(sa["state"], "running");
    assert_eq!(sb["state"], "running");
    assert_ne!(sa["port"], sb["port"], "two engines cannot share a port");
    assert_ne!(sa["pid"], sb["pid"]);

    let pa = WsPaths::new(f.path(), &a);
    let pb = WsPaths::new(f.path(), &b);
    for (x, y) in [(&pa.xdg_data, &pb.xdg_data), (&pa.xdg_config, &pb.xdg_config), (&pa.xdg_state, &pb.xdg_state), (&pa.xdg_cache, &pb.xdg_cache)] {
        assert_ne!(x, y);
        assert!(x.exists(), "{} missing", x.display());
        assert!(y.exists(), "{} missing", y.display());
    }
    let env = f.sup.with_core(|core| core.rt(&a).unwrap().engine_env()).unwrap();
    assert_eq!(env["XDG_DATA_HOME"].as_str(), pa.xdg_data.to_string_lossy().as_ref());
    assert!(Path::new(&env["OPENCODE_CONFIG"]).ends_with("config/opencode.json"));
    // Org isolation (R-06): nothing from another workspace's environment.
    assert!(!env["XDG_DATA_HOME"].contains(pb.xdg_data.to_string_lossy().as_ref()));

    assert_eq!(f.ok("POST", &format!("/api/ws/{a}/engine/stop"), None)["state"], "stopped");
    assert_eq!(f.ok("GET", &format!("/api/ws/{a}/engine"), None)["state"], "stopped");
    f.sup.shutdown().unwrap();
    assert_eq!(f.ok("GET", &format!("/api/ws/{b}/engine"), None)["state"], "stopped");
}

#[test]
fn the_server_password_lives_in_the_env_and_never_in_argv() {
    if system_version().is_none() {
        return;
    }
    let f = setup();
    let ws = create(&f, "Secret env", "personal");
    f.ok("POST", &format!("/api/ws/{ws}/engine/start"), None);
    let password = f.sup.with_core(|core| core.keychain.get(&ws, "OPENCODE_ENGINE_PASSWORD")).unwrap().expect("engine password");
    let pid = f.ok("GET", &format!("/api/ws/{ws}/engine"), None)["pid"].as_i64().unwrap() as i32;
    let argv = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let environ = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    assert!(!String::from_utf8_lossy(&argv).contains(&password), "password leaked into argv");
    assert!(String::from_utf8_lossy(&environ).contains(&format!("OPENCODE_SERVER_PASSWORD={password}")));
    // The logs route answers even while the server is young.
    assert!(f.ok("GET", &format!("/api/ws/{ws}/engine/logs"), None).get("text").is_some());
    f.sup.shutdown().unwrap();
}

#[test]
fn shutdown_stops_every_managed_server_and_is_idempotent() {
    if system_version().is_none() {
        return;
    }
    let f = setup();
    let ws = create(&f, "Quit", "personal");
    let status = f.ok("POST", &format!("/api/ws/{ws}/engine/start"), None);
    let pid = status["pid"].as_i64().unwrap() as i32;
    assert!(pid_alive(pid));
    f.sup.shutdown().unwrap();
    assert!(wait_for_death(pid), "opencode {pid} survived shutdown");
    f.sup.shutdown().unwrap();
    assert_eq!(f.ok("GET", &format!("/api/ws/{ws}/engine"), None)["state"], "stopped");
}

#[test]
fn opencode_json_reports_the_path_the_builder_and_the_written_text() {
    let f = setup();
    let ws = create(&f, "Json", "personal");
    let out = f.ok("GET", &format!("/api/ws/{ws}/opencode-json"), None);
    assert!(out["path"].as_str().unwrap().ends_with("config/opencode.json"));
    assert_eq!(out["json"]["$schema"], "https://opencode.ai/config.json");
    assert!(out["written"].is_string(), "the route also reads the file");
    f.sup.with_core(|core| core.rt(&ws).unwrap().write_config()).unwrap().unwrap();
    let out = f.ok("GET", &format!("/api/ws/{ws}/opencode-json"), None);
    assert!(out["written"].as_str().unwrap().contains("$schema"));
}

/// Rewriting the config while the engine runs restarts it; a stopped engine just gets the new file.
#[test]
fn settings_changes_reach_the_engine() {
    if system_version().is_none() {
        return;
    }
    let f = setup();
    let ws = create(&f, "Restart", "personal");
    let first = f.ok("POST", &format!("/api/ws/{ws}/engine/start"), None);
    let restarted = f.ok("POST", &format!("/api/ws/{ws}/engine/restart"), None);
    assert_eq!(restarted["state"], "running");
    assert_ne!(first["pid"], restarted["pid"]);
    let stopped: Value = f.ok("POST", &format!("/api/ws/{ws}/engine/stop"), None);
    assert_eq!(stopped["state"], "stopped");
    f.sup.shutdown().unwrap();
}
