//! Workspace lifecycle: create, delete, activate (port of the same methods in `supervisor.ts`).

use super::core::{Core, CreateOptions};
use super::events::ApiError;
use crate::paths::valid_id;
use crate::runtime::Runtime;
use workspaceai_shared::defaults::{make_workspace, provider_presets};
use workspaceai_shared::types::WorkspaceKind;
use workspaceai_shared::util::uid;

impl Core {
    pub fn create_workspace(&mut self, name: &str, kind: WorkspaceKind, opts: CreateOptions) -> Result<String, ApiError> {
        let clean = name.trim();
        if clean.is_empty() {
            return Err(ApiError::bad_request("Name the workspace"));
        }
        let id = if kind == WorkspaceKind::Org {
            format!("org_{}", workspaceai_shared::util::slugify(clean, "org"))
        } else {
            uid("ws")
        };
        if !valid_id(&id) {
            return Err(ApiError::bad_request("That workspace name is not usable as an id"));
        }
        if self.runtimes.contains_key(&id) {
            return Err(ApiError::bad_request(format!("You are already in {clean}")));
        }
        let mut ws = make_workspace(&id, clean, kind, &self.home.to_string_lossy());
        ws.providers = match opts.providers {
            Some(p) => p,
            None if kind == WorkspaceKind::Org => vec![],
            None => {
                let mut default = provider_presets().into_iter().find(|(id, _)| *id == "opencode").map(|(_, p)| p).unwrap_or_default();
                default.is_default = Some(true);
                vec![default]
            }
        };
        ws.default_model = opts.default_model.unwrap_or_else(|| match ws.providers.first() {
            Some(p) => match p.models.first() {
                Some(m) => format!("{}/{}", p.id, m.id),
                None => String::new(),
            },
            None => String::new(),
        });
        let rt = Runtime::new(ws, &self.home, self.events.clone(), self.keychain.clone())?;
        rt.save()?;
        rt.write_scopes_manifest()?;
        rt.audit("workspace.create", &format!("{clean} ({})", kind.as_str()), "you");
        self.runtimes.insert(id.clone(), rt);
        if self.settings.active_workspace.is_none() {
            self.settings.active_workspace = Some(id.clone());
        }
        self.save_settings()?;
        Ok(id)
    }

    pub fn delete_workspace(&mut self, id: &str) -> Result<(), ApiError> {
        let rt = self.rt(id)?;
        let last_personal = rt.ws.kind == WorkspaceKind::Personal && self.ordered().iter().filter(|r| r.ws.kind == WorkspaceKind::Personal).count() == 1;
        if last_personal {
            return Err(ApiError::bad_request("Your personal workspace can’t be deleted"));
        }
        let root = rt.paths.root.clone();
        self.keychain.purge(id);
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        self.runtimes.remove(id);
        for c in &mut self.computers {
            c.access.remove(id);
        }
        if self.settings.active_workspace.as_deref() == Some(id) {
            self.settings.active_workspace = self.ordered().first().map(|r| r.id.clone());
        }
        self.save_computers()?;
        self.save_settings()?;
        Ok(())
    }

    pub fn activate(&mut self, id: &str) -> Result<(), ApiError> {
        self.rt(id)?;
        self.settings.active_workspace = Some(id.to_string());
        self.save_settings()
    }
}
