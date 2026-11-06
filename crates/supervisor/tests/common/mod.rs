//! Shared test fixture: one temp `HARNESS_HOME` per test, memory keychain, thin wrappers over `handle`.
#![allow(dead_code)]

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Once;
use workspaceai_supervisor::{ApiError, Supervisor};

static KEYCHAIN_ENV: Once = Once::new();

pub struct Fixture {
    pub sup: Supervisor,
    pub home: tempfile::TempDir,
}

impl Fixture {
    pub fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, ApiError> {
        self.sup.handle(method, path, &BTreeMap::new(), body)
    }

    pub fn call_with_query(&self, method: &str, path: &str, query: &[(&str, &str)]) -> Result<Value, ApiError> {
        let query: BTreeMap<String, String> = query.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        self.sup.handle(method, path, &query, None)
    }

    /// Like `ok()` in the TS tests: panics with the status and message the route answered.
    pub fn ok(&self, method: &str, path: &str, body: Option<Value>) -> Value {
        match self.call(method, path, body) {
            Ok(v) => v,
            Err(e) => panic!("{method} {path} → {} {}", e.status, e.message),
        }
    }

    pub fn path(&self) -> &Path {
        self.home.path()
    }
}

pub fn setup() -> Fixture {
    KEYCHAIN_ENV.call_once(|| std::env::set_var("HARNESS_KEYCHAIN", "memory"));
    let home = tempfile::tempdir().expect("temp home");
    let sup = Supervisor::new(home.path().to_path_buf()).expect("supervisor");
    Fixture { sup, home }
}

pub fn create(f: &Fixture, name: &str, kind: &str) -> String {
    f.ok("POST", "/api/workspaces", Some(serde_json::json!({ "name": name, "kind": kind })))[ "id" ]
        .as_str()
        .expect("workspace id")
        .to_string()
}
