//! Creating, joining, leaving and exporting workspaces, and the choices on Settings › General. Wording and request
//! shapes follow `NewWorkspace.tsx` and the General page of the web app.

use crate::types::WorkspaceSummary;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// (id, label, note) of the three kinds in the New workspace dialog.
pub const TYPES: [(&str, &str, &str); 3] = [("team", "Team", "shared space you manage"), ("personal", "Personal", "another private space"), ("org", "Organization", "join with SSO")];

pub const NEW_NOTE: &str = "Gets its own opencode server, Open Design daemon, memory, secrets and a General project. Nothing is copied from other workspaces.";
pub const ORG_NOTE: &str = "Sign in with your organization’s identity provider. Its signed policy decides the models, connectors, sharing and retention here.";

pub fn submit_label(kind: &str, busy: bool) -> &'static str {
    match (busy, kind) {
        (true, _) => "Working…",
        (_, "org") => "Continue with SSO",
        _ => "Create workspace",
    }
}

/// `(path, body)` for the dialog, or `None` when the name is empty ("Name the workspace").
pub fn create_request(kind: &str, name: &str, email: &str, issuer: &str) -> Option<(&'static str, Value)> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some(if kind == "org" {
        ("/api/workspaces/join-org", json!({ "name": name, "email": email.trim(), "issuer": issuer.trim() }))
    } else {
        ("/api/workspaces", json!({ "name": name, "kind": if kind == "personal" { "personal" } else { "team" } }))
    })
}

pub fn created_toast(kind: &str, name: &str) -> String {
    if kind == "org" {
        format!("Joined {name} · policy verified")
    } else {
        format!("Created {name} · its own engine starts with the first chat")
    }
}

// ---- delete / leave ----

/// The only personal workspace cannot be removed.
pub fn can_delete(kind: &str, workspaces: &[WorkspaceSummary]) -> bool {
    !(kind == "personal" && workspaces.iter().filter(|w| w.kind == "personal").count() <= 1)
}

pub fn remove_label(kind: &str) -> &'static str {
    if kind == "org" { "Leave organization" } else { "Delete workspace" }
}

pub fn remove_note(kind: &str, allowed: bool) -> &'static str {
    match (allowed, kind) {
        (false, _) => "Your only personal workspace can’t be deleted",
        (_, "org") => "Your org data stays with the org",
        _ => "Deletes its chats, memory and artifacts index",
    }
}

/// Button text; the second press (armed) says what happens.
pub fn remove_button(kind: &str, armed: bool) -> &'static str {
    match (armed, kind) {
        (true, "org") => "Click again to leave",
        (true, _) => "Click again: removes its chats, memory and secrets",
        (_, "org") => "Leave…",
        _ => "Delete…",
    }
}

pub fn remove_path(workspace: &str) -> String {
    format!("/api/workspaces/{}", crate::api::seg(workspace))
}

pub fn removed_toast(name: &str) -> String {
    format!("{name} removed")
}

/// Where to go after `removed` is gone: the first other workspace.
pub fn next_workspace(workspaces: &[WorkspaceSummary], removed: &str) -> Option<String> {
    workspaces.iter().find(|w| w.id != removed).map(|w| w.id.clone())
}

// ---- export ----

pub fn export_path(workspace: &str) -> String {
    format!("/api/ws/{}/export", crate::api::seg(workspace))
}

/// `filename="x.tar.gz"` out of a Content-Disposition header.
pub fn filename_from_disposition(header: &str) -> Option<String> {
    let rest = header.split("filename=\"").nth(1)?;
    rest.split('"').next().filter(|s| !s.is_empty()).map(str::to_string)
}

/// A server-given name as one file name: no directories, no control characters; "export" when nothing is left.
pub fn safe_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim().trim_start_matches('.');
    if clean.is_empty() { "export".into() } else { clean.to_string() }
}

/// `dir/name`, or `dir/stem (2).ext` and so on so a download never overwrites a file.
pub fn unique_path(dir: &Path, name: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let name = safe_filename(name);
    let first = dir.join(&name);
    if !exists(&first) {
        return first;
    }
    let (stem, ext) = match name.find('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !exists(p))
        .unwrap_or(first)
}

/// Downloads go to `$HOME/Downloads` when it exists, else the home directory.
pub fn download_dir(home: &Path) -> PathBuf {
    let d = home.join("Downloads");
    if d.is_dir() { d } else { home.to_path_buf() }
}

// ---- default project / model ----

/// Model ids to offer: the current one first (even when no longer listed), then the rest, without repeats.
pub fn model_choices(current: &str, listed: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in std::iter::once(current).chain(listed.iter().map(String::as_str)) {
        if !m.is_empty() && !out.iter().any(|x| x == m) {
            out.push(m.to_string());
        }
    }
    out
}

pub fn model_toast(model: &str) -> String {
    format!("Model · {model}")
}
