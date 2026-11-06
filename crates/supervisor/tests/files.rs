//! Port of `files.test.ts`: `@` references list tracked and untracked project files, never dependency or
//! harness folders. The git cases skip with a printed reason when git is not on PATH.

mod common;

use std::path::Path;
use std::process::Command;
use workspaceai_supervisor::files::search_files;

fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for d in ["src/views", "node_modules/x", ".harness/memory"] {
        std::fs::create_dir_all(dir.path().join(d)).unwrap();
    }
    for f in ["main.ts", "src/main.ts", "src/views/Chat.tsx", "my notes.md", "node_modules/x/main.ts", ".harness/memory/facts.md", "ignored.log"] {
        std::fs::write(dir.path().join(f), "").unwrap();
    }
    dir
}

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success())
}

fn git_init(dir: &Path) {
    let out = Command::new("git").current_dir(dir).args(["init", "-q"]).output().unwrap();
    assert!(out.status.success(), "git init failed: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn offers_files_and_folders_best_match_first_and_skips_dependencies() {
    let dir = tree();
    let d = dir.path().to_string_lossy().to_string();
    let main = search_files(&d, "main", 30);
    assert_eq!(&main[..2], ["main.ts", "src/main.ts"]);
    assert!(search_files(&d, "view", 30).contains(&"src/views/".to_string()));
    let all = search_files(&d, "", 100);
    assert!(all.contains(&"my notes.md".to_string()));
    assert!(!all.iter().any(|p| p.starts_with("node_modules") || p.starts_with(".harness")));
    assert!(!all.iter().any(|p| p.contains("/node_modules/") || p.contains("/.harness/")));
}

#[test]
fn completes_inside_a_folder_already_typed_in_full_and_caps_the_results() {
    let dir = tree();
    let d = dir.path().to_string_lossy().to_string();
    let inside = search_files(&d, "src/", 30);
    assert!(!inside.contains(&"src/".to_string()));
    assert!(inside.contains(&"src/main.ts".to_string()));
    assert_eq!(search_files(&d, "", 2).len(), 2);
}

#[test]
fn a_folder_that_does_not_exist_returns_nothing() {
    assert!(search_files("/tmp/wsai-no-such-folder-4b1", "", 30).is_empty());
}

#[test]
fn honours_gitignore_in_a_repository() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let dir = tree();
    std::fs::write(dir.path().join(".gitignore"), "ignored.log\nnode_modules\n").unwrap();
    git_init(dir.path());
    let d = dir.path().to_string_lossy().to_string();
    assert!(search_files(&d, "ignored", 30).is_empty());
    let all = search_files(&d, "", 100);
    assert!(all.contains(&"my notes.md".to_string()));
    assert!(!all.iter().any(|p| p.starts_with("node_modules") || p.starts_with(".harness")));
    assert_eq!(
        search_files(&d, "main", 30).into_iter().take(2).collect::<Vec<_>>(),
        ["main.ts", "src/main.ts"]
    );
}
