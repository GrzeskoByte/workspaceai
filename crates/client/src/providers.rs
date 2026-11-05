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
