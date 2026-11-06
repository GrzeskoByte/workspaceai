//! `ApiError` (what `routes.ts` throws) and `UiEvent` (what the TS supervisor emits).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::io;
use std::sync::{Arc, Mutex};

/// A message plus the HTTP status the loopback layer answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl ApiError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        ApiError { status, message: message.into() }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(404, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(403, message)
    }

    /// Wire form of an error: `{"error": message}`.
    pub fn body(&self) -> Value {
        json!({ "error": self.message })
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<io::Error> for ApiError {
    fn from(e: io::Error) -> Self {
        ApiError::new(500, e.to_string())
    }
}

/// The events the UI listens to. Only the variants this batch emits are modelled; the tag matches `UiEvent` in
/// `shared/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum UiEvent {
    Snapshot { workspace: String },
    State { workspace: Option<String> },
}

pub type Listener = Arc<dyn Fn(UiEvent) + Send + Sync>;

/// Fan-out shared by the core and every runtime, so a runtime can wake the UI without knowing the core.
#[derive(Clone, Default)]
pub struct Emitter {
    listeners: Arc<Mutex<Vec<Listener>>>,
}

impl Emitter {
    pub fn on(&self, listener: Listener) {
        self.listeners.lock().unwrap_or_else(|e| e.into_inner()).push(listener);
    }

    pub fn emit(&self, event: UiEvent) {
        // Snapshot the list: a listener may subscribe or emit while it runs.
        let listeners = self.listeners.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for l in listeners {
            l(event.clone());
        }
    }
}
