//! `GET /api/ws/:id/export`: one `.tar.gz` of the workspace, built in memory with `tar` + `flate2`.
//! Layout matches `routes.ts`: `<slug>/config`, `<slug>/memory`, `<slug>/data/{state.json,audit.jsonl}`,
//! `<slug>/data/opencode/xdg-data`, and `<slug>/projects/<pid>/{AGENTS.md,.harness}`.
//!
//! Hardening over the TS (which shells out to `tar -h`): files that hold credentials are left out and any
//! file containing a keychain value, a private key or a known credential file name is skipped entirely.

use crate::paths::expand_home;
use crate::runtime::Runtime;
use crate::supervisor::ApiError;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

/// A built archive, ready to be written as the response body.
#[derive(Debug)]
pub struct Archive {
    pub content_type: &'static str,
    pub filename: String,
    pub bytes: Vec<u8>,
}

/// File names that hold credentials rather than workspace content (opencode `/auth.json`, org issuer keys).
const CREDENTIAL_FILES: [&str; 5] = ["auth.json", "issuer.json", "credentials.json", "keychain.json", "secrets.json"];

/// Build the archive for `rt`. `secrets` are every keychain value of the workspace: never exported.
pub fn build(rt: &Runtime, secrets: &[String]) -> Result<Archive, ApiError> {
    rt.audit("workspace.export", &format!("{}.tar.gz", rt.ws.slug), "you");
    let root = rt.ws.slug.clone();
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    let mut seen_dirs: HashSet<PathBuf> = HashSet::new();

    if rt.paths.config.exists() {
        add_tree(&rt.paths.config, "config", &mut entries, &mut seen_dirs);
    }
    if rt.paths.memory.exists() {
        add_tree(&rt.paths.memory, "memory", &mut entries, &mut seen_dirs);
    }
    if rt.paths.state.exists() {
        entries.push(("data/state.json".into(), rt.paths.state.clone()));
    }
    if rt.paths.audit.exists() {
        entries.push(("data/audit.jsonl".into(), rt.paths.audit.clone()));
    }
    if rt.paths.xdg_data.exists() {
        add_tree(&rt.paths.xdg_data, "data/opencode/xdg-data", &mut entries, &mut seen_dirs);
    }
    for p in &rt.state.projects {
        let dir = PathBuf::from(expand_home(&p.folder));
        for item in ["AGENTS.md", ".harness"] {
            let path = dir.join(item);
            if !path.exists() {
                continue;
            }
            let prefix = format!("projects/{}/{item}", p.id);
            if path.is_dir() {
                add_tree(&path, &prefix, &mut entries, &mut seen_dirs);
            } else {
                entries.push((prefix, path));
            }
        }
    }

    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut tar = tar::Builder::new(&mut gz);
        for (rel, source) in entries {
            if should_skip(&source, secrets) {
                continue;
            }
            let name = format!("{root}/{rel}");
            tar.append_path_with_name(&source, &name).map_err(ApiError::from)?;
        }
        tar.finish().map_err(ApiError::from)?;
    }
    let bytes = gz.finish().map_err(ApiError::from)?;
    Ok(Archive { content_type: "application/gzip", filename: format!("{root}.tar.gz"), bytes })
}

/// Recursively collect files under `dir`; symlinked folders are followed once (tar `-h`), with a cycle guard.
fn add_tree(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>, seen: &mut HashSet<PathBuf>) {
    let Ok(canonical) = std::fs::canonicalize(dir) else { return };
    if !seen.insert(canonical) {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut children: Vec<_> = read.flatten().collect();
    children.sort_by_key(|e| e.file_name());
    for entry in children {
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = format!("{prefix}/{name}");
        let path = entry.path();
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.is_dir() {
            add_tree(&path, &rel, out, seen);
        } else if meta.is_file() {
            out.push((rel, path));
        }
    }
}

/// A file goes in unless it is a credential file, a private key, or contains a keychain value.
fn should_skip(source: &Path, secrets: &[String]) -> bool {
    let name = source.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if CREDENTIAL_FILES.contains(&name) {
        return true;
    }
    let Ok(bytes) = std::fs::read(source) else { return true };
    let text = String::from_utf8_lossy(&bytes);
    if text.contains("-----BEGIN PRIVATE KEY-----") {
        return true;
    }
    secrets.iter().any(|s| s.len() >= 8 && text.contains(s.as_str()))
}

/// Write the archive to a file (tests and the loopback server can use it).
pub fn write_to(archive: &Archive, file: &Path) -> std::io::Result<()> {
    let mut f = std::fs::File::create(file)?;
    f.write_all(&archive.bytes)
}
