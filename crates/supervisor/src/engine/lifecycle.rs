//! The `Runtime` engine methods: environment, client, start, restart, stop, status, model cache.
//! Port of the engine part of `runtime.ts`. One managed server per workspace; remote mode never spawns.

use super::client::OpencodeClient;
use super::detect::opencode_binary;
use super::remote::{port_of, remote_url_allowed};
use super::server::{tail, ManagedOptions, ManagedServer};
use crate::jsonfile::write_json;
use crate::paths::expand_home;
use crate::runtime::Runtime;
use crate::supervisor::ApiError;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use workspaceai_shared::opencode_config::{build_opencode_json, referenced_env};
use workspaceai_shared::types::{EngineMode, EngineStatus, WorkspaceKind};
use workspaceai_shared::util::now_ms;

/// Env vars a workspace process keeps from the supervisor's own environment (R-06: nothing else leaks).
const KEEP_ENV: [&str; 13] = [
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TERM", "SHELL", "TMPDIR", "DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS",
];

fn base_env() -> BTreeMap<String, String> {
    KEEP_ENV.iter().filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v))).collect()
}

impl Runtime {
    fn lock_path(&self) -> PathBuf {
        self.paths.data.join("opencode/runtime.json")
    }

    pub fn touch(&self) {
        self.last_used.store(now_ms(), Ordering::SeqCst);
    }

    pub fn engine_running(&self) -> bool {
        self.engine.as_ref().is_some_and(|s| s.is_running())
    }

    /// Live status: derived from the process, so an idle stop shows up without a route call.
    pub fn engine_status(&self) -> EngineStatus {
        match &self.engine {
            Some(server) => server.status(),
            None => self.status.clone(),
        }
    }

    pub fn engine_logs(&self) -> String {
        tail(&self.paths.logs.join("opencode.log"), 500)
    }

    /// Environment for this workspace's process group: base env, XDG isolation, keychain secrets.
    pub fn engine_env(&self) -> BTreeMap<String, String> {
        let built = build_opencode_json(&self.ws, self.policy.as_ref(), &serde_json::Map::new());
        let mut out = base_env();
        for (k, v) in [
            ("XDG_DATA_HOME", &self.paths.xdg_data),
            ("XDG_CONFIG_HOME", &self.paths.xdg_config),
            ("XDG_STATE_HOME", &self.paths.xdg_state),
            ("XDG_CACHE_HOME", &self.paths.xdg_cache),
        ] {
            out.insert(k.to_string(), v.to_string_lossy().into_owned());
        }
        out.insert("OPENCODE_CONFIG".into(), self.paths.opencode_json.to_string_lossy().into_owned());
        out.extend(built.env);
        for name in referenced_env(&built.json) {
            if let Some(v) = self.keychain.get(&self.id, &name) {
                out.insert(name, v);
            }
        }
        if self.ws.kind == WorkspaceKind::Org {
            if let Some(policy) = &self.policy {
                out.insert("NO_PROXY".into(), format!("{},127.0.0.1,localhost", policy.allowed_endpoints.join(",")));
            }
        }
        out
    }

    pub fn engine_client(&mut self) -> Result<OpencodeClient, ApiError> {
        self.touch();
        if self.ws.engine.mode == EngineMode::Remote {
            if let Some(client) = &self.remote {
                return Ok(client.clone());
            }
            return self.connect_remote();
        }
        if self.engine_running() {
            return Ok(self.engine.as_ref().expect("checked").client());
        }
        self.start_managed()
    }

    fn start_managed(&mut self) -> Result<OpencodeClient, ApiError> {
        self.write_config()?;
        self.status = EngineStatus { state: "starting".into(), ..Default::default() };
        let binary = opencode_binary(&self.home, &self.ws.engine).map_err(ApiError::bad_request)?;
        let cwd = self
            .state
            .projects
            .iter()
            .find(|p| p.is_general == Some(true))
            .map(|p| PathBuf::from(expand_home(&p.folder)))
            .unwrap_or_else(|| self.paths.root.clone());
        let mut server = ManagedServer::new(
            ManagedOptions { binary, cwd, env: self.engine_env(), log_dir: self.paths.logs.clone(), idle_minutes: self.ws.engine.idle_stop_minutes },
            self.last_used.clone(),
        );
        if let Err(e) = server.start() {
            self.status = EngineStatus { state: "error".into(), error: Some(e.clone()), ..Default::default() };
            return Err(ApiError::bad_request(e));
        }
        let _ = self.keychain.set(&self.id, "OPENCODE_ENGINE_PASSWORD", &server.password);
        let _ = write_json(&self.lock_path(), &json!({ "pid": server.pid(), "url": server.url() }));
        self.status = server.status();
        self.engine = Some(server);
        self.audit("engine.start", &format!("opencode {} on 127.0.0.1:{}", self.status.version, self.status.port), "workspaceai");
        Ok(self.engine.as_ref().expect("just set").client())
    }

