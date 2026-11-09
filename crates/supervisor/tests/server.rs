//! The loopback listener (plan (e), B9 acceptance): 127.0.0.1 only, per-launch token, path allowlist, body cap;
//! the URL and token reach MCP servers through the env only. Ports `core.test.ts` › rejects requests without the
//! per-launch token and `security.test.ts` › the supervisor API only listens on loopback and needs its token.

mod common;

use common::serve;
use serde_json::{json, Value};
use workspaceai_supervisor::paths::WsPaths;
use workspaceai_supervisor::server::{token_eq, MAX_BODY};

#[test]
fn the_supervisor_api_only_listens_on_loopback_and_needs_its_token() {
    let f = serve();
    assert_eq!(f.server.addr.ip().to_string(), "127.0.0.1");
    assert!(f.server.url().starts_with("http://127.0.0.1:"));
    let w = f.ok("POST", "/api/workspaces", Some(json!({ "name": "Net", "kind": "personal" })))["id"].as_str().unwrap().to_string();
    let snap = format!("/api/ws/{w}/snapshot");
    assert_eq!(f.http("GET", &snap, None, None).0, 401, "no token");
    assert_eq!(f.http("GET", &snap, Some("wrong"), None).0, 401, "wrong token");
    let short = &f.server.token[..f.server.token.len() - 1];
    assert_eq!(f.http("GET", &snap, Some(short), None).0, 401, "prefix of the token");
    let (status, body) = f.http("GET", &snap, Some(&f.server.token), None);
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["workspace"]["id"], json!(w));
}

#[test]
fn rejects_requests_without_the_per_launch_token() {
    let f = serve();
    assert_eq!(f.http("GET", "/api/state", None, None).0, 401);
}

#[test]
fn only_allowlisted_paths_are_routed() {
    let f = serve();
    let w = f.ok("POST", "/api/workspaces", Some(json!({ "name": "Net", "kind": "personal" })))["id"].as_str().unwrap().to_string();
    let t = Some(f.server.token.as_str());
    // Real in-process routes, but not for external callers.
    for (m, p) in [("GET", "/api/state".to_string()), ("GET", format!("/api/ws/{w}/secrets")), ("PATCH", format!("/api/ws/{w}")), ("DELETE", format!("/api/workspaces/{w}")), ("GET", "/".to_string()), ("POST", format!("/api/ws/{w}/facts"))] {
        let body = if m == "GET" || m == "DELETE" { None } else { Some("{}") };
        assert_eq!(f.http(m, &p, t, body).0, 404, "{m} {p}");
    }
    // Encoded or dot segments never reach the router.
    assert_eq!(f.http("GET", &format!("/api/ws/{w}/artifacts/..%2F..%2Fx/content"), t, None).0, 404);
    // Allowlisted: the memory-changed callback works with the token.
    assert_eq!(f.http("POST", &format!("/api/ws/{w}/memory-changed"), t, Some("{}")).0, 200);
    assert_eq!(f.http("POST", &format!("/api/ws/{w}/memory-changed"), t, Some("not json")).0, 400);
    // The workspace still exists: the DELETE above never ran.
    assert!(f.sup.with_core(|c| c.rt(&w).is_ok()).unwrap());
}

#[test]
fn oversized_bodies_are_refused() {
    let f = serve();
    let w = f.ok("POST", "/api/workspaces", Some(json!({ "name": "Net", "kind": "personal" })))["id"].as_str().unwrap().to_string();
    let big = format!("{{\"content\":\"{}\"}}", "x".repeat(MAX_BODY));
    let res = ureq::post(&format!("{}/api/ws/{w}/agent/guide", f.server.url())).set("x-harness-token", &f.server.token).send_string(&big);
    match res {
        Err(ureq::Error::Status(413, _)) => {}
        // The server may answer before the client finishes sending; a reset connection is a refusal too.
        Err(ureq::Error::Transport(_)) => {}
        other => panic!("expected 413, got {other:?}"),
    }
}

