//! Port of `detect.ts` (pure parts): where opencode is, which pinned release a workspace runs, install path.
//! Session import (`LocalOpencodeData`, `copySessions`) belongs to batch 5.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use workspaceai_shared::types::EngineSettings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: String,
    pub version: String,
}

fn sanitized(binary: &str) -> String {
    binary.chars().filter(|c| c.is_ascii_alphanumeric() || "._/-".contains(*c)).collect()
}

/// `command -v <binary>` then `<path> --version`, as the TS does.
pub fn find_opencode(binary: &str) -> Option<Found> {
    let out = Command::new("sh").arg("-c").arg(format!("command -v {}", sanitized(binary))).stdin(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    let version = Command::new(&path).arg("--version").stdin(Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&version.stdout).trim().to_string();
    let text = text.strip_prefix("opencode ").map(str::to_string).unwrap_or(text);
    let text = text.strip_prefix("opencode").map(str::to_string).unwrap_or(text);
    Some(Found { path, version: text.trim().to_string() })
}

pub fn pinned_opencode_dir(home: &Path, version: &str) -> PathBuf {
    let safe: String = version.chars().filter(|c| c.is_ascii_alphanumeric() || ".-".contains(*c)).collect();
    home.join("vendor").join("opencode").join(safe)
}

pub fn pinned_opencode_bin(home: &Path, version: &str) -> PathBuf {
    pinned_opencode_dir(home, version).join("node_modules").join(".bin").join("opencode")
}

/// The unmodified binary a workspace runs: its pinned release, or the one on PATH (or a custom path).
pub fn opencode_binary(home: &Path, engine: &EngineSettings) -> Result<String, String> {
    let version = engine.version.trim();
    if !version.is_empty() && version != "system" {
        let bin = pinned_opencode_bin(home, version);
        if !bin.exists() {
            return Err(format!("opencode {version} is pinned for this workspace but not installed — Settings › Engine › Install"));
        }
        return Ok(bin.to_string_lossy().into_owned());
    }
    if engine.binary.trim().is_empty() {
        Ok("opencode".into())
    } else {
        Ok(engine.binary.trim().to_string())
    }
}

/// `^\d+\.\d+\.\d+([-.][0-9A-Za-z.]+)?$` without pulling in regex.
fn exact_version(v: &str) -> bool {
    let split = v.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(v.len());
    let (core, suffix) = v.split_at(split);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return false;
    }
    if suffix.is_empty() {
        return true;
    }
    let tail = &suffix[1..];
    (suffix.starts_with('-') || suffix.starts_with('.'))
        && !tail.is_empty()
        && tail.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
}

/// Installs the official `opencode-ai` npm release as is; nothing is patched.
/// Untested here (the tests never run npm) — see PORT-STATUS, batch 12.
pub fn install_opencode(home: &Path, version: &str) -> Result<PathBuf, String> {
    if !exact_version(version) {
        return Err("Use an exact opencode version such as 2.2.1".into());
    }
    let dir = pinned_opencode_dir(home, version);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut child = Command::new("npm")
        .args(["install", "--prefix", &dir.to_string_lossy(), "--no-audit", "--no-fund", &format!("opencode-ai@{version}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("npm did not start: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(10 * 60);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let bin = pinned_opencode_bin(home, version);
                return if bin.exists() { Ok(bin) } else { Err("npm finished but no opencode binary was installed".into()) };
            }
            Ok(Some(status)) => return Err(format!("npm install failed ({status})")),
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let _ = child.kill();
    Err("npm install timed out".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_paths_strip_unsafe_characters() {
        let home = Path::new("/tmp/h");
        assert_eq!(pinned_opencode_dir(home, "2.2.1"), home.join("vendor/opencode/2.2.1"));
        assert_eq!(pinned_opencode_dir(home, "../../etc"), home.join("vendor/opencode/....etc"));
        assert!(pinned_opencode_bin(home, "2.2.1").ends_with("node_modules/.bin/opencode"));
    }

    #[test]
    fn exact_versions() {
        assert!(exact_version("2.2.1"));
        assert!(exact_version("2.2.1-beta.3"));
        assert!(!exact_version("latest"));
        assert!(!exact_version("2.2"));
        assert!(!exact_version("2.2.1; rm -rf /"));
    }
}
