//! Port of `test/providers.test.ts` (R-02, R-20, R-21) and the providers/mcp parts of `vendors.test.ts`:
//! a fake OpenAI-compatible endpoint stands in for a LiteLLM-style gateway, so no model key is needed.

mod common;

use common::{create, setup};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Response, Server};

const KEY: &str = "sk-gateway-test-0123456789";

#[derive(Clone, Copy)]
enum Mode {
    Gateway,
    HtmlPage,
    MethodNotAllowed,
}

struct Fake {
    server: Arc<Server>,
    url: String,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    fn start(mode: Mode) -> Fake {
        let server = Arc::new(Server::http("127.0.0.1:0").unwrap());
        let port = server.server_addr().to_ip().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let handle = server.clone();
        let log = seen.clone();
        std::thread::spawn(move || {
            for mut request in handle.incoming_requests() {
                let path = request.url().split('?').next().unwrap_or("").to_string();
                match mode {
                    Mode::HtmlPage => respond(request, 200, "text/html", "<!doctype html>".into()),
                    Mode::MethodNotAllowed => {
                        let status = if request.method() == &Method::Post { 405 } else { 200 };
                        let ctype = if status == 405 { "application/json" } else { "text/html" };
                        respond(request, status, ctype, if status == 405 { "{\"detail\":\"Method Not Allowed\"}".into() } else { "<!doctype html>".into() })
                    }
                    Mode::Gateway => {
                        let authorized = request.headers().iter().any(|h| h.field.equiv("authorization") && h.value.as_str() == format!("Bearer {KEY}"));
                        if !authorized {
                            respond(request, 401, "application/json", "{\"error\":{\"message\":\"bad key\"}}".into());
                            continue;
                        }
                        if path.ends_with("/models") {
                            respond(request, 200, "application/json", "{\"object\":\"list\",\"data\":[{\"id\":\"gateway-model\",\"object\":\"model\"}]}".into());
                            continue;
                        }
                        let mut body = String::new();
                        let _ = request.as_reader().read_to_string(&mut body);
                        let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                        if let Some(model) = parsed["model"].as_str() {
                            log.lock().unwrap().push(model.to_string());
                        }
                        let reply = json!({
                            "id": "c1", "object": "chat.completion", "created": 1, "model": parsed["model"],
                            "choices": [{ "index": 0, "message": { "role": "assistant", "content": "Hello from the gateway." }, "finish_reason": "stop" }],
                            "usage": { "prompt_tokens": 5, "completion_tokens": 4, "total_tokens": 9 }
                        });
                        respond(request, 200, "application/json", reply.to_string());
                    }
                }
            }
        });
        Fake { server, url: format!("http://127.0.0.1:{port}/v1"), seen }
    }

    fn stop(&self) {
        self.server.unblock();
    }
}

fn respond(request: tiny_http::Request, status: u16, content_type: &str, body: String) {
    let header = Header::from_bytes(&b"content-type"[..], content_type.as_bytes()).unwrap();
    let _ = request.respond(Response::from_string(body).with_status_code(status).with_header(header));
}

