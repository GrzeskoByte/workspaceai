//! Projects (R-09, R-10, R-11) and their instruction layers (R-12): port of the project section of
//! `runtime.ts`. Project folders stay on disk when a project is deleted; only harness files go.

use crate::jsonfile::{read_text, write_text};
use crate::paths::expand_home;
use crate::runtime::Runtime;
use crate::supervisor::ApiError;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use workspaceai_shared::types::{Artifact, Project};
use workspaceai_shared::util::{descendants, is_within, now_ms, project_path, project_path_name, slugify, uid};

/// One row of `GET /api/ws/:id/projects/:pid/layers`: broad (workspace) to narrow (project).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstructionLayer {
    pub name: String,
    pub file: String,
    pub text: String,
    /// `workspace` | `parent` | `project`
    pub kind: String,
}

impl Runtime {
    pub fn project(&self, id: &str) -> Option<&Project> {
        self.state.projects.iter().find(|p| p.id == id)
    }

    pub fn general(&self) -> Option<&Project> {
        self.state.projects.iter().find(|p| p.is_general == Some(true))
    }

    /// `createProject`: a folder per project, nested under the parent (or `<home>/projects/<slug>`).
    pub fn create_project(&mut self, name: &str, parent: Option<&str>, folder: Option<&str>) -> Result<Project, ApiError> {
        let clean = name.trim();
        if clean.is_empty() {
            return Err(ApiError::bad_request("Name the project"));
        }
        let parent = parent.filter(|p| !p.is_empty());
        let par = match parent {
            Some(pid) => Some(self.project(pid).ok_or_else(|| ApiError::bad_request("Parent project not found"))?.clone()),
            None => None,
        };
        let seg = slugify(clean, "project");
        let dir = match folder.filter(|f| !f.is_empty()) {
            Some(f) => absolute(&expand_home(f))?,
            None => match &par {
                Some(p) => PathBuf::from(expand_home(&p.folder)).join(seg),
                None => self.home.join("projects").join(&self.ws.slug).join(seg),
            },
        };
        std::fs::create_dir_all(&dir)?;
        let p = Project {
            id: uid("proj"),
            name: clean.into(),
            parent: parent.map(str::to_string),
            folder: dir.to_string_lossy().into_owned(),
            inherit: true,
            model: String::new(),
            archived: false,
            created_at: now_ms(),
            ..Default::default()
        };
        self.state.projects.push(p.clone());
        let detail = project_path_name(&self.state.projects, Some(&p.id));
        self.audit("project.create", &detail, "you");
        self.changed()?;
        Ok(p)
    }

    /// `updateProject`: name, inherit, model, parent and vaults. Vault validation arrives with B12.
    pub fn update_project(&mut self, id: &str, patch: &Value) -> Result<Project, ApiError> {
        let Some(obj) = patch.as_object() else {
            return Err(ApiError::bad_request("Expected a JSON object"));
        };
        let projects = &self.state.projects;
        let mut p = projects.iter().find(|x| x.id == id).cloned().ok_or_else(|| ApiError::bad_request("Project not found"))?;
        if let Some(parent) = obj.get("parent") {
            if p.is_general == Some(true) {
                return Err(ApiError::bad_request("General stays at the top level"));
            }
            let next = parent.as_str();
            if next == Some(id) || next.is_some_and(|n| descendants(projects, id).iter().any(|d| d == n)) {
                return Err(ApiError::bad_request("A project cannot move inside itself"));
            }
            p.parent = next.map(str::to_string);
        }
        if let Some(name) = obj.get("name") {
            let name = name.as_str().unwrap_or_default().trim();
            if name.is_empty() {
                return Err(ApiError::bad_request("Name the project"));
            }
            p.name = name.into();
        }
        if let Some(v) = obj.get("inherit") {
            p.inherit = v.as_bool().unwrap_or(true);
        }
        if let Some(v) = obj.get("model") {
            p.model = v.as_str().unwrap_or_default().into();
        }
        if let Some(v) = obj.get("vaults") {
            // ponytail: stored as given; `cleanAttachments` checks land with the vault routes (B12).
            p.vaults = serde_json::from_value(v.clone()).map_err(|e| ApiError::bad_request(format!("Bad vaults: {e}")))?;
        }
        let name = p.name.clone();
        if let Some(slot) = self.state.projects.iter_mut().find(|x| x.id == id) {
            *slot = p.clone();
        }
        self.audit("project.update", &name, "you");
        self.changed()?;
        Ok(p)
    }

    /// `archiveProject`: hides the project, its sub-projects and their chats. Returns the hidden sub-projects.
    pub fn archive_project(&mut self, id: &str) -> Result<usize, ApiError> {
        let p = self.project(id).cloned().ok_or_else(|| ApiError::bad_request("Project not found"))?;
        if p.is_general == Some(true) {
            return Err(ApiError::bad_request("General can’t be archived — quick chats live here."));
        }
        let mut gone = vec![id.to_string()];
        gone.extend(descendants(&self.state.projects, id));
        for x in &mut self.state.projects {
            if gone.iter().any(|g| g == &x.id) {
                x.archived = true;
            }
        }
        for c in &mut self.state.chats {
            if gone.iter().any(|g| g == &c.project) {
                c.archived = true;
            }
        }
        self.audit("project.archive", &p.name, "you");
        self.changed()?;
        Ok(gone.len() - 1)
    }

