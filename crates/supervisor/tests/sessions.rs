//! Port of `test/sessions.test.ts`: R-17 start or connect, R-18 import, R-19 two-way sync — against real opencode
//! servers. Skips with a printed reason when `opencode` is not on PATH; every wait is bounded.

mod common;

use common::{create, setup, Fixture};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::time::{Duration, Instant};
use workspaceai_supervisor::engine::detect::find_opencode;
use workspaceai_supervisor::engine::{ManagedOptions, ManagedServer, OpencodeClient};

struct User {
    server: ManagedServer,
    _home: tempfile::TempDir,
    dirs: [String; 3], // castform, castform/sim, dotfiles
}

/// A separately running opencode server ("the user's own") with three sessions in three folders.
fn user_server() -> Option<User> {
    if find_opencode("opencode").is_none() {
        eprintln!("skipping: opencode is not on PATH (session tests need the real binary)");
        return None;
    }
    let home = tempfile::tempdir().unwrap();
    let h = home.path().to_string_lossy().into_owned();
    let dirs = [format!("{h}/code/castform"), format!("{h}/code/castform/sim"), format!("{h}/dotfiles")];
    for d in &dirs {
        std::fs::create_dir_all(d).unwrap();
    }
    let x = format!("{h}/user-opencode");
    let mut env: BTreeMap<String, String> = ["PATH", "HOME", "TMPDIR"].iter().filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v))).collect();
    for (k, sub) in [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"), ("XDG_STATE_HOME", "state"), ("XDG_CACHE_HOME", "cache")] {
        env.insert(k.into(), format!("{x}/{sub}"));
    }
    std::fs::create_dir_all(&x).unwrap();
    let mut server = ManagedServer::new(
        ManagedOptions { binary: "opencode".into(), cwd: home.path().into(), env, log_dir: x.into(), idle_minutes: 0 },
        Arc::new(AtomicI64::new(0)),
    );
    server.start().expect("user opencode starts");
    let c = server.client();
    for (title, dir) in [("Fix STL import scaling", &dirs[0]), ("Wall thickness solver tests", &dirs[1]), ("Neovim config cleanup", &dirs[2])] {
        c.create_session(title, dir, "").unwrap();
    }
    Some(User { server, _home: home, dirs })
}

fn wait(mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    false
}

fn by_name<'a>(snap: &'a Value, n: &str) -> Option<&'a Value> {
    snap["projects"].as_array().unwrap().iter().find(|p| p["name"] == n)
}

fn snapshot(f: &Fixture, ws: &str) -> Value {
    f.ok("GET", &format!("/api/ws/{ws}/snapshot"), None)
}