#[test]
fn a_custom_provider_is_saved_as_an_env_reference_and_can_be_tested() {
    let fake = Fake::start(Mode::Gateway);
    let f = setup();
    let ws = create(&f, "Personal", "personal");
    let path = format!("/api/ws/{ws}/providers");

    let added = f.ok("POST", &path, Some(json!({
        "id": "litellm", "name": "LiteLLM", "pkg": "@ai-sdk/openai-compatible",
        "url": fake.url, "env": "LITELLM_API_KEY", "model": "gateway-model", "key": KEY
    })));
    assert_eq!(added["provider"]["id"], "litellm");
    assert_eq!(added["provider"]["keyEnv"], "LITELLM_API_KEY");
    assert_eq!(added["provider"]["note"], format!("@ai-sdk/openai-compatible · {}", fake.url));
    assert!(added["opencodeJson"].as_str().unwrap().contains("{env:LITELLM_API_KEY}"));
    assert!(!added["opencodeJson"].as_str().unwrap().contains(KEY), "the key must never be written");

    let written = f.ok("GET", &format!("/api/ws/{ws}/opencode-json"), None);
    assert_eq!(written["json"]["provider"]["litellm"]["options"]["apiKey"], "{env:LITELLM_API_KEY}");
    // The workspace already had a default model, so adding a provider does not steal it.
    assert_eq!(written["json"]["model"], "opencode/big-pickle");

    // A saved provider is tested with its keychain key, and only then asks the model for a reply.
    let ok = f.ok("POST", &format!("{path}/test"), Some(json!({ "id": "litellm" })));
    assert_eq!(ok["ok"], true, "{ok}");
    assert!(fake.seen.lock().unwrap().contains(&"gateway-model".to_string()));
    let order = f.ok("POST", &format!("{path}/test"), Some(json!({ "id": "litellm", "pkg": "@ai-sdk/openai-compatible", "model": "probe-model" })));
    assert_eq!(order["ok"], true);
    assert_eq!(fake.seen.lock().unwrap().last().map(String::as_str), Some("probe-model"));
    let bad = f.ok("POST", &format!("{path}/test"), Some(json!({ "id": "litellm", "key": "nope" })));
    assert_eq!(bad["ok"], false);
    assert_eq!(bad["error"], "The key was rejected (401)");

    // Duplicate ids and empty model lists keep the TS error strings.
    assert_eq!(f.call("POST", &path, Some(json!({ "id": "litellm" }))).unwrap_err().message, "A provider with that ID already exists");
    assert_eq!(f.call("POST", &path, Some(json!({ "id": "  " }))).unwrap_err().message, "Provider ID is required");
    assert_eq!(f.call("PATCH", &format!("{path}/litellm"), Some(json!({ "models": [" "] }))).unwrap_err().message, "At least one model is required");

    // Editing keeps a known model's settings and adds the new one.
    f.ok("PATCH", &format!("{path}/litellm"), Some(json!({ "name": "Renamed", "url": format!("{}/", fake.url), "models": ["gateway-model", "second"] })));
    let snapshot = f.ok("GET", &format!("/api/ws/{ws}/snapshot"), None);
    let provider = snapshot["workspace"]["providers"].as_array().unwrap().iter().find(|p| p["id"] == "litellm").unwrap();
    assert_eq!(provider["name"], "Renamed");
    assert_eq!(provider["models"].as_array().unwrap().len(), 2);
    assert_eq!(provider["keyEnv"], "LITELLM_API_KEY");

    f.ok("DELETE", &format!("{path}/litellm"), None);
    let snapshot = f.ok("GET", &format!("/api/ws/{ws}/snapshot"), None);
    assert!(!snapshot["workspace"]["providers"].as_array().unwrap().iter().any(|p| p["id"] == "litellm"));
    let key = f.sup.with_core(|core| core.keychain.get(&ws, "LITELLM_API_KEY")).unwrap();
    assert_eq!(key, None, "deleting a provider removes its key");
    fake.stop();
}

#[test]
fn connection_tests_reject_plain_http_remote_hosts_and_web_pages() {
    let f = setup();
    let ws = create(&f, "Tests", "personal");
    let path = format!("/api/ws/{ws}/providers/test");

    let remote = f.ok("POST", &path, Some(json!({ "url": "http://gateway.example.test/v1" })));
    assert_eq!(remote["ok"], false);
    assert!(remote["error"].as_str().unwrap().contains("https"), "{remote}");

    let web = Fake::start(Mode::HtmlPage);
    let page = f.ok("POST", &path, Some(json!({ "url": web.url })));
    assert_eq!(page["ok"], false);
    assert_eq!(page["error"], "That URL does not serve the chat API. Check the Base URL (Open WebUI uses https://your-host/api).");
    web.stop();

    let no_chat = Fake::start(Mode::MethodNotAllowed);
    for model in [Value::Null, json!("gemma")] {
        let body = if model.is_null() { json!({ "url": no_chat.url }) } else { json!({ "url": no_chat.url, "model": model }) };
        let r = f.ok("POST", &path, Some(body));
        assert_eq!(r["ok"], false, "{r}");
        assert!(r["error"].as_str().unwrap().contains("Base URL"), "{r}");
    }
    no_chat.stop();
}

