//! Port of `keychain.ts`. Secrets live in the OS keychain under service `harness`, account `<ws>/<name>` (R-34);
//! config files only reference them. `HARNESS_KEYCHAIN=memory` keeps them in process memory (tests, CI).

use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "harness";

pub struct Keychain {
    /// `Some` = in-memory backend.
    mem: Option<Mutex<HashMap<String, String>>>,
}

impl Keychain {
    pub fn from_env() -> Self {
        if std::env::var("HARNESS_KEYCHAIN").as_deref() == Ok("memory") {
            Self::memory()
        } else {
            Keychain { mem: None }
        }
    }

    pub fn memory() -> Self {
        Keychain { mem: Some(Mutex::new(HashMap::new())) }
    }

    /// The real OS keychain, regardless of `HARNESS_KEYCHAIN` (used to test the OS path itself).
    pub fn os() -> Self {
        Keychain { mem: None }
    }

    pub fn backend_name(&self) -> &'static str {
        if self.mem.is_some() { "memory" } else { "os" }
    }

    fn raw_get(&self, account: &str) -> Option<String> {
        match &self.mem {
            Some(m) => m.lock().unwrap().get(account).cloned(),
            None => keyring::Entry::new(SERVICE, account).ok()?.get_password().ok(),
        }
    }

    fn raw_set(&self, account: &str, value: &str) -> Result<(), String> {
        match &self.mem {
            Some(m) => {
                m.lock().unwrap().insert(account.into(), value.into());
                Ok(())
            }
            None => keyring::Entry::new(SERVICE, account).and_then(|e| e.set_password(value)).map_err(|e| e.to_string()),
        }
    }

    fn raw_del(&self, account: &str) {
        match &self.mem {
            Some(m) => {
                m.lock().unwrap().remove(account);
            }
            None => {
                if let Ok(e) = keyring::Entry::new(SERVICE, account) {
                    let _ = e.delete_credential(); // absent is fine
                }
            }
        }
    }

    /// Index of secret names per workspace, so we can list and delete without enumerating the OS keychain.
    fn index_account(ws: &str) -> String {
        format!("{ws}/__index__")
    }

    fn read_index(&self, ws: &str) -> Vec<String> {
        self.raw_get(&Self::index_account(ws)).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn write_index(&self, ws: &str, idx: &[String]) -> Result<(), String> {
        self.raw_set(&Self::index_account(ws), &serde_json::to_string(idx).unwrap())
    }

    pub fn get(&self, ws: &str, name: &str) -> Option<String> {
        self.raw_get(&format!("{ws}/{name}"))
    }

    pub fn set(&self, ws: &str, name: &str, value: &str) -> Result<(), String> {
        self.raw_set(&format!("{ws}/{name}"), value)?;
        let mut idx = self.read_index(ws);
        if !idx.iter().any(|n| n == name) {
            idx.push(name.into());
            self.write_index(ws, &idx)?;
        }
        Ok(())
    }

    pub fn has(&self, ws: &str, name: &str) -> bool {
        self.get(ws, name).is_some()
    }

    pub fn delete(&self, ws: &str, name: &str) -> Result<(), String> {
        self.raw_del(&format!("{ws}/{name}"));
        let idx: Vec<String> = self.read_index(ws).into_iter().filter(|n| n != name).collect();
        self.write_index(ws, &idx)
    }

    pub fn list(&self, ws: &str) -> Vec<String> {
        self.read_index(ws)
    }

    /// Removes every secret a workspace owns (workspace delete / leave org).
    pub fn purge(&self, ws: &str) {
        for n in self.read_index(ws) {
            self.raw_del(&format!("{ws}/{n}"));
        }
        self.raw_del(&Self::index_account(ws));
    }
}
