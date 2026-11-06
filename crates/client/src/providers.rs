//! Model providers and their API keys. Keys live in the OS keychain behind the supervisor; this module only
//! builds requests and says whether a key is stored. It never keeps or prints a key.

use crate::api::seg;
use crate::types::Provider;
use serde_json::{json, Value};

/// Longest key accepted; anything longer is almost certainly a paste mistake and is refused, not cut.
const MAX_KEY: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    NotNeeded,
    Missing,
    Stored,
}

impl KeyState {
    pub fn text(self) -> &'static str {
        match self {
            KeyState::NotNeeded => "No key needed",
            KeyState::Missing => "No key yet",
            KeyState::Stored => "Key stored in the keychain",
        }
    }
}

pub fn key_state(p: &Provider, stored_names: &[String]) -> KeyState {
    match p.key_env.as_deref() {
        None | Some("") => KeyState::NotNeeded,
        Some(env) if stored_names.iter().any(|n| n == env) => KeyState::Stored,
        Some(_) => KeyState::Missing,
    }
}

/// `PUT …/secrets/:name` body. Whitespace around a pasted key is dropped.
pub fn secret_body(value: &str) -> Option<Value> {
    let v = value.trim();
    (!v.is_empty() && v.len() <= MAX_KEY).then(|| json!({ "value": v }))
}

/// How the server names a secret: upper case, anything but letters, digits and `_` becomes `_`.
pub fn secret_name(raw: &str) -> Option<String> {
    (!raw.is_empty()).then(|| raw.to_uppercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect())
}

pub fn secrets_list_path(workspace: &str) -> String {
    format!("/api/ws/{}/secrets", seg(workspace))
}

pub fn secret_path(workspace: &str, name: &str) -> String {
    format!("{}/{}", secrets_list_path(workspace), seg(name))
}

pub fn provider_path(workspace: &str, provider: &str) -> String {
    format!("/api/ws/{}/providers/{}", seg(workspace), seg(provider))
}

pub const PACKAGE: &str = "@ai-sdk/openai-compatible";

/// Starting points for the custom provider form: (label, id, name, url, key env, model).
pub const PRESETS: [(&str, &str, &str, &str, &str, &str); 3] = [
    ("LiteLLM proxy", "litellm", "LiteLLM proxy", "http://localhost:4000/v1", "LITELLM_API_KEY", "[MODEL ALIAS]"),
    ("Ollama", "ollama", "Ollama", "http://localhost:11434/v1", "OLLAMA_API_KEY", "qwen3-coder"),
    ("OpenAI-compatible", "my-endpoint", "My endpoint", "https://[YOUR-ENDPOINT]/v1", "MY_ENDPOINT_KEY", "my-model"),
];

/// What the custom provider form holds. `key` is only ever sent, never shown or logged.
#[derive(Clone, Default)]
pub struct CustomForm {
    pub id: String,
    pub name: String,
    pub url: String,
    pub model: String,
    pub env: String,
    pub key: String,
}

impl std::fmt::Debug for CustomForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomForm").field("id", &self.id).field("url", &self.url).finish_non_exhaustive()
    }
}

pub fn providers_path(workspace: &str) -> String {
    format!("/api/ws/{}/providers", seg(workspace))
}

pub fn test_path(workspace: &str) -> String {
    format!("{}/test", providers_path(workspace))
}

fn http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// `POST …/providers` body, or the reason the form cannot be sent yet.
pub fn custom_body(f: &CustomForm) -> Result<Value, &'static str> {
    if f.id.trim().is_empty() {
        return Err("Give the provider an ID.");
    }
    if f.name.trim().is_empty() {
        return Err("Give the provider a name.");
    }
    if !http_url(f.url.trim()) {
        return Err("The base URL must start with http:// or https://.");
    }
    let mut body = json!({ "id": f.id.trim(), "name": f.name.trim(), "url": f.url.trim(), "model": f.model.trim(), "pkg": PACKAGE });
    if !f.env.trim().is_empty() {
        body["env"] = json!(f.env.trim());
    }
    if let Some(k) = secret_body(&f.key) {
        body["key"] = k["value"].clone();
    }
    Ok(body)
}

/// `POST …/providers/test` body; the model is only used for a real reply.
pub fn test_body(f: &CustomForm) -> Result<Value, &'static str> {
    if !http_url(f.url.trim()) {
        return Err("The base URL must start with http:// or https://.");
    }
    let mut body = json!({ "url": f.url.trim(), "pkg": PACKAGE });
    if !f.model.trim().is_empty() {
        body["model"] = json!(f.model.trim());
    }
    if let Some(k) = secret_body(&f.key) {
        body["key"] = k["value"].clone();
    }
    Ok(body)
}

/// "Connected · model replied · 120 ms", or the reason it failed.
pub fn check_text(reply: &Value, model: &str) -> String {
    if reply["ok"].as_bool() != Some(true) {
        return reply["error"].as_str().filter(|e| !e.is_empty()).unwrap_or("No answer").to_string();
    }
    let model = model.trim();
    let mut s = if model.is_empty() { "Connected · add a model id to test a real reply".to_string() } else { format!("Connected · {model} replied") };
    if let Some(ms) = reply["ms"].as_u64().filter(|m| *m > 0) {
        s.push_str(&format!(" · {ms} ms"));
    }
    s
}
