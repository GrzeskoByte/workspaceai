//! Artifact store (port of `artifacts.ts`, R-04, R-13, R-14). Files live in the project folder, so moving a project
//! carries its artifacts: `<project>/.harness/artifacts/<id>/manifest.json + v<n>.<ext>`.
//! Every save is a version; restoring makes a new version rather than rewriting history.
//!
//! ponytail: Open Design is not wired yet (B10). Every save takes the built-in path (`od: null`), decks render on the
//! plain slide wrapper, and PDF/PPTX use the built-in print page / are refused. B10 fills `od_sync` and the skeleton.

use crate::formats::{file_ext, format_of, latest_format, looks_html, normalize_type, od_mode, resolve_format, to_html};
use crate::jsonfile::{read_text, write_json, write_text};
use crate::paths::{expand_home, valid_id};
use crate::runtime::Runtime;
use crate::supervisor::ApiError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use workspaceai_shared::types::{Artifact, ArtifactFormat, ArtifactType, ArtifactVersion};
use workspaceai_shared::util::{now_ms, uid};

/// A file answer (exports, shared pages): bytes plus how to serve them.
#[derive(Debug, Clone)]
pub struct FileBody {
    pub filename: String,
    pub mime: String,
    pub body: Vec<u8>,
}

/// What `artifact_create` and the library pass in.
#[derive(Debug, Clone, Default)]
pub struct NewArtifact {
    pub project: Option<String>,
    pub chat: Option<String>,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub note: Option<String>,
    pub format: Option<String>,
}

/// The harness tool type for each artifact kind (the `artifact_guide` type an update must have read).
pub fn tool_type(t: &ArtifactType) -> &'static str {
    match t {
        ArtifactType::Deck => "deck",
        ArtifactType::Prototype | ArtifactType::LiveArtifact | ArtifactType::Image => "prototype",
        ArtifactType::Sheet => "sheet",
        ArtifactType::DesignSystem => "design-system",
        ArtifactType::Doc | ArtifactType::Video => "doc",
    }
}

