//! Port of `packages/shared/src/util.ts`.

use crate::types::{Artifact, Project, Session};
use std::collections::{hash_map::RandomState, HashSet};
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn base36(mut n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// `prefix_<time><counter><random>` in base 36, unique per call.
pub fn uid(prefix: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = (SEQ.fetch_add(1, Ordering::Relaxed) + 1) % 1_000_000;
    let mut h = RandomState::new().build_hasher();
    h.write_u64(now_ms() as u64 ^ seq);
    let mut rnd = base36(h.finish() % 36u64.pow(4));
    while rnd.len() < 4 {
        rnd.insert(0, '0');
    }
    format!("{prefix}_{}{}{rnd}", base36(now_ms() as u64), base36(seq))
}

/// Whether a chat keeps following new text after a scroll: any move up lets go, and coming back near the bottom follows again.
pub fn follows_bottom(following: bool, last_top: f64, top: f64, gap_to_bottom: f64) -> bool {
    if top < last_top {
        return false;
    }
    following || gap_to_bottom < 80.0
}

pub fn slugify(name: &str, fallback: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let t = out.trim_matches('-');
    if t.is_empty() { fallback.to_string() } else { t.to_string() }
}

pub fn initials_of(name: &str) -> String {
    let parts: Vec<&str> = name.split_whitespace().collect();
    let first = |p: &str| p.chars().next().map(|c| c.to_string()).unwrap_or_default();
    match parts.len() {
        0 => "?".into(),
        1 => first(parts[0]).to_uppercase(),
        _ => parts.iter().map(|p| first(p)).collect::<String>().chars().take(2).collect::<String>().to_uppercase(),
    }
}

/// Root-first chain of projects from the top level down to `pid`.
pub fn project_path<'a>(projects: &'a [Project], pid: Option<&str>) -> Vec<&'a Project> {
    let mut out = Vec::new();
    let mut p = pid.and_then(|id| projects.iter().find(|x| x.id == id));
    let mut guard = 0;
    while let Some(cur) = p {
        if guard >= 64 {
            break;
        }
        guard += 1;
        out.insert(0, cur);
        p = cur.parent.as_deref().and_then(|par| projects.iter().find(|x| x.id == par));
    }
    out
}

pub fn project_path_name(projects: &[Project], pid: Option<&str>) -> String {
    project_path(projects, pid).iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(" › ")
}

pub fn child_projects<'a>(projects: &'a [Project], pid: Option<&str>) -> Vec<&'a Project> {
    projects.iter().filter(|p| p.parent.as_deref() == pid && !p.archived).collect()
}

pub fn descendants(projects: &[Project], pid: &str) -> Vec<String> {
    child_projects(projects, Some(pid)).into_iter().flat_map(|p| std::iter::once(p.id.clone()).chain(descendants(projects, &p.id))).collect()
}

/// True when `pid` is `ancestor` or nested anywhere beneath it.
pub fn is_within(projects: &[Project], pid: &str, ancestor: &str) -> bool {
    project_path(projects, Some(pid)).iter().any(|p| p.id == ancestor)
}

/// Depth-first flat list, used by pickers.
pub fn flat_projects(projects: &[Project]) -> Vec<(&Project, usize)> {
    fn walk<'a>(projects: &'a [Project], pid: Option<&str>, depth: usize, out: &mut Vec<(&'a Project, usize)>) {
        let mut kids = child_projects(projects, pid);
        kids.sort_by_key(|p| (!p.is_general.unwrap_or(false), p.created_at));
        for p in kids {
            out.push((p, depth));
            walk(projects, Some(&p.id), depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(projects, None, 0, &mut out);
    out
}

/// Artifacts that roll up into a project's Artifacts tab: its own plus every sub-project's (R-13).
pub fn rolled_up_artifacts<'a>(projects: &[Project], artifacts: &'a [Artifact], pid: &str) -> Vec<&'a Artifact> {
    artifacts.iter().filter(|a| is_within(projects, &a.project, pid)).collect()
}

pub fn chats_within<'a>(projects: &[Project], chats: &'a [Session], pid: &str) -> Vec<&'a Session> {
    chats.iter().filter(|c| !c.archived && is_within(projects, &c.project, pid)).collect()
}

/// Folder → parent folder mapping used when imported session folders become nested projects (R-18).
pub fn nest_folders(dirs: &[&str]) -> Vec<(String, Option<String>)> {
    let mut seen = HashSet::new();
    let mut sorted: Vec<&str> = dirs.iter().copied().filter(|d| seen.insert(*d)).collect();
    sorted.sort_by_key(|d| d.encode_utf16().count());
    sorted
        .iter()
        .map(|d| {
            let parent = sorted
                .iter()
                .filter(|o| *o != d && d.starts_with(&format!("{}/", o.strip_suffix('/').unwrap_or(o))))
                .rev() // max_by_key keeps the last of equals; JS keeps the first of the longest
                .max_by_key(|o| o.encode_utf16().count())
                .map(|o| o.to_string());
            (d.to_string(), parent)
        })
        .collect()
}

pub fn title_case_segment(dir: &str) -> String {
    let trimmed = dir.strip_suffix('/').unwrap_or(dir);
    let seg = match trimmed.rsplit('/').next() {
        Some(s) if !s.is_empty() => s,
        _ => dir,
    };
    let mut c = seg.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn time_ago(ms: i64, now: i64) -> String {
    let round = |x: f64| x.round() as i64; // JS Math.round: halves go up, same for positives
    let s = round(((now - ms) as f64 / 1000.0).max(0.0));
    if s < 60 {
        return "just now".into();
    }
    let m = round(s as f64 / 60.0);
    if m < 60 {
        return format!("{m} min ago");
    }
    let h = round(m as f64 / 60.0);
    if h < 24 {
        return format!("{h} h ago");
    }
    let d = round(h as f64 / 24.0);
    if d == 1 {
        return "yesterday".into();
    }
    if d < 7 {
        return format!("{d} days ago");
    }
    if d < 14 {
        return "last week".into();
    }
    if d < 31 {
        return format!("{} weeks ago", round(d as f64 / 7.0));
    }
    if d < 62 { "last month".into() } else { format!("{} months ago", round(d as f64 / 30.0)) }
}

/// Fuzzy score used by the palette and tree search: 3 prefix, 2 substring, 1 subsequence, 0 none.
pub fn fuzzy_score(query: &str, label: &str, extra: &str) -> u8 {
    let q = query.to_lowercase();
    let q = q.trim();
    if q.is_empty() {
        return 1;
    }
    let l = label.to_lowercase();
    if l.starts_with(q) {
        return 3;
    }
    let hay = format!("{label} {extra}").to_lowercase();
    if hay.contains(q) {
        return 2;
    }
    let mut want = q.chars().peekable();
    for ch in hay.chars() {
        if want.peek() == Some(&ch) {
            want.next();
        }
        if want.peek().is_none() {
            return 1;
        }
    }
    0
}