    /// `restoreProject`: back, with every archived project below it (and their chats).
    pub fn restore_project(&mut self, id: &str) -> Result<(), ApiError> {
        let gone: Vec<String> = std::iter::once(id.to_string())
            .chain(self.state.projects.iter().filter(|x| is_within(&self.state.projects, &x.id, id)).map(|x| x.id.clone()))
            .collect();
        for x in &mut self.state.projects {
            if gone.iter().any(|g| g == &x.id) {
                x.archived = false;
            }
        }
        for c in &mut self.state.chats {
            if gone.iter().any(|g| g == &c.project) {
                c.archived = false;
            }
        }
        self.changed()
    }

    /// `deleteProject`: the whole sub-tree, archived parts too, with chats, routines, graphs and artifacts.
    /// The folders and any other files in them (AGENTS.md included) stay on disk.
    pub fn delete_project(&mut self, id: &str) -> Result<(), ApiError> {
        let p = self.project(id).cloned().ok_or_else(|| ApiError::bad_request("Project not found"))?;
        if p.is_general == Some(true) {
            return Err(ApiError::bad_request("General can’t be deleted"));
        }
        let doomed: Vec<Project> = self.state.projects.iter().filter(|x| is_within(&self.state.projects, &x.id, id)).cloned().collect();
        let ids: Vec<String> = doomed.iter().map(|x| x.id.clone()).collect();
        let in_tree = |pid: &str| ids.iter().any(|i| i == pid);

        self.state.chats.retain(|c| !in_tree(&c.project));
        self.state.routines.retain(|r| !in_tree(&r.project));
        self.state.graphs.retain(|g| !g.get("project").and_then(|v| v.as_str()).is_some_and(in_tree));
        let gone_artifacts: Vec<Artifact> = self.state.artifacts.iter().filter(|a| in_tree(&a.project)).cloned().collect();
        self.state.artifacts.retain(|a| !in_tree(&a.project));

        let survivors: Vec<Project> = self.state.projects.iter().filter(|x| !ids.iter().any(|i| i == &x.id)).cloned().collect();
        for x in &doomed {
            let folder = PathBuf::from(expand_home(&x.folder));
            // Named files, never all of `.harness`: a project at `~` would take the whole harness home with it.
            if survivors.iter().any(|s| PathBuf::from(expand_home(&s.folder)) == folder) {
                continue;
            }
            let _ = std::fs::remove_file(folder.join(".harness/memory/facts.md"));
            let _ = std::fs::remove_file(folder.join(".harness/map.json"));
            for a in gone_artifacts.iter().filter(|a| a.project == x.id) {
                let _ = std::fs::remove_dir_all(folder.join(".harness/artifacts").join(&a.id));
            }
        }
        self.state.projects = survivors;
        if ids.iter().any(|i| i == &self.ws.default_project) {
            self.ws.default_project = self.general().map(|g| g.id.clone()).unwrap_or_default();
        }
        let detail = format!("{} (+{} sub-projects)", p.name, ids.len().saturating_sub(1));
        self.audit("project.delete", &detail, "you");
        self.changed()
    }

    /// `<project folder>/AGENTS.md`.
    pub fn agents_file(&self, pid: &str) -> Result<PathBuf, ApiError> {
        let p = self.project(pid).ok_or_else(|| ApiError::bad_request("Project not found"))?;
        Ok(PathBuf::from(expand_home(&p.folder)).join("AGENTS.md"))
    }

    pub fn read_agents(&self, pid: &str) -> Result<String, ApiError> {
        Ok(read_text(&self.agents_file(pid)?))
    }

    pub fn write_agents(&mut self, pid: &str, text: &str) -> Result<(), ApiError> {
        let file = self.agents_file(pid)?;
        write_text(&file, text)?;
        self.audit("instructions.save", &file.to_string_lossy(), "you");
        self.changed()?;
        self.refresh_instructions();
        Ok(())
    }

    /// Broad to narrow: the workspace instructions, every parent's AGENTS.md (when inheriting), then the
    /// project's own (R-12). Unknown ids give the workspace layer only, as the TS does.
    pub fn instruction_layers(&self, pid: &str) -> Vec<InstructionLayer> {
        let chain = project_path(&self.state.projects, Some(pid));
        let own = chain.last().copied();
        let mut layers = vec![InstructionLayer {
            name: format!("Workspace · {}", self.ws.name),
            file: "workspace settings".into(),
            text: self.ws.instructions.clone(),
            kind: "workspace".into(),
        }];
        if own.is_some_and(|p| p.inherit) {
            for p in chain.iter().take(chain.len().saturating_sub(1)) {
                let file = PathBuf::from(expand_home(&p.folder)).join("AGENTS.md");
                layers.push(InstructionLayer {
                    name: p.name.clone(),
                    file: file.to_string_lossy().into_owned(),
                    text: read_text(&file),
                    kind: "parent".into(),
                });
            }
        }
        if let Some(own) = own {
            let file = PathBuf::from(expand_home(&own.folder)).join("AGENTS.md");
            layers.push(InstructionLayer {
                name: own.name.clone(),
                file: file.to_string_lossy().into_owned(),
                text: read_text(&file),
                kind: "project".into(),
            });
        }
        layers
    }
}

/// `resolve()` for a folder the user typed: relative paths land under the current directory.
fn absolute(path: &str) -> Result<PathBuf, ApiError> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Ok(p.to_path_buf());
    }
    Ok(std::env::current_dir()?.join(p))
}
