//! B5 acceptance for the terminal adapters (plan (c)): claude isolation and per-session MCP, fork/resume
//! choice, claude copy-in import and transcript read, the Chat → Session load migration, and the headless runner
//! against a synthetic opencode server. No real agent CLI or opencode binary is needed.

mod common;

use common::{create, setup, Fixture};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use workspaceai_supervisor::engine::OpencodeClient;
use workspaceai_supervisor::headless::ask_and_wait;
use workspaceai_supervisor::paths::{expand_home, WsPaths};
use workspaceai_supervisor::{Supervisor, UiEvent};

fn enable(f: &Fixture, ws: &str, agent: &str) {
    f.sup
        .with_core_mut(|c| {
            let rt = c.rt_mut(ws).unwrap();
            match rt.ws.agents.iter_mut().find(|a| a.id == agent) {
                Some(a) => a.on = true,
                None => rt.ws.agents.push(workspaceai_shared::types::AcpAgent { id: agent.into(), name: agent.into(), on: true, ..Default::default() }),
            }
        })
        .unwrap();
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

#[test]
fn claude_spec_is_isolated_and_carries_per_session_mcp() {
    let f = setup();
    let ws = create(&f, "Agents", "personal");
    enable(&f, &ws, "claude");
    let s = f.ok("POST", &format!("/api/ws/{ws}/chats"), Some(json!({ "agent": "claude", "text": "fix the build" })));
    let (cid, sid) = (s["id"].as_str().unwrap().to_string(), s["sid"].as_str().unwrap().to_string());
    assert_eq!(sid.len(), 36, "claude ids are fixed before spawn");
    assert_eq!(s["prompt"], "fix the build");

    let spec = f.ok("GET", &format!("/api/ws/{ws}/chats/{cid}/launch"), None);
    let argv = strings(&spec["argv"]);
    let home = WsPaths::new(f.path(), &ws).data.join("agents/claude");
    assert_eq!(spec["env"]["CLAUDE_CONFIG_DIR"], home.to_string_lossy().as_ref());
    assert_eq!(&argv[..3], ["claude", "--session-id", sid.as_str()]);
    assert_eq!(argv.last().unwrap(), "fix the build", "first launch passes the prompt");
    assert!(!argv.contains(&"--dangerously-skip-permissions".to_string()));

    // never the user's own ~/.claude, in argv, env or the MCP file
    let user = expand_home("~/.claude");
    let mcp_path = argv[argv.iter().position(|a| a == "--mcp-config").unwrap() + 1].clone();
    assert!(argv.contains(&"--strict-mcp-config".to_string()));
    let mcp_text = std::fs::read_to_string(&mcp_path).unwrap();
    let mut all: Vec<String> = argv.clone();
    all.extend(spec["env"].as_object().unwrap().values().map(|v| v.as_str().unwrap().to_string()));
    all.push(mcp_text.clone());
    for v in &all {
        assert!(!v.contains(&format!("{user}/")) && !v.ends_with(&user), "references ~/.claude: {v}");
    }
    let mcp: Value = serde_json::from_str(&mcp_text).unwrap();
    for name in ["harness-memory", "harness-artifacts"] {
        assert_eq!(mcp["mcpServers"][name]["env"]["HARNESS_CHAT"], cid.as_str(), "{name}");
        assert_eq!(mcp["mcpServers"][name]["env"]["HARNESS_WS"], ws.as_str());
    }

    // second launch: no prompt again; still a new id until claude has written the transcript
    let again = strings(&f.ok("GET", &format!("/api/ws/{ws}/chats/{cid}/launch"), None)["argv"]);
    assert_ne!(again.last().unwrap(), "fix the build");
    // once the transcript exists, launches resume, full access adds the flag, transcript reads back
    let folder = f.sup.with_core(|c| c.rt(&ws).unwrap().project_folder(&s["project"].as_str().unwrap().to_string())).unwrap();
    let dir = home.join("projects").join(workspaceai_supervisor::agents::claude_slug(&folder));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{sid}.jsonl")), "{\"type\":\"user\",\"message\":{\"content\":\"fix the build\"}}\n{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}\n").unwrap();
    f.ok("PATCH", &format!("/api/ws/{ws}/chats/{cid}"), Some(json!({ "fullAccess": true })));
    let resumed = strings(&f.ok("GET", &format!("/api/ws/{ws}/chats/{cid}/launch"), None)["argv"]);
    assert_eq!(&resumed[..3], ["claude", "--resume", sid.as_str()]);
    assert!(resumed.contains(&"--dangerously-skip-permissions".to_string()));
    let msgs = f.ok("GET", &format!("/api/ws/{ws}/chats/{cid}/messages"), None)["messages"].clone();
    assert_eq!((msgs[0]["text"].as_str(), msgs[1]["text"].as_str()), (Some("fix the build"), Some("done")));

    // fork: new fixed id that forks the old transcript on first launch
    let fork = f.ok("POST", &format!("/api/ws/{ws}/chats/{cid}/fork"), None);
    let fargv = strings(&f.ok("GET", &format!("/api/ws/{ws}/chats/{}/launch", fork["id"].as_str().unwrap()), None)["argv"]);
    assert_eq!(&fargv[..6], ["claude", "--resume", sid.as_str(), "--fork-session", "--session-id", fork["sid"].as_str().unwrap()]);

    // delete keeps the transcript unless confirmed
    f.ok("DELETE", &format!("/api/ws/{ws}/chats/{cid}"), None);
    assert!(dir.join(format!("{sid}.jsonl")).exists());
    assert!(!std::path::Path::new(&mcp_path).exists());
}

#[test]
fn grok_and_pi_specs_use_their_workspace_homes() {
    let f = setup();
    let ws = create(&f, "Others", "personal");
    enable(&f, &ws, "grok");
    enable(&f, &ws, "pi");
    let data = WsPaths::new(f.path(), &ws).data;
    let g = f.ok("POST", &format!("/api/ws/{ws}/chats"), Some(json!({ "agent": "grok" })));
    let spec = f.ok("GET", &format!("/api/ws/{ws}/chats/{}/launch", g["id"].as_str().unwrap()), None);
    assert_eq!(spec["env"]["GROK_HOME"], data.join("agents/grok").to_string_lossy().as_ref());
    assert_eq!(f.call("POST", &format!("/api/ws/{ws}/chats/{}/fork", g["id"].as_str().unwrap()), None).unwrap_err().status, 400);
    let p = f.ok("POST", &format!("/api/ws/{ws}/chats"), Some(json!({ "agent": "pi" })));
    let spec = f.ok("GET", &format!("/api/ws/{ws}/chats/{}/launch", p["id"].as_str().unwrap()), None);
    assert_eq!(spec["env"]["PI_CODING_AGENT_DIR"], data.join("agents/pi").to_string_lossy().as_ref());
    assert_eq!(strings(&spec["argv"])[..3], ["pi".to_string(), "--session-id".into(), p["sid"].as_str().unwrap().into()]);
    assert!(data.join("agents/pi/models.json").exists());
    // an agent switched off, or unknown, is refused
    assert_eq!(f.call("POST", &format!("/api/ws/{ws}/chats"), Some(json!({ "agent": "gemini" }))).unwrap_err().status, 400);
}

#[test]
fn claude_sessions_are_copied_in_never_pointed_at() {
    let f = setup();
    let ws = create(&f, "Import", "personal");
    let src = tempfile::tempdir().unwrap();
    let cwd = f.path().join("code/castform");
    std::fs::create_dir_all(&cwd).unwrap();
    let id = "0b9a2c1e-1111-4222-8333-444455556666";
    let dir = src.path().join("projects/-x");
    std::fs::create_dir_all(&dir).unwrap();
    let line = json!({ "type": "user", "cwd": cwd, "message": { "content": "Fix STL import scaling" } });
    std::fs::write(dir.join(format!("{id}.jsonl")), format!("{line}\n")).unwrap();
    std::fs::write(dir.join("not-a-session.txt"), "x").unwrap();

    let found = f.sup.with_core(|c| c.rt(&ws).unwrap().scan_claude_sessions(src.path())).unwrap();
    assert_eq!((found.len(), found[0]["title"].as_str()), (1, Some("Fix STL import scaling")));
    let r = f.sup.with_core_mut(|c| c.rt_mut(&ws).unwrap().import_claude_sessions(src.path(), &[id.to_string()])).unwrap().unwrap();
    assert_eq!(r["chats"], 1);
    let home = WsPaths::new(f.path(), &ws).data.join("agents/claude/projects");
    let copied = home.join(workspaceai_supervisor::agents::claude_slug(&cwd.to_string_lossy())).join(format!("{id}.jsonl"));
    assert!(copied.is_file() && !copied.symlink_metadata().unwrap().file_type().is_symlink());
    let snap = f.ok("GET", &format!("/api/ws/{ws}/snapshot"), None);
    let s = snap["chats"].as_array().unwrap().iter().find(|c| c["sid"] == id).unwrap().clone();
    assert_eq!((s["agent"].as_str(), s["imported"].as_bool()), (Some("claude"), Some(true)));
    assert_eq!(snap["projects"].as_array().unwrap().iter().find(|p| p["id"] == s["project"]).unwrap()["name"], "Castform");
    assert!(f.sup.with_core(|c| c.rt(&ws).unwrap().scan_claude_sessions(src.path())).unwrap().is_empty(), "imported once");
}

#[test]
fn ts_chats_migrate_on_load_with_one_backup() {
    let f = setup();
    let ws = create(&f, "Legacy", "personal");
    let paths = WsPaths::new(f.path(), &ws);
    let mut state: Value = serde_json::from_str(&std::fs::read_to_string(&paths.state).unwrap()).unwrap();
    state["chats"] = json!([
        { "id": "chat_1", "project": "p", "title": "Old", "sid": "acp:claude:6f1c", "agent": "claude", "model": "default", "effort": "high", "web": true, "instructions": "", "imported": false, "archived": false, "yolo": true, "createdAt": 1, "updatedAt": 2, "transcript": [], "keepMe": 7 }
    ]);
    let written = serde_json::to_string(&state).unwrap();
    std::fs::write(&paths.state, &written).unwrap();
    drop(f.sup);
    let sup = Supervisor::new(f.home.path().to_path_buf()).unwrap();
    let backup = paths.state.with_extension("json.bak-tsport");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), written, "TS file kept once");
    let now: Value = serde_json::from_str(&std::fs::read_to_string(&paths.state).unwrap()).unwrap();
    let c = &now["chats"][0];
    assert_eq!((c["sid"].as_str(), c["launched"].as_bool(), c["keepMe"].as_i64()), (Some("6f1c"), Some(true), Some(7)));
    for k in ["model", "effort", "web", "transcript", "graphNotes", "yolo"] {
        assert!(c.get(k).is_none(), "{k} dropped");
    }
    let mtime = std::fs::metadata(&paths.state).unwrap().modified().unwrap();
    drop(sup);
    let _again = Supervisor::new(f.home.path().to_path_buf()).unwrap();
    assert_eq!(std::fs::metadata(&paths.state).unwrap().modified().unwrap(), mtime, "no write when nothing changed");
}

