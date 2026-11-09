//! `Core`: app settings, computers and every workspace runtime.

use super::events::{ApiError, Emitter, Listener, UiEvent};
use crate::jsonfile::{read_json, write_json};
use crate::runtime::Runtime;
use crate::keychain::Keychain;
use crate::paths::{app_json_path, computers_json_path, valid_id, WsPaths};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use workspaceai_shared::types::{AppSettings, Computer, Workspace};
use workspaceai_shared::util::initials_of;

/// Options for `create_workspace` (`opts` in the TS).
#[derive(Debug, Clone, Default)]
pub struct CreateOptions {
    pub providers: Option<Vec<workspaceai_shared::types::Provider>>,
    pub default_model: Option<String>,
}

/// What an SSO sign-in result carries (only the demo org exists for now).
#[derive(Debug, Clone)]
pub struct Sso {
    pub issuer: String,
    pub subject: String,
    pub idp: String,
}

impl Default for Sso {
    fn default() -> Self {
        Sso {
            issuer: "https://login.[ORG-HOST]/realms/caim".into(),
            subject: "you@[ORG-HOST]".into(),
            idp: "MyAccessID".into(),
        }
    }
}

/// Org policy overrides offered by `/api/workspaces/join-org`; `None` keeps the demo policy value.
#[derive(Debug, Clone, Default)]
pub struct PolicyOverrides {
    pub audit_url: Option<String>,
    pub audit_every_minutes: Option<i64>,
    pub log_content: Option<bool>,
    pub gateway: Option<String>,
}

pub struct Core {
    pub home: PathBuf,
    pub settings: AppSettings,
    pub computers: Vec<Computer>,
    pub keychain: Arc<Keychain>,
    pub runtimes: BTreeMap<String, Runtime>,
    pub events: Emitter,
    /// The loopback listener's URL and token while it runs (`server::serve`).
    pub loopback: crate::server::LoopbackSlot,
}

fn default_settings() -> AppSettings {
    AppSettings { theme: "claude".into(), accent: "default".into(), ..Default::default() }
}

/// `{ ...DEFAULT_SETTINGS, ...app.json }`, as the TS constructor does (shallow merge).
fn load_settings(home: &PathBuf) -> AppSettings {
    let mut base = serde_json::to_value(default_settings()).unwrap_or_else(|_| json!({}));
    if let Some(Value::Object(read)) = read_json::<Value>(&app_json_path(home)) {
        if let Value::Object(map) = &mut base {
            for (k, v) in read {
                map.insert(k, v);
            }
        }
    }
    serde_json::from_value(base).unwrap_or_default()
}

fn load_workspace(home: &PathBuf, id: &str) -> Option<Workspace> {
    read_json::<Workspace>(&WsPaths::new(home, id).harness_json)
}

impl Core {
    pub fn new(home: PathBuf) -> Result<Self, ApiError> {
        std::fs::create_dir_all(&home)?;
        let settings = load_settings(&home);
        let computers: Vec<Computer> = read_json(&computers_json_path(&home)).unwrap_or_default();
        let events = Emitter::default();
        let keychain = Arc::new(Keychain::from_env());
        let loopback = crate::server::LoopbackSlot::default();
        let mut runtimes = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(home.join("ws")) {
            for entry in entries.flatten() {
                let id = entry.file_name().to_string_lossy().to_string();
                // Known debt (PORT-STATUS): ids that are not one safe path segment are skipped, not loaded.
                if !valid_id(&id) {
                    continue;
                }
                if let Some(ws) = load_workspace(&home, &id) {
                    runtimes.insert(id.clone(), Runtime::new(ws, &home, events.clone(), keychain.clone(), loopback.clone())?);
                }
            }
        }
        Ok(Core { home, settings, computers, keychain, runtimes, events, loopback })
    }

    /// Stops every managed engine. Idempotent; `Supervisor::shutdown` and `Drop` both call it.
    pub fn shutdown(&mut self) {
        for rt in self.runtimes.values_mut() {
            rt.stop_engine();
        }
    }

    pub fn on(&self, listener: Listener) {
        self.events.on(listener);
    }

    pub fn emit(&self, event: UiEvent) {
        self.events.emit(event);
    }

    // ---------- app ----------
    pub fn save_settings(&mut self) -> Result<(), ApiError> {
        write_json(&app_json_path(&self.home), &self.settings)?;
        self.emit(UiEvent::State { workspace: self.settings.active_workspace.clone() });
        Ok(())
    }

    pub fn save_computers(&mut self) -> Result<(), ApiError> {
        write_json(&computers_json_path(&self.home), &self.computers)?;
        self.emit(UiEvent::State { workspace: self.settings.active_workspace.clone() });
        Ok(())
    }

    pub fn initials_for(&self, name: &str) -> String {
        initials_of(name)
    }

    pub fn rt(&self, id: &str) -> Result<&Runtime, ApiError> {
        self.runtimes.get(id).ok_or_else(|| ApiError::not_found("Workspace not found"))
    }

    pub fn rt_mut(&mut self, id: &str) -> Result<&mut Runtime, ApiError> {
        self.runtimes.get_mut(id).ok_or_else(|| ApiError::not_found("Workspace not found"))
    }
}