/// One opencode pair for the whole file (as the TS `beforeAll`), so the R-17..R-19 steps run in order.
#[test]
fn sessions_r17_r18_r19() {
    let Some(user) = user_server() else { return };
    let (url, pw) = (user.server.url(), user.server.password.clone());
    let f = setup();

    // a connection must pass its test first (R-17)
    let good = f.ok("POST", "/api/onboarding/test-remote", Some(json!({ "url": url, "username": "opencode", "password": pw })));
    assert_eq!((good["ok"].as_bool(), good["sessions"].as_u64(), good["folders"].as_u64()), (Some(true), Some(3), Some(3)));
    let bad = f.ok("POST", "/api/onboarding/test-remote", Some(json!({ "url": url, "username": "opencode", "password": "wrong" })));
    assert_eq!(bad["ok"], false);
    assert!(bad["error"].as_str().unwrap().contains("password"));
    let plain = f.ok("POST", "/api/onboarding/test-remote", Some(json!({ "url": "http://dev-box.example.test:4096", "username": "opencode", "password": "x" })));
    assert_eq!(plain["ok"], false);
    assert!(plain["error"].as_str().unwrap().contains("HTTPS") || plain["error"].as_str().unwrap().contains("tunnel"));

    // a workspace connected to a running server imports its sessions; folders become nested projects (R-18)
    let r = create(&f, "Remote", "personal");
    let mode = format!("/api/ws/{r}/engine/mode");
    assert_eq!(f.call("POST", &mode, Some(json!({ "mode": "remote", "url": url, "username": "opencode", "password": "wrong" }))).unwrap_err().status, 400);
    f.ok("POST", &mode, Some(json!({ "mode": "remote", "url": url, "username": "opencode", "password": pw })));
    assert_eq!(f.sup.with_core(|c| c.rt(&r).unwrap().engine_status().state).unwrap(), "connected");
    let scan = f.ok("GET", &format!("/api/ws/{r}/scan-sessions"), None)["sessions"].as_array().unwrap().clone();
    assert_eq!(scan.len(), 3);
    let keep: Vec<Value> = scan.iter().filter(|s| s["title"] != "Neovim config cleanup").map(|s| s["id"].clone()).collect();
    f.ok("POST", &format!("/api/ws/{r}/import-sessions"), Some(json!({ "ids": keep })));
    let snap = snapshot(&f, &r);
    let cf = by_name(&snap, "Castform").expect("Castform").clone();
    let sim = by_name(&snap, "Sim").expect("Sim").clone();
    assert_eq!(sim["parent"], cf["id"]);
    assert!(by_name(&snap, "Dotfiles").is_none());
    let solver = snap["chats"].as_array().unwrap().iter().find(|c| c["title"] == "Wall thickness solver tests").unwrap();
    assert_eq!((&solver["project"], &solver["imported"]), (&sim["id"], &json!(true)));

    // a session started in the TUI appears in the right project (R-19)
    let tui = OpencodeClient::new(&url, "opencode", &pw).create_session("Started in the TUI", &user.dirs[1], "").unwrap();
    let sid = tui["id"].as_str().unwrap().to_string();
    assert!(wait(|| snapshot(&f, &r)["chats"].as_array().unwrap().iter().any(|c| c["sid"] == sid.as_str())));
    let snap = snapshot(&f, &r);
    let adopted = snap["chats"].as_array().unwrap().iter().find(|c| c["sid"] == sid.as_str()).unwrap();
    assert_eq!(adopted["project"], sim["id"]);
    assert!(by_name(&snap, "Dotfiles").is_none(), "a session skipped at import is not adopted later");
    // …and a session started here is a session on that same server, so the TUI sees it.
    let chat = f.ok("POST", &format!("/api/ws/{r}/chats"), Some(json!({ "project": cf["id"], "title": "Started in harness" })));
    let on_server = OpencodeClient::new(&url, "opencode", &pw).list_sessions(None, 1000).unwrap();
    assert!(on_server.iter().any(|s| s["id"] == chat["sid"]));
    let cmd = f.ok("GET", &format!("/api/ws/{r}/chats/{}/attach", chat["id"].as_str().unwrap()), None)["command"].as_str().unwrap().to_string();
    assert_eq!(cmd, format!("opencode --server {url} --session {} {}", chat["sid"].as_str().unwrap(), user.dirs[0]));

    // rename, archive and delete reach the server (plan (c))
    let cid = chat["id"].as_str().unwrap();
    f.ok("PATCH", &format!("/api/ws/{r}/chats/{cid}"), Some(json!({ "title": "Renamed here" })));
    let c = OpencodeClient::new(&url, "opencode", &pw);
    assert_eq!(c.get_session(chat["sid"].as_str().unwrap()).unwrap()["title"], "Renamed here");
    f.ok("DELETE", &format!("/api/ws/{r}/chats/{cid}"), None);
    assert!(c.get_session(chat["sid"].as_str().unwrap()).is_err());
    assert!(!snapshot(&f, &r)["chats"].as_array().unwrap().iter().any(|x| x["id"] == cid));

    // a managed workspace copies sessions from another server with their history (R-18)
    let m = create(&f, "Managed", "personal");
    let src = OpencodeClient::new(&url, "opencode", &pw);
    let ids: Vec<String> = src.list_sessions(None, 1000).unwrap().iter().filter(|s| s["title"] == "Fix STL import scaling").map(|s| s["id"].as_str().unwrap().to_string()).collect();
    let res = f.sup.with_core_mut(|core| core.rt_mut(&m).unwrap().copy_sessions(&src, &ids)).unwrap().unwrap();
    assert_eq!((res["failed"].clone(), res["chats"].as_u64()), (json!([]), Some(1)));
    let snap = snapshot(&f, &m);
    let copied = snap["chats"].as_array().unwrap().iter().find(|c| c["title"] == "Fix STL import scaling").unwrap().clone();
    let proj = snap["projects"].as_array().unwrap().iter().find(|p| p["id"] == copied["project"]).unwrap();
    assert_eq!(proj["name"], "Castform");
    let own_url = f.sup.with_core(|c| c.rt(&m).unwrap().engine_status().url).unwrap();
    assert_ne!(own_url, url);
    let mine = f.sup.with_core_mut(|core| core.rt_mut(&m).unwrap().engine_client().unwrap()).unwrap();
    assert_eq!(mine.get_session(copied["sid"].as_str().unwrap()).unwrap()["id"], copied["sid"]);

    // the opencode launch spec carries --server and the password in env, never in argv (B5 acceptance)
    let spec = f.ok("GET", &format!("/api/ws/{m}/chats/{}/launch", copied["id"].as_str().unwrap()), None);
    let argv: Vec<&str> = spec["argv"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    let i = argv.iter().position(|a| *a == "--server").expect("--server");
    assert_eq!(argv[i + 1], own_url);
    assert_eq!(argv[argv.iter().position(|a| *a == "--session").unwrap() + 1], copied["sid"].as_str().unwrap());
    let secret = f.sup.with_core(|c| c.rt(&m).unwrap().engine.as_ref().unwrap().password.clone()).unwrap();
    assert!(!secret.is_empty());
    assert_eq!(spec["env"]["OPENCODE_SERVER_PASSWORD"], secret.as_str());
    assert!(!argv.iter().any(|a| a.contains(&secret)), "password must not be in argv");
    // and the history reads back as plain text (empty sessions have none)
    assert!(f.ok("GET", &format!("/api/ws/{m}/chats/{}/messages", copied["id"].as_str().unwrap()), None)["messages"].is_array());
    f.sup.shutdown().unwrap();
}