fn move_dir(from: &Path, to: &Path) -> Result<(), ApiError> {
    if !from.exists() {
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_err() {
        copy_dir(from, to)?;
        std::fs::remove_dir_all(from)?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

impl Runtime {
    pub fn artifact_dir(&self, id: &str, project: &str) -> Result<PathBuf, ApiError> {
        if !valid_id(id) {
            return Err(ApiError::bad_request("Invalid artifact id"));
        }
        let p = self.project(project).ok_or_else(|| ApiError::bad_request("Project not found"))?;
        Ok(PathBuf::from(expand_home(&p.folder)).join(".harness/artifacts").join(id))
    }

    fn write_manifest(&self, a: &Artifact) -> Result<(), ApiError> {
        let m = json!({
            "id": a.id, "title": a.title, "type": a.kind, "od.mode": od_mode(&a.kind), "project": a.project,
            "originSession": a.origin_session, "originChat": a.chat, "designSystem": a.design_system,
            "versions": a.versions, "createdAt": a.created_at, "updatedAt": a.updated_at,
        });
        write_json(&self.artifact_dir(&a.id, &a.project)?.join("manifest.json"), &m)?;
        Ok(())
    }

    pub fn artifact(&self, id: &str) -> Result<&Artifact, ApiError> {
        self.state.artifacts.iter().find(|a| a.id == id).ok_or_else(|| ApiError::not_found("Artifact not found"))
    }

    fn artifact_mut(&mut self, id: &str) -> Result<&mut Artifact, ApiError> {
        self.state.artifacts.iter_mut().find(|a| a.id == id).ok_or_else(|| ApiError::not_found("Artifact not found"))
    }

    /// Artifacts from a session go to that session's project at any depth; from the library a project must be picked.
    pub fn create_artifact(&mut self, o: NewArtifact) -> Result<Artifact, ApiError> {
        let chat = o.chat.as_deref().and_then(|c| self.session(c)).cloned();
        let project = chat.as_ref().map(|c| c.project.clone()).or(o.project).filter(|p| self.project(p).is_some());
        let project = project.ok_or_else(|| ApiError::bad_request("Pick a project for this artifact"))?;
        let kind = normalize_type(&o.kind);
        let now = now_ms();
        let title: String = if o.title.is_empty() { kind.as_str().to_string() } else { o.title.chars().take(120).collect() };
        let format = resolve_format(&kind, &o.content, o.format.as_deref());
        let file = format!("v1.{}", file_ext(&format));
        let mut a = Artifact {
            id: uid("art"),
            project,
            chat: chat.as_ref().map(|c| c.id.clone()),
            origin_session: chat.as_ref().map(|c| c.sid.clone()),
            kind,
            title,
            version: 1,
            design_system: self.ws.design_system.clone(),
            created_at: now,
            updated_at: now,
            ..Default::default()
        };
        let dir = self.artifact_dir(&a.id, &a.project)?;
        write_text(&dir.join(&file), &o.content)?;
        a.versions.push(ArtifactVersion {
            v: 1, at: now, chat: a.chat.clone(), note: o.note.unwrap_or_else(|| "created".into()), file, format: Some(format), od: Some(None), ..Default::default()
        });
        self.state.artifacts.push(a.clone());
        self.write_manifest(&a)?;
        let from = chat.map(|c| format!(" · from {}", c.title)).unwrap_or_default();
        self.audit("artifact.create", &format!("{} · {}{from}", a.kind.as_str(), a.title), "you");
        self.changed()?;
        Ok(a)
    }

    /// A revision from another session keeps the original project and adds that session to the history.
    pub fn revise_artifact(&mut self, id: &str, content: &str, chat: Option<String>, note: Option<String>, title: Option<String>, format: Option<String>) -> Result<Artifact, ApiError> {
        let a = self.artifact(id)?.clone();
        let v = a.version + 1;
        // A revision keeps the format unless it asks for another or is plainly HTML (a Markdown deck rebuilt in HTML).
        let asked = format.unwrap_or_else(|| if looks_html(content) { "html".into() } else { latest_format(&a).as_str().into() });
        let format = resolve_format(&a.kind, content, Some(&asked));
        let file = format!("v{v}.{}", file_ext(&format));
        write_text(&self.artifact_dir(&a.id, &a.project)?.join(&file), content)?;
        let a = {
            let m = self.artifact_mut(id)?;
            m.version = v;
            m.updated_at = now_ms();
            if let Some(t) = title.filter(|t| !t.is_empty()) {
                m.title = t.chars().take(120).collect();
            }
            m.versions.push(ArtifactVersion { v, at: m.updated_at, chat, note: note.unwrap_or_else(|| "revised".into()), file, format: Some(format), od: Some(None), ..Default::default() });
            m.clone()
        };
        self.write_manifest(&a)?;
        self.audit("artifact.version", &format!("{} v{v}", a.title), "you");
        self.changed()?;
        Ok(a)
    }

    /// `{ artifact, version, content, format }` of version `v` (the latest when absent or unknown).
    pub fn read_artifact(&self, id: &str, v: Option<i64>) -> Result<(Artifact, i64, String, ArtifactFormat), ApiError> {
        let a = self.artifact(id)?;
        let want = v.unwrap_or(a.version);
        let ver = a.versions.iter().find(|x| x.v == want).or(a.versions.last()).ok_or_else(|| ApiError::not_found("Artifact has no versions"))?;
        let content = read_text(&self.artifact_dir(&a.id, &a.project)?.join(&ver.file));
        Ok((a.clone(), ver.v, content, format_of(ver)))
    }

    /// A version as the panel shows it: its source plus the page.
    pub fn view_artifact(&self, id: &str, v: Option<i64>) -> Result<Value, ApiError> {
        let (a, version, content, format) = self.read_artifact(id, v)?;
        let html = to_html(&a, &content, false, &format, None);
        Ok(json!({ "artifact": a, "version": version, "content": content, "format": format, "html": html }))
    }

    pub fn restore_artifact(&mut self, id: &str, v: i64) -> Result<Artifact, ApiError> {
        let (_, _, content, _) = self.read_artifact(id, Some(v))?;
        self.revise_artifact(id, &content, None, Some(format!("restored v{v}")), None, None)
    }

    /// Moving the folder keeps provenance: the origin-session link stays.
    pub fn move_artifact(&mut self, id: &str, project: &str) -> Result<Artifact, ApiError> {
        let a = self.artifact(id)?.clone();
        if self.project(project).is_none() {
            return Err(ApiError::bad_request("Project not found"));
        }
        if a.project == project {
            return Ok(a);
        }
        move_dir(&self.artifact_dir(&a.id, &a.project)?, &self.artifact_dir(&a.id, project)?)?;
        let a = {
            let m = self.artifact_mut(id)?;
            m.project = project.to_string();
            m.clone()
        };
        self.write_manifest(&a)?;
        let name = self.project(project).map(|p| p.name.clone()).unwrap_or_default();
        self.audit("artifact.move", &format!("{} → {name}", a.title), "you");
        self.changed()?;
        Ok(a)
    }

    /// Moves the files of artifacts whose rows already point at a new project (after a session move).
    pub fn relocate_artifacts(&self, ids: &[String], from_project: &str) -> Result<(), ApiError> {
        for id in ids {
            let a = self.artifact(id)?;
            let (from, to) = (self.artifact_dir(id, from_project)?, self.artifact_dir(id, &a.project)?);
            if from != to && from.exists() {
                move_dir(&from, &to)?;
                self.write_manifest(a)?;
            }
        }
        Ok(())
    }

    pub fn remove_artifact(&mut self, id: &str) -> Result<(), ApiError> {
        let a = self.artifact(id)?.clone();
        let dir = self.artifact_dir(&a.id, &a.project)?;
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        self.state.artifacts.retain(|x| x.id != id);
        self.audit("artifact.delete", &a.title, "you");
        self.changed()
    }

    /// Built-in export. PPTX needs Open Design (B10); PDF is the HTML page that prints itself.
    pub fn export_artifact(&self, id: &str, format: &str, v: Option<i64>) -> Result<FileBody, ApiError> {
        let (a, version, content, src) = self.read_artifact(id, v)?;
        let base: String = a.title.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ').collect();
        let base = base.split_whitespace().collect::<Vec<_>>().join("-").to_lowercase();
        let base = if base.is_empty() { "artifact".to_string() } else { base };
        let name = |ext: &str| format!("{base}-v{version}.{ext}");
        let wanted = if format == "md" { "markdown" } else { format };
        if wanted == src.as_str() && src != ArtifactFormat::Html {
            let mime = match format {
                "md" => "text/markdown",
                "csv" => "text/csv",
                "svg" => "image/svg+xml",
                "json" => "application/json",
                _ => "text/plain",
            };
            return Ok(FileBody { filename: name(format), mime: mime.into(), body: content.into_bytes() });
        }
        match format {
            "html" | "pdf" => Ok(FileBody { filename: name("html"), mime: "text/html".into(), body: to_html(&a, &content, format == "pdf", &src, None).into_bytes() }),
            "pptx" => Err(ApiError::bad_request("PowerPoint export needs Open Design")),
            _ => Err(ApiError::bad_request(format!("Export format not available for {}", a.kind.as_str()))),
        }
    }

    /// Sharing publishes a static export at an unguessable link; org policy can forbid it. `base` is the loopback URL.
    pub fn share_artifact(&self, id: &str, base: Option<&str>) -> Result<Value, ApiError> {
        if self.ws.kind == workspaceai_shared::types::WorkspaceKind::Org && self.policy.as_ref().is_some_and(|p| p.block_external_sharing) {
            self.audit("artifact.share", "org-only link", "you");
            return Ok(json!({ "scope": "organization", "note": "Shared with your organization (public links are off)" }));
        }
        if !self.ws.public_links {
            return Ok(json!({ "scope": "private", "note": "Public links are off for this workspace — turn them on in Settings › Artifacts & sharing" }));
        }
        let base = base.ok_or_else(|| ApiError::new(503, "Sharing needs the local server, which is not running"))?;
        let out = self.export_artifact(id, "html", None)?;
        let key = crate::server::random_token(18);
        write_bytes(&self.paths.data.join("shares").join(format!("{key}.html")), &out.body)?;
        self.audit("artifact.share", &format!("{} · static link", out.filename), "you");
        Ok(json!({ "scope": "link", "url": format!("{base}/api/shared/{}/{key}", self.id), "note": "Link copied — a static export, never the live daemon" }))
    }

    /// `GET /api/shared/:wsid/:key`: the stored page, or None when the link was removed.
    pub fn shared_page(&self, key: &str) -> Option<Vec<u8>> {
        let key: String = key.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
        if key.is_empty() {
            return None;
        }
        std::fs::read(self.paths.data.join("shares").join(format!("{key}.html"))).ok()
    }

    /// The session an agent tool call comes from: the one it names (claude, grok carry `HARNESS_CHAT`), else the
    /// most recently launched opencode session (opencode shares one MCP config per workspace; plan (c)).
    pub fn calling_session(&self, named: Option<&str>) -> Option<String> {
        if let Some(c) = named.filter(|c| self.session(c).is_some()) {
            return Some(c.to_string());
        }
        self.last_opencode_launch.clone().filter(|c| self.session(c).is_some())
    }

    pub fn mark_guided(&mut self, chat: Option<&str>, kind: &str) {
        if let Some(c) = chat {
            self.guided.insert(format!("{c}:{kind}"));
        }
    }

    pub fn is_guided(&self, chat: Option<&str>, kind: &str) -> bool {
        chat.is_none() || kind == "sheet" || chat.is_some_and(|c| self.guided.contains(&format!("{c}:{kind}")))
    }
}

fn write_bytes(file: &Path, body: &[u8]) -> std::io::Result<()> {
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(file, body)
}

/// Port of the `FORMAT` table in `design-guide.ts`.
fn format_help(kind: &str) -> &'static str {
    match kind {
        "deck" => "Pass `format`: \"html\" (preferred) for a deck built on Open Design’s deck framework below — copy its skeleton, bind the design system’s tokens in :root, fill one <section class=\"slide\"> per slide; or \"markdown\" for quick slides separated by a line with only `---`, which WorkspaceAI lays onto the same framework.",
        "prototype" => "Pass `format: \"html\"`: one complete self-contained HTML file — inline CSS (the design system’s tokens as CSS custom properties on :root) and inline JS, no external network requests or CDN fonts.",
        "sheet" => "CSV with a header row (`format: \"csv\"`), or `format: \"html\"` for a designed table page.",
        "design-system" => "Pass `format: \"markdown\"`: a DESIGN.md with one H1 (the system’s name), a one-line summary under it, then principles, colour tokens (a table with 6-digit hex values, light and dark), type, spacing and component rules. Every value must be concrete enough to build from.",
        _ => "Pass `format`: \"markdown\" for a plain document (one H1, sections, lists, tables), or \"html\" for a designed page (one complete self-contained HTML file built with the design system and, if one fits, an Open Design skill below). Open Design stores the page either way.",
    }
}

/// `artifactGuide` without Open Design: the header and format rules. ponytail: B10 adds the design system,
/// Open Design skills, craft rules and the deck framework.
pub fn artifact_guide(kind: &str) -> String {
    let rule = if kind == "design-system" {
        "You are writing a new design system, not following one."
    } else {
        "The design system is authoritative: use its palette, type scale, spacing and component rules exactly; do not invent other colours or fonts."
    };
    format!(
        "# Open Design reference for this {kind}\n\nFollow it for the artifact you save with artifact_create / artifact_update. {rule} Pass the complete document as `content` — never a summary or a file path.\n\n## Format\n{}",
        format_help(kind)
    )
}
