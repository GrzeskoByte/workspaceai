//! Port of `jsonfile.ts`: JSON/text files written atomically (temp file, then rename).

use serde::{de::DeserializeOwned, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// `None` when the file is missing or does not parse (the TS `fallback`).
pub fn read_json<T: DeserializeOwned>(file: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(file).ok()?).ok()
}

fn tmp_for(file: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut s = file.as_os_str().to_owned();
    s.push(format!(".{}.{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    PathBuf::from(s)
}

fn write_atomic(file: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = tmp_for(file);
    let res = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).mode(mode).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, file)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// Two-space indented like `JSON.stringify(v, null, 2)`, newline at the end, mode 0600.
pub fn write_json<T: Serialize + ?Sized>(file: &Path, value: &T) -> io::Result<()> {
    let mut text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    text.push('\n');
    write_atomic(file, text.as_bytes(), 0o600)
}

pub fn write_text(file: &Path, text: &str) -> io::Result<()> {
    write_atomic(file, text.as_bytes(), 0o644)
}

pub fn read_text(file: &Path) -> String {
    fs::read_to_string(file).unwrap_or_default()
}

pub fn append_line(file: &Path, line: &str) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = OpenOptions::new().append(true).create(true).mode(0o600).open(file)?;
    f.write_all(format!("{line}\n").as_bytes())
}
