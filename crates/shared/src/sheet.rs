//! Reads an agent's own `/usage` and `/context` text into meters and breakdowns. Port of `readSheet` from
//! `apps/ui/src/views/ActionSheet.tsx`; whatever is not recognised stays in `rest`.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq)]
pub struct Meter {
    pub label: String,
    pub pct: f64,
    pub sub: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankedRow {
    /// -1 when the line has no percentage.
    pub pct: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub heading: String,
    pub rows: Vec<RankedRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Category {
    pub name: String,
    pub value: String,
    pub pct: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sheet {
    pub meters: Vec<Meter>,
    pub ranked: Vec<Ranked>,
    pub categories: Vec<Category>,
    pub model: Option<String>,
    pub rest: String,
    pub intro: Vec<String>,
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}
re!(USED, r"(?i)^\s*([^:\n|]{2,60}):\s*(\d+(?:\.\d+)?)%\s*used(?:\s*[·•-]\s*(.+))?\s*$");
re!(TOKENS, r"(?i)^\s*\**Tokens:?\**\s*:?\s*([\d.,]+\s*[kmb]?)\s*/\s*([\d.,]+\s*[kmb]?)\s*\((\d+(?:\.\d+)?)%\)");
re!(MODEL, r"(?i)^\s*\**Model:?\**\s*:?\s*(.+?)\s*$");
re!(RANK_HEAD, r"^\S.*·");
re!(RANK_ROW1, r"^\s{2,}\d+%\s{2,}");
re!(INDENTED, r"^\s{2,}");
re!(RANK_ROW, r"^\s{2,}(\d+)%\s{2,}(.+)$");
re!(CAT_HEAD, r"(?i)^\|\s*Category\s*\|");
re!(CAT_SEP, r"^\|[\s:-]+\|");
re!(PIPE, r"^\|");
re!(HEADING_OR_PIPE, r"^#|\|");
re!(CONTEXT_TITLE, r"(?im)^##\s*Context Usage\s*$");
re!(CATEGORY_TITLE, r"(?im)^###\s*Estimated usage by category\s*$");
re!(BLANKS, r"\n{3,}");

/// Leading float like JS `parseFloat` (0 when there is none).
fn parse_float(s: &str) -> f64 {
    let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    s[..end].parse().unwrap_or(0.0)
}

pub fn read_sheet(text: &str) -> Sheet {
    let mut out = Sheet::default();
    let mut keep: Vec<String> = Vec::new();
    let text = text.replace('\r', "");
    let lines: Vec<&str> = text.split('\n').collect();
    let at = |i: usize| lines.get(i).copied().unwrap_or("");
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        // "Current session: 67% used · resets Oct 30, 9:20pm"
        if let Some(m) = USED.captures(l) {
            out.meters.push(Meter { label: m[1].trim().into(), pct: m[2].parse().unwrap_or(0.0), sub: m.get(3).map(|x| x.as_str().trim().to_string()) });
            i += 1;
            continue;
        }
        // "**Tokens:** 24.9k / 1m (2%)"
        if let Some(t) = TOKENS.captures(l) {
            out.meters.push(Meter { label: "Context window".into(), pct: t[3].parse().unwrap_or(0.0), sub: Some(format!("{} of {} tokens", t[1].trim(), t[2].trim())) });
            i += 1;
            continue;
        }
        if let Some(mo) = MODEL.captures(l) {
            if out.model.is_none() {
                out.model = Some(mo[1].replace('*', "").trim().to_string());
                i += 1;
                continue;
            }
        }
        // "Last 24h · 870 requests" followed by "  90%  Start fresh when you switch tasks"
        if RANK_HEAD.is_match(l) && RANK_ROW1.is_match(at(i + 1)) {
            let mut r = Ranked { heading: l.trim().into(), rows: vec![] };
            while i + 1 < lines.len() && INDENTED.is_match(lines[i + 1]) {
                i += 1;
                match RANK_ROW.captures(lines[i]) {
                    Some(row) => r.rows.push(RankedRow { pct: row[1].parse().unwrap_or(0.0), text: row[2].trim().into() }),
                    None => r.rows.push(RankedRow { pct: -1.0, text: lines[i].trim().into() }),
                }
            }
            out.ranked.push(r);
            i += 1;
            continue;
        }
        // The "| Category | Tokens | Percentage |" table of /context.
        if CAT_HEAD.is_match(l) && CAT_SEP.is_match(at(i + 1)) {
            i += 1;
            while i + 1 < lines.len() && PIPE.is_match(lines[i + 1]) {
                i += 1;
                let c: Vec<&str> = lines[i].split('|').map(|x| x.trim()).filter(|x| !x.is_empty()).collect();
                if c.len() >= 3 {
                    out.categories.push(Category { name: c[0].into(), value: c[1].into(), pct: parse_float(c[2]) });
                }
            }
            i += 1;
            continue;
        }
        keep.push(l.to_string());
        i += 1;
    }
    // Short lines before anything recognised read as an intro ("You are using your subscription…").
    while keep.first().is_some_and(|k| k.trim().is_empty()) {
        keep.remove(0);
    }
    if !out.meters.is_empty() || !out.ranked.is_empty() {
        while keep.first().is_some_and(|k| !k.trim().is_empty() && !HEADING_OR_PIPE.is_match(k) && k.encode_utf16().count() < 140) {
            out.intro.push(keep.remove(0).trim().to_string());
        }
    }
    let rest = keep.join("\n");
    let rest = CONTEXT_TITLE.replace(&rest, "");
    let rest = CATEGORY_TITLE.replace(&rest, "");
    out.rest = BLANKS.replace_all(&rest, "\n\n").trim().to_string();
    out
}