#[test]
fn web_providers_match_the_shared_table() {
    let f = setup();
    let table = f.ok("GET", "/api/web-providers", None);
    assert_eq!(table["Tavily"], json!({ "pkg": "tavily-mcp", "env": "TAVILY_API_KEY" }));
    assert_eq!(table["Brave"], json!({ "pkg": "@brave/brave-search-mcp-server", "env": "BRAVE_API_KEY" }));
    assert_eq!(table["Firecrawl"], json!({ "pkg": "firecrawl-mcp", "env": "FIRECRAWL_API_KEY" }));
}

#[test]
fn mcp_connectors_add_toggle_and_delete() {
    let f = setup();
    let ws = create(&f, "Connectors", "personal");
    let path = format!("/api/ws/{ws}/mcp");

    let stdio = f.ok("POST", &path, Some(json!({ "name": "Files", "command": "npx -y @modelcontextprotocol/server-filesystem /tmp" })));
    assert_eq!(stdio["kind"], "MCP · stdio");
    assert_eq!(stdio["type"], "local");
    assert_eq!(stdio["command"][0], "npx");
    let id = stdio["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("tmp-"), "id from the last command segment: {id}");

    let http = f.ok("POST", &path, Some(json!({ "command": "https://mcp.example.test/sse" })));
    assert_eq!(http["kind"], "MCP · http");
    assert_eq!(http["type"], "remote");
    assert_eq!(http["url"], "https://mcp.example.test/sse");

    assert_eq!(f.call("POST", &path, Some(json!({}))).unwrap_err().message, "Name or command required");

    let off = f.ok("PATCH", &format!("{path}/{id}"), Some(json!({ "on": false })));
    assert_eq!(off["on"], false);
    let written = f.ok("GET", &format!("/api/ws/{ws}/opencode-json"), None);
    assert!(written["json"]["mcp"].to_string().contains("\"enabled\":false"), "{written}");
    assert_eq!(f.call("PATCH", &format!("{path}/nope"), Some(json!({}))).unwrap_err().message, "Connector not found");
    assert_eq!(f.call("DELETE", &format!("{path}/nope"), None).unwrap_err().message, "Connector not found");

    let before = f.ok("GET", &format!("/api/ws/{ws}/snapshot"), None)["workspace"]["mcp"].as_array().unwrap().len();
    f.ok("DELETE", &format!("{path}/{id}"), None);
    assert_eq!(f.ok("GET", &format!("/api/ws/{ws}/snapshot"), None)["workspace"]["mcp"].as_array().unwrap().len(), before - 1);
}

#[test]
fn an_org_workspace_cannot_change_providers() {
    let f = setup();
    let org = f.ok("POST", "/api/workspaces/join-org", Some(json!({ "name": "Gateway Org", "email": "me@org.test" })))["id"].as_str().unwrap().to_string();
    let err = f.call("POST", &format!("/api/ws/{org}/providers"), Some(json!({ "id": "custom" }))).unwrap_err();
    assert_eq!(err.status, 403);
    assert_eq!(err.message, "Locked by org policy");
    let err = f.call("PATCH", &format!("/api/ws/{org}/mcp/x"), Some(json!({ "on": false }))).unwrap_err();
    assert_eq!(err.status, 404, "the mcp route looks the connector up first");
}
