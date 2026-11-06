//! The in-process entry point: one mutex around `Core`, `handle()` mirrors the TS router.

use super::core::Core;
use super::events::ApiError;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Supervisor {
    inner: Mutex<Core>,
}

impl Supervisor {
    pub fn new(home: PathBuf) -> Result<Self, ApiError> {
        Ok(Supervisor { inner: Mutex::new(Core::new(home)?) })
    }

    pub fn with_default_home() -> Result<Self, ApiError> {
        Self::new(crate::paths::harness_home())
    }

    pub fn handle(&self, method: &str, path: &str, query: &BTreeMap<String, String>, body: Option<Value>) -> Result<Value, ApiError> {
        let mut core = self.inner.lock().map_err(|_| ApiError::new(500, "Supervisor lock poisoned"))?;
        crate::router::dispatch(&mut core, method, path, query, body)
    }

    /// For tests and the app: read something from the core without a route.
    pub fn with_core<T>(&self, f: impl FnOnce(&Core) -> T) -> Result<T, ApiError> {
        let core = self.inner.lock().map_err(|_| ApiError::new(500, "Supervisor lock poisoned"))?;
        Ok(f(&core))
    }

    /// Same, for the few callers that change state directly (scheduler, tests).
    pub fn with_core_mut<T>(&self, f: impl FnOnce(&mut Core) -> T) -> Result<T, ApiError> {
        let mut core = self.inner.lock().map_err(|_| ApiError::new(500, "Supervisor lock poisoned"))?;
        Ok(f(&mut core))
    }

    /// Stops every managed opencode server. Idempotent; safe to call on quit.
    pub fn shutdown(&self) -> Result<(), ApiError> {
        let mut core = self.inner.lock().map_err(|_| ApiError::new(500, "Supervisor lock poisoned"))?;
        core.shutdown();
        Ok(())
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        // Belt and braces: `ManagedServer::drop` also kills its child, but a poisoned lock must not leak a process.
        match self.inner.get_mut() {
            Ok(core) => core.shutdown(),
            Err(e) => e.into_inner().shutdown(),
        }
    }
}