/// A synthetic opencode: the wait takes 2.5 s, one permission ask is pending meanwhile, then a reply is listed.
fn fake_opencode() -> String {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.server_addr().to_ip().unwrap());
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let path = req.url().split('?').next().unwrap_or("").to_string();
            let body = match (req.method().as_str(), path.as_str()) {
                ("POST", "/api/session/s1/prompt") => json!({}),
                ("POST", "/api/experimental/session/s1/wait") => {
                    std::thread::sleep(Duration::from_millis(2500));
                    json!({})
                }
                ("GET", "/api/session/s1/permission") => json!({ "data": [{ "id": "per_1", "sessionID": "s1", "permission": "bash" }] }),
                ("GET", "/api/session/s1/message") => {
                    let now = workspaceai_shared::util::now_ms();
                    json!({ "data": [{ "id": "m1", "type": "assistant", "time": { "created": now }, "content": [{ "type": "text", "text": "done" }] }] })
                }
                _ => json!({}),
            };
            let _ = req.respond(tiny_http::Response::from_string(body.to_string()));
        }
    });
    url
}

#[test]
fn headless_ask_and_wait_raises_approvals_and_returns_the_reply() {
    let url = fake_opencode();
    let client = OpencodeClient::new(&url, "opencode", "pw");
    let events = workspaceai_supervisor::supervisor::Emitter::default();
    let seen: Arc<Mutex<Vec<UiEvent>>> = Arc::default();
    let s2 = seen.clone();
    events.on(Arc::new(move |e| s2.lock().unwrap().push(e)));
    let reply = ask_and_wait(&client, &events, "ws_1", "chat_1", "s1", "go", Duration::from_secs(20)).unwrap();
    assert_eq!(reply, "done");
    let got = seen.lock().unwrap();
    let approvals: Vec<&UiEvent> = got.iter().filter(|e| matches!(e, UiEvent::Approval { .. })).collect();
    assert_eq!(approvals.len(), 1, "one event per ask, however often it is polled");
    assert!(matches!(approvals[0], UiEvent::Approval { chat, permission, .. } if chat == "chat_1" && permission["id"] == "per_1"));
}