#[test]
fn token_compare_is_exact() {
    assert!(token_eq(b"abc", b"abc"));
    assert!(!token_eq(b"abc", b"abd"));
    assert!(!token_eq(b"abc", b"ab"));
    assert!(!token_eq(b"", b"a"));
}

#[test]
fn mcp_servers_get_the_loopback_url_and_token_through_the_env_only() {
    let f = serve();
    let w = f.ok("POST", "/api/workspaces", Some(json!({ "name": "Env", "kind": "personal" })))["id"].as_str().unwrap().to_string();
    let token = f.server.token.clone();
    // opencode: the config file names `{env:…}`, the server process env carries the values.
    let (env, cfg) = f
        .sup
        .with_core(|c| {
            let rt = c.rt(&w).unwrap();
            rt.write_config().unwrap();
            (rt.engine_env(), std::fs::read_to_string(&rt.paths.opencode_json).unwrap())
        })
        .unwrap();
    assert_eq!(env.get("HARNESS_SUPERVISOR"), Some(&f.server.url()));
    assert_eq!(env.get("HARNESS_SUPERVISOR_TOKEN"), Some(&token));
    assert!(!cfg.contains(&token), "token written into opencode.json");
    let cfg: Value = serde_json::from_str(&cfg).unwrap();
    for name in ["harness-memory", "harness-artifacts", "harness-vault"] {
        let m = &cfg["mcp"][name];
        let cmd = m["command"].as_array().unwrap();
        assert_eq!((cmd[1].as_str(), cmd.len()), (Some("mcp"), 3), "{name}: `<exe> mcp <kind>`");
        assert_eq!(m["environment"]["HARNESS_SUPERVISOR_TOKEN"], "{env:HARNESS_SUPERVISOR_TOKEN}");
        assert_eq!(m["environment"]["HARNESS_SUPERVISOR"], "{env:HARNESS_SUPERVISOR}");
        assert!(m["environment"].get("HARNESS_CHAT").is_none(), "opencode shares one config");
    }
    // claude: the per-session MCP file has `${…}` references; the launch env has the values; argv has neither.
    f.sup
        .with_core_mut(|c| {
            if let Some(a) = c.rt_mut(&w).unwrap().ws.agents.iter_mut().find(|a| a.id == "claude") {
                a.on = true;
            }
        })
        .unwrap();
    let s = f.ok("POST", &format!("/api/ws/{w}/chats"), Some(json!({ "agent": "claude" })));
    let spec = f.ok("GET", &format!("/api/ws/{w}/chats/{}/launch", s["id"].as_str().unwrap()), None);
    assert_eq!(spec["env"]["HARNESS_SUPERVISOR_TOKEN"], json!(token));
    assert!(!spec["argv"].to_string().contains(&token), "token in argv");
    let file = WsPaths::new(f.home.path(), &w).run.join(format!("{}.mcp.json", s["sid"].as_str().unwrap()));
    let text = std::fs::read_to_string(file).unwrap();
    assert!(!text.contains(&token), "token written into the claude MCP file");
    let mcp: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(mcp["mcpServers"]["harness-artifacts"]["env"]["HARNESS_SUPERVISOR_TOKEN"], "${HARNESS_SUPERVISOR_TOKEN}");
    assert_eq!(mcp["mcpServers"]["harness-artifacts"]["env"]["HARNESS_CHAT"], s["id"]);
    assert_eq!(mcp["mcpServers"]["harness-artifacts"]["args"], json!(["mcp", "artifacts"]));
}

#[test]
fn stopping_the_listener_withdraws_the_url_and_token() {
    let f = serve();
    let w = f.ok("POST", "/api/workspaces", Some(json!({ "name": "Gone", "kind": "personal" })))["id"].as_str().unwrap().to_string();
    let common::Served { server, sup, home: _home } = f;
    drop(server);
    let env = sup.with_core(|c| c.rt(&w).unwrap().engine_env()).unwrap();
    assert!(!env.contains_key("HARNESS_SUPERVISOR_TOKEN"));
}
