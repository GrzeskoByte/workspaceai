//! The MCP stdio servers driven as a client would, against the built binary (`workspaceai-mcp`, the same code the
//! app serves as `workspaceai mcp <kind>`). Ports `memory.test.ts` › the agent's memory MCP server reads approved
//! facts and writes only suggestions; adds the artifact tools end to end over the loopback listener.

mod common;

use common::{serve, Served};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: i64,
}

impl Client {
    fn start(kind: &str, env: &[(&str, String)]) -> Client {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_workspaceai-mcp"));
        cmd.arg(kind).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).env_clear();
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("spawn workspaceai-mcp");
        let (stdin, stdout) = (child.stdin.take().unwrap(), BufReader::new(child.stdout.take().unwrap()));
        let mut c = Client { child, stdin, stdout, next: 0 };
        let init = c.request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }));
        assert_eq!(init["serverInfo"]["name"], format!("harness-{kind}"));
        c.notify("notifications/initialized");
        c
    }

    fn send(&mut self, msg: Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str) {
        self.send(json!({ "jsonrpc": "2.0", "method": method }));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("reply");
        let reply: Value = serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON-RPC: {line:?}"));
        assert_eq!(reply["id"], id);
        reply["result"].clone()
    }

    fn tools(&mut self) -> Vec<String> {
        self.request("tools/list", json!({}))["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
    }

    /// The text of a tool result; panics on `isError` unless `ok` is false.
    fn call(&mut self, name: &str, args: Value) -> (String, bool) {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        let text = r["content"].as_array().unwrap().iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n");
        (text, r["isError"] == true)
    }

    fn text(&mut self, name: &str, args: Value) -> String {
        let (t, err) = self.call(name, args);
        assert!(!err, "{name} failed: {t}");
        t
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn env_for(f: &Served, w: &str, chat: Option<&str>) -> Vec<(&'static str, String)> {
    let memory = f.sup.with_core(|c| c.rt(w).unwrap().paths.memory.to_string_lossy().into_owned()).unwrap();
    let mut env = vec![
        ("HARNESS_MEMORY_DIR", memory),
        ("HARNESS_WS", w.to_string()),
        ("HARNESS_SUPERVISOR", f.server.url()),
        ("HARNESS_SUPERVISOR_TOKEN", f.server.token.clone()),
        ("HARNESS_MEMORY_BACKEND", "facts".into()),
    ];
    if let Some(c) = chat {
        env.push(("HARNESS_CHAT", c.to_string()));
    }
    env
}

#[test]
fn the_agents_memory_mcp_server_reads_approved_facts_and_writes_only_suggestions() {
    let f = serve();
    let w = f.ok("POST", "/api/workspaces", Some(json!({ "name": "Personal", "kind": "personal" })))["id"].as_str().unwrap().to_string();
    let p = f.ok("POST", &format!("/api/ws/{w}/projects"), Some(json!({ "name": "Castform", "parent": null })))["id"].as_str().unwrap().to_string();
    let s = f.ok("POST", &format!("/api/ws/{w}/projects"), Some(json!({ "name": "Sim", "parent": p })))["id"].as_str().unwrap().to_string();
    f.ok("POST", &format!("/api/ws/{w}/facts"), Some(json!({ "text": "Default mold wall is 8 mm.", "scope": s })));
    f.ok("POST", &format!("/api/ws/{w}/facts"), Some(json!({ "text": "Uses Mold Star 30.", "scope": p })));
    let mut c = Client::start("memory", &env_for(&f, &w, None));
    let tools = c.tools();
    for t in ["memory_search", "memory_read", "memory_write"] {
        assert!(tools.contains(&t.to_string()), "{t} in {tools:?}");
    }
    assert!(!tools.iter().any(|t| t == "graphify_query"), "Graphify is not ported");
    // A sub-project's read widens to its parents and the workspace.
    let read = c.text("memory_read", json!({ "project": "Sim" }));
    assert!(read.contains("Default mold wall is 8 mm."), "{read}");
    assert!(read.contains("Uses Mold Star 30."), "{read}");
    let wrote = c.text("memory_write", json!({ "text": "Pours at 22 °C.", "project": "Sim" }));
    assert!(wrote.contains("approves"), "{wrote}");
    assert_eq!(c.text("memory_write", json!({ "text": "pours at 22 °c.", "project": "Sim" })), "Already remembered.");
    // Not usable until approved.
    assert_eq!(c.text("memory_search", json!({ "query": "pours", "project": "Sim" })), "No matching facts.");
    let facts = f.ok("GET", &format!("/api/ws/{w}/snapshot"), None)["facts"].as_array().unwrap().clone();
    let fact = facts.iter().find(|x| x["text"] == "Pours at 22 °C.").expect("suggested fact");
    assert_eq!((fact["status"].as_str(), fact["scope"].as_str()), (Some("suggested"), Some(s.as_str())));
    // The supervisor heard about it over loopback (memory-changed).
    let audit = f.ok("GET", &format!("/api/ws/{w}/audit"), None).to_string();
    assert!(audit.contains("from agent"), "memory-changed callback: {audit}");
    f.ok("PATCH", &format!("/api/ws/{w}/facts/{}", fact["id"].as_str().unwrap()), Some(json!({ "status": "approved" })));
    assert!(c.text("memory_search", json!({ "query": "pours", "project": "Sim" })).contains("Pours at 22 °C."));
    // Arguments are checked like the TS schema.
    assert!(c.call("memory_write", json!({ "text": "x".repeat(401) })).1);
    assert!(c.call("memory_write", json!({})).1);
}

#[test]
fn the_memory_server_needs_its_folder() {
    let out = Command::new(env!("CARGO_BIN_EXE_workspaceai-mcp")).arg("memory").env_clear().stdin(Stdio::null()).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("HARNESS_MEMORY_DIR"));
}

#[test]
fn artifact_tools_save_through_the_supervisor_for_the_calling_session() {
    let f = serve();
    let w = f.ok("POST", "/api/workspaces", Some(json!({ "name": "Art", "kind": "personal" })))["id"].as_str().unwrap().to_string();
    let p = f.ok("POST", &format!("/api/ws/{w}/projects"), Some(json!({ "name": "Castform", "parent": null })))["id"].as_str().unwrap().to_string();
    f.sup
        .with_core_mut(|c| {
            if let Some(a) = c.rt_mut(&w).unwrap().ws.agents.iter_mut().find(|a| a.id == "claude") {
                a.on = true;
            }
        })
        .unwrap();
    let chat = f.ok("POST", &format!("/api/ws/{w}/chats"), Some(json!({ "project": p, "agent": "claude" })))["id"].as_str().unwrap().to_string();
    let mut c = Client::start("artifacts", &env_for(&f, &w, Some(&chat)));
    assert_eq!(c.tools(), ["artifact_guide", "artifact_create", "artifact_update", "artifact_read", "artifact_list"]);
    let (err, failed) = c.call("artifact_create", json!({ "type": "doc", "title": "Notes", "content": "# Notes", "format": "markdown" }));
    assert!(failed && err.contains("artifact_guide"), "guide first: {err}");
    assert!(c.text("artifact_guide", json!({ "type": "doc" })).contains("Open Design reference"));
    let created = c.text("artifact_create", json!({ "type": "doc", "title": "Notes", "content": "# Notes", "format": "markdown" }));
    assert!(created.contains("Created doc “Notes”") && created.contains("in Castform"), "{created}");
    let a = f.ok("GET", &format!("/api/ws/{w}/snapshot"), None)["artifacts"][0].clone();
    assert_eq!((a["project"].as_str(), a["chat"].as_str()), (Some(p.as_str()), Some(chat.as_str())), "attributed to HARNESS_CHAT (R-13)");
    let id = a["id"].as_str().unwrap();
    assert_eq!(c.text("artifact_update", json!({ "id": id, "content": "# Notes v2" })), "Saved “Notes” v2.");
    assert!(c.text("artifact_read", json!({ "id": id })).starts_with("# Notes (Doc, v2, markdown)\n\n# Notes v2"));
    assert!(c.text("artifact_list", json!({})).contains(&format!("- {id} · Doc · Notes · v2")));
    // A wrong token is refused by the listener, and the tool says so.
    let mut env = env_for(&f, &w, Some(&chat));
    env.iter_mut().find(|(k, _)| *k == "HARNESS_SUPERVISOR_TOKEN").unwrap().1 = "nope".into();
    let mut bad = Client::start("artifacts", &env);
    let (msg, failed) = bad.call("artifact_list", json!({}));
    assert!(failed && msg.contains("unauthorized"), "{msg}");
}
