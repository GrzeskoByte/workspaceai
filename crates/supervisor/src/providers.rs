//! Provider helpers: the OpenAI-compatible connection test (R-20) and the web search provider table (R-21).
//! Port of `testEndpoint` in `routes.ts` and `WEB_PROVIDERS` in `shared/opencode-config.ts`.

use serde_json::{json, Value};
use std::time::Duration;
use workspaceai_shared::util::now_ms;

const NOT_API: &str = "That URL does not serve the chat API. Check the Base URL (Open WebUI uses https://your-host/api).";

/// `WEB_PROVIDERS`, verbatim.
pub fn web_providers() -> Value {
    json!({
        "Tavily": { "pkg": "tavily-mcp", "env": "TAVILY_API_KEY" },
        "Brave": { "pkg": "@brave/brave-search-mcp-server", "env": "BRAVE_API_KEY" },
        "Firecrawl": { "pkg": "firecrawl-mcp", "env": "FIRECRAWL_API_KEY" },
    })
}

fn local_base(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    if u.starts_with("https://") {
        return true;
    }
    ["http://localhost", "http://127.0.0.1", "http://[::1]"]
        .iter()
        .any(|h| u.strip_prefix(h).is_some_and(|rest| rest.starts_with(':') || rest.starts_with('/')))
}

/// One HTTP call; `Ok` also for 4xx/5xx (the caller reads the status), `Err` for a transport failure.
fn http(method: &str, url: &str, key: Option<&str>, body: Option<&Value>, timeout: Duration) -> Result<(u16, String), String> {
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let mut request = agent.request(method, url).set("accept", "application/json");
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        request = request.set("authorization", &format!("Bearer {k}"));
    }
    let res = match body {
        Some(b) => request.set("content-type", "application/json").send_string(&b.to_string()),
        None => request.call(),
    };
    match res {
        Ok(r) => Ok((r.status(), r.into_string().unwrap_or_default())),
        Err(ureq::Error::Status(code, r)) => Ok((code, r.into_string().unwrap_or_default())),
        Err(e) => Err(e.to_string()),
    }
}

fn no_answer(base_url: &str, e: &str) -> Value {
    json!({ "ok": false, "error": format!("No answer from {base_url} ({})", e.chars().take(80).collect::<String>()) })
}

fn rejected(status: u16) -> Value {
    json!({ "ok": false, "error": format!("The key was rejected ({status})") })
}

fn model_ids(text: &str) -> (Option<Value>, Vec<String>) {
    let parsed = serde_json::from_str::<Value>(text).ok();
    let ids = parsed
        .as_ref()
        .and_then(|j| j["data"].as_array())
        .map(|a| a.iter().filter_map(|m| m["id"].as_str().map(str::to_string)).take(50).collect())
        .unwrap_or_default();
    (parsed, ids)
}

/// With a model it asks for a short reply (the request opencode sends); without one it only lists models.
pub fn test_endpoint(base_url: &str, key: Option<&str>, model: Option<&str>) -> Value {
    if !local_base(base_url) {
        return json!({ "ok": false, "error": "Use https://, or http:// only for localhost" });
    }
    let t0 = now_ms();
    let base = base_url.trim_end_matches('/');
    let (status, text) = match http("GET", &format!("{base}/models"), key, None, Duration::from_secs(8)) {
        Ok(v) => v,
        Err(e) => return no_answer(base_url, &e),
    };
    if status == 401 || status == 403 {
        return rejected(status);
    }
    let (parsed, models) = model_ids(&text);
    let Some(model) = model.map(str::trim).filter(|m| !m.is_empty()) else {
        if !(200..300).contains(&status) {
            return json!({ "ok": false, "error": format!("Endpoint answered {status}") });
        }
        return match parsed {
            Some(_) => json!({ "ok": true, "models": models, "ms": now_ms() - t0 }),
            None => json!({ "ok": false, "error": NOT_API }),
        };
    };
    let body = json!({ "model": model, "messages": [{ "role": "user", "content": "Reply with OK." }], "max_tokens": 8 });
    let (status, text) = match http("POST", &format!("{base}/chat/completions"), key, Some(&body), Duration::from_secs(60)) {
        Ok(v) => v,
        Err(e) => return no_answer(base_url, &e),
    };
    if status == 401 || status == 403 {
        return rejected(status);
    }
    if status == 405 {
        return json!({ "ok": false, "error": NOT_API });
    }
    let reply = serde_json::from_str::<Value>(&text).ok();
    if !(200..300).contains(&status) {
        let why = reply
            .as_ref()
            .and_then(|r| r["error"].as_str().map(str::to_string).or_else(|| r["error"]["message"].as_str().map(str::to_string)).or_else(|| r["detail"].as_str().map(str::to_string)))
            .unwrap_or_default();
        let why: String = why.chars().take(160).collect();
        let suffix = if why.is_empty() { String::new() } else { format!(" · {why}") };
        return json!({ "ok": false, "error": format!("Model {model} failed: endpoint answered {status}{suffix}") });
    }
    let has_choices = reply.as_ref().and_then(|r| r["choices"].as_array()).is_some_and(|c| !c.is_empty());
    if !has_choices {
        return json!({ "ok": false, "error": NOT_API });
    }
    json!({ "ok": true, "models": models, "ms": now_ms() - t0 })
}
