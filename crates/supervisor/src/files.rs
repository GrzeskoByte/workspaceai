//! `@` file references (port of `files.ts`): tracked and untracked files from `git ls-files`, or a bounded
//! folder walk when the folder is not in a git repository. Paths only, relative to the project.

use std::path::Path;
use std::process::Command;
use workspaceai_shared::util::fuzzy_score;

/// Dependency, build and harness folders that are never offered (`SKIP` in `graphify.ts`).
pub const SKIP: [&str; 14] = [
    ".git", "node_modules", ".next", "dist", "build", "out", ".cache", ".turbo", "target", "vendor", ".venv", "venv", "__pycache__", ".harness",
];

const WALK_BUDGET: usize = 20_000;

/// `git -C <folder> ls-files -co --exclude-standard -z`; `None` when git is missing or the folder is not a repository.
fn git_files(folder: &Path) -> Option<Vec<String>> {
    let out = Command::new("git").arg("-C").arg(folder).args(["ls-files", "-co", "--exclude-standard", "-z"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect())
}

/// At most 20 000 entries, skipping the same folders git would ignore only when they are obvious.
fn walk_files(folder: &Path) -> Vec<String> {
    fn walk(base: &Path, rel: &str, out: &mut Vec<String>, budget: &mut usize) {
        let Ok(entries) = std::fs::read_dir(base.join(rel)) else { return };
        for entry in entries.flatten() {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let name = entry.file_name().to_string_lossy().to_string();
            let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if !SKIP.contains(&name.as_str()) {
                    walk(base, &path, out, budget);
                }
            } else if kind.is_file() {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    let mut budget = WALK_BUDGET;
    walk(folder, "", &mut out, &mut budget);
    out
}

/// Best matches for `query`, a folder being a path ending in `/`. Names beat paths, then shorter paths win.
pub fn search_files(folder: &str, query: &str, limit: usize) -> Vec<String> {
    let base = Path::new(folder);
    let files: Vec<String> = git_files(base).unwrap_or_else(|| walk_files(base))
        .into_iter()
        .filter(|f| !f.split('/').any(|seg| SKIP.contains(&seg)))
        .collect();
    let mut dirs: Vec<String> = Vec::new();
    for f in &files {
        for (i, _) in f.match_indices('/') {
            let d = format!("{}/", &f[..i]);
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
    }
    let name = |p: &str| p.trim_end_matches('/').rsplit('/').next().unwrap_or(p).to_string();
    let mut all: Vec<(u8, String)> = dirs
        .into_iter()
        .chain(files)
        .filter(|p| p != query)
        .map(|p| (fuzzy_score(query, &name(&p), &p), p))
        .filter(|(score, _)| *score > 0)
        .collect();
    all.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())).then(a.1.cmp(&b.1)));
    all.into_iter().take(limit).map(|(_, p)| p).collect()
}
