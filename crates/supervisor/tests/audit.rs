//! Audit collection for org workspaces (R-36): new entries only, content withheld unless policy says otherwise.

mod common;

use common::{create, setup};
use serde_json::Value;
use std::time::{Duration, Instant};

fn collector(expected: usize) -> (u16, std::thread::JoinHandle<Vec<Value>>) {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("collector");
    let port = server.server_addr().to_ip().expect("ip listen addr").port();
    let handle = std::thread::spawn(move || {
        let mut bodies = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(15);
        while bodies.len() < expected && Instant::now() < deadline {
            let Ok(Some(mut request)) = server.recv_timeout(Duration::from_millis(250)) else { continue };
            let body = {
                let reader = request.as_reader();
                let mut text = String::new();
                reader.read_to_string(&mut text).expect("body");
                text
            };
            bodies.push(serde_json::from_str(&body).unwrap_or(Value::Null));
            request.respond(tiny_http::Response::from_string("{}")).expect("response");
        }
        bodies
    });
    (port, handle)
}

fn audit_lines(f: &common::Fixture, ws: &str) -> usize {
    std::fs::read_to_string(f.path().join("ws").join(ws).join("data").join("audit.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

fn join_org(f: &common::Fixture, name: &str, email: &str, extra: Value) -> String {
    let mut body = serde_json::json!({ "name": name, "email": email });
    if let (Some(target), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in extra {
            target.insert(k.clone(), v.clone());
        }
    }
    f.ok("POST", "/api/workspaces/join-org", Some(body))["id"].as_str().unwrap().to_string()
}

#[test]
fn org_uploads_new_audit_entries_only_and_withholds_content_without_log_content() {
    let f = setup();
    let (port, collector) = collector(3);
    let url = format!("http://127.0.0.1:{port}/audit");
    let org = join_org(&f, "Audit Org", "me@org.test", serde_json::json!({ "auditUrl": url.clone() }));

    f.sup
        .with_core_mut(|core| {
            let rt = core.rt(&org).unwrap();
            rt.audit("chat.prompt", "Secret merger plan · opencode", "harness");
            rt.audit("provider.call", "org-gateway/llama · in 120 · out 40", "harness");
            rt.audit("fact.write", "private note", "agent");
        })
        .unwrap();

    let first = f.ok("POST", &format!("/api/ws/{org}/audit/upload"), None);
    assert!(first["sent"].as_u64().unwrap() > 2, "sent: {first}");
    // Nothing new: no request, nothing sent.
    assert_eq!(f.ok("POST", &format!("/api/ws/{org}/audit/upload"), None)["sent"], 0);
    f.sup.with_core_mut(|core| core.rt(&org).unwrap().audit("export", "workspace.tar.gz", "you")).unwrap();
    assert_eq!(f.ok("POST", &format!("/api/ws/{org}/audit/upload"), None)["sent"], 1);

    // A second org whose policy allows content keeps details.
    let loud = join_org(&f, "Loud Org", "you@org.test", serde_json::json!({ "auditUrl": url, "logContent": true }));
    f.sup.with_core_mut(|core| core.rt(&loud).unwrap().audit("chat.prompt", "keep this", "harness")).unwrap();
    assert!(f.ok("POST", &format!("/api/ws/{loud}/audit/upload"), None)["sent"].as_u64().unwrap() >= 1);

    // A personal workspace never uploads.
    let personal = create(&f, "Mine", "personal");
    assert_eq!(f.ok("POST", &format!("/api/ws/{personal}/audit/upload"), None)["sent"], 0);

    let bodies = collector.join().expect("collector thread");
    assert_eq!(bodies.len(), 3, "bodies: {bodies:?}");

    let all = &bodies[0];
    assert_eq!(all["org"], "Audit Org");
    assert_eq!(all["workspace"], org);
    assert_eq!(all["subject"], "me@org.test");
    assert!(all["policyVersion"].as_str().unwrap().starts_with("2026."));
    let events = all["events"].as_array().unwrap();
    let actions: Vec<&str> = events.iter().map(|e| e["action"].as_str().unwrap()).collect();
    for expected in ["sso.signin", "policy.accept", "chat.prompt", "provider.call", "fact.write"] {
        assert!(actions.contains(&expected), "missing {expected}: {actions:?}");
    }
    let detail = |action: &str| events.iter().find(|e| e["action"] == action).unwrap()["detail"].as_str().unwrap();
    assert_eq!(detail("chat.prompt"), "");
    assert_eq!(detail("fact.write"), "");
    assert!(detail("provider.call").contains("in 120 · out 40"));
    assert!(!all.to_string().contains("Secret merger plan"));

    // The second upload carries only the entry written after the first cursor.
    let second = &bodies[1];
    assert_eq!(second["events"].as_array().unwrap().len(), 1);
    assert_eq!(second["events"][0]["action"], "export");
    assert_eq!(second["workspace"], org);

    // logContent keeps the detail.
    let loud_body = &bodies[2];
    assert_eq!(loud_body["workspace"], loud);
    let loud_prompt = loud_body["events"].as_array().unwrap().iter().find(|e| e["action"] == "chat.prompt").expect("chat.prompt").clone();
    assert_eq!(loud_prompt["detail"], "keep this");

    // Cursor file records how many lines have been sent.
    let cursor: Value = serde_json::from_str(
        &std::fs::read_to_string(f.path().join("ws").join(&org).join("data").join("audit-upload.json")).expect("cursor"),
    )
    .expect("cursor json");
    assert_eq!(cursor["sent"].as_u64().unwrap() as usize, audit_lines(&f, &org));
    assert!(cursor["at"].as_i64().unwrap() > 0);
}
