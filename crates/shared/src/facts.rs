//! Facts are short statements kept as Markdown files (R-22): workspace scope in `~/.harness/ws/<id>/memory/facts.md`,
//! project scope in `<project>/.harness/memory/facts.md`. Port of `packages/shared/src/facts.ts`.

use crate::types::{Fact, FactStatus};
use regex::Regex;
use std::sync::LazyLock;

static LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^- (.*?)\s*<!--\s*(.*?)\s*-->\s*$").unwrap());

/// JS `encodeURIComponent`.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// JS `decodeURIComponent`; malformed input is returned as written instead of throwing.
pub fn decode_uri_component(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 1) && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

pub fn parse_facts(md: &str, scope: &str) -> Vec<Fact> {
    let mut out: Vec<Fact> = Vec::new();
    for raw in md.split('\n') {
        let Some(m) = LINE.captures(raw.trim()) else { continue };
        let mut meta = std::collections::HashMap::new();
        for kv in m[2].split_whitespace() {
            if let Some(i) = kv.find(':').filter(|i| *i > 0) {
                meta.insert(&kv[..i], &kv[i + 1..]);
            }
        }
        let id = meta.get("id").filter(|s| !s.is_empty()).map(|s| s.to_string()).unwrap_or_else(|| format!("f_{}", out.len()));
        out.push(Fact {
            id,
            text: m[1].to_string(),
            scope: scope.to_string(),
            status: if meta.get("status") == Some(&"suggested") { FactStatus::Suggested } else { FactStatus::Approved },
            source: meta.get("source").filter(|s| !s.is_empty()).map(|s| decode_uri_component(s)),
            created_at: meta.get("at").and_then(|s| s.parse::<f64>().ok()).filter(|n| *n != 0.0).map(|n| n as i64).unwrap_or(0),
            extra: Default::default(),
        });
    }
    out
}

pub fn serialize_facts(facts: &[Fact], title: &str) -> String {
    let mut lines = vec![
        format!("# {title}"),
        String::new(),
        "<!-- Managed by WorkspaceAI. Each line is one fact; suggestions wait for approval. -->".to_string(),
        String::new(),
    ];
    for f in facts {
        let mut meta = vec![format!("id:{}", f.id), format!("status:{}", f.status.as_str()), format!("at:{}", f.created_at)];
        if let Some(s) = f.source.as_ref().filter(|s| !s.is_empty()) {
            meta.push(format!("source:{}", encode_uri_component(s)));
        }
        static NL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n+").unwrap());
        let text = NL.replace_all(&f.text, " ").replace("<!--", "").replace("-->", "");
        lines.push(format!("- {text} <!-- {} -->", meta.join(" ")));
    }
    lines.join("\n") + "\n"
}

/// Simple relevance search shared by the UI filter and the memory MCP `memory_search` tool.
pub fn search_facts<'a>(facts: &'a [Fact], query: &str) -> Vec<&'a Fact> {
    let q = query.to_lowercase();
    // JS `\W` is ASCII-only: any other character separates terms.
    let terms: Vec<&str> = q.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|t| t.len() > 1).collect();
    let approved = facts.iter().filter(|f| f.status == FactStatus::Approved);
    if terms.is_empty() {
        return approved.collect();
    }
    let mut scored: Vec<(&Fact, usize)> = approved
        .map(|f| {
            let t = f.text.to_lowercase();
            (f, terms.iter().filter(|x| t.contains(**x)).count())
        })
        .filter(|(_, s)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().map(|(f, _)| f).collect()
}