    fn connect_remote(&mut self) -> Result<OpencodeClient, ApiError> {
        let engine = self.ws.engine.clone();
        remote_url_allowed(&engine.url).map_err(ApiError::bad_request)?;
        let secret = engine.password_secret.clone().unwrap_or_else(|| "OPENCODE_SERVER_PASSWORD".into());
        let password = self.keychain.get(&self.id, &secret).unwrap_or_default();
        let username = if engine.username.is_empty() { "opencode" } else { engine.username.as_str() };
        let client = OpencodeClient::new(&engine.url, username, &password);
        let info = client.info().map_err(|e| ApiError::bad_request(e.to_string()))?;
        self.status = EngineStatus {
            state: "connected".into(),
            url: engine.url.clone(),
            port: port_of(&engine.url),
            version: info.version,
            ..Default::default()
        };
        self.remote = Some(client.clone());
        Ok(client)
    }

    pub fn stop_engine(&mut self) {
        if let Some(mut server) = self.engine.take() {
            server.stop();
        }
        self.remote = None;
        self.status = EngineStatus { state: "stopped".into(), url: self.status.url.clone(), port: self.status.port, ..Default::default() };
        let _ = std::fs::remove_file(self.lock_path());
        let _ = self.keychain.delete(&self.id, "OPENCODE_ENGINE_PASSWORD");
    }

    pub fn restart_engine(&mut self) -> Result<(), ApiError> {
        self.status = EngineStatus { state: "restarting".into(), ..Default::default() };
        self.stop_engine();
        self.engine_client().map(|_| ())
    }

    /// `engine/settings` changes reach a stopped engine on its next start; a running one restarts.
    pub fn config_changed(&mut self) -> Result<(), ApiError> {
        if self.engine_running() {
            self.restart_engine()
        } else {
            self.write_config()
        }
    }

    /// Models opencode reports, cached in `state.json` for when the engine is stopped (`engineModels`).
    pub fn engine_models(&mut self) -> Vec<Value> {
        if self.engine_running() || self.remote.is_some() {
            if let Ok(client) = self.engine_client() {
                if let Ok(list) = client.models(None) {
                    let live: Vec<Value> = list
                        .iter()
                        .filter(|m| m["enabled"].as_bool() != Some(false) && m["status"].as_str() != Some("deprecated"))
                        .map(|m| {
                            let efforts: Vec<&str> = m["variants"].as_array().map(|a| a.iter().filter_map(|v| v["id"].as_str()).collect()).unwrap_or_default();
                            let id = m["id"].as_str().unwrap_or_default();
                            let mut out = json!({
                                "id": id,
                                "providerID": m["providerID"].as_str().unwrap_or_default(),
                                "name": m["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(id),
                            });
                            if !efforts.is_empty() {
                                out["efforts"] = json!(efforts);
                            }
                            out
                        })
                        .collect();
                    self.state.extra.insert("engineModels".into(), json!(live));
                    let _ = self.save();
                    return live;
                }
            }
        }
        self.state.extra.get("engineModels").and_then(|v| v.as_array()).cloned().unwrap_or_default()
    }

    /// Providers this workspace can reach: key in the keychain, none needed, local, or reused from opencode.
    pub fn connected_providers(&self) -> Vec<workspaceai_shared::types::Provider> {
        let local = |u: Option<&str>| {
            u.is_some_and(|u| {
                let u = u.to_ascii_lowercase();
                ["http://localhost", "http://127.0.0.1", "http://[::1]"].iter().any(|h| u.starts_with(&format!("{h}:")) || u.starts_with(&format!("{h}/")))
            })
        };
        workspaceai_shared::opencode_config::effective_providers(&self.ws, self.policy.as_ref())
            .into_iter()
            .filter(|p| {
                p.from_opencode == Some(true)
                    || p.key_env.as_deref().unwrap_or("").is_empty()
                    || local(p.base_url.as_deref())
                    || p.key_env.as_deref().is_some_and(|k| self.keychain.has(&self.id, k))
            })
            .collect()
    }
}
