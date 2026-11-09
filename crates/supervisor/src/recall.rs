//! Memory recall on every message (port of `recall.ts`). Facts are looked up and handed to the agent as a
//! `<harness-memory>` block; graph recall and `chatMemory` are deferred with the Graphify backend (plan non-goal).

use serde_json::Value;
use workspaceai_shared::types::{Fact, FactStatus, MemoryBackend, Project, Session, Workspace};
use workspaceai_shared::util::project_path;

const OPEN: &str = "<harness-memory>";
const CLOSE: &str = "</harness-memory>";
const STOP: &str = "about above after again against also because been before being below between both could does doing down during \
each from further have having here into itself just more most other over same should some such than that their them then there \
these they this those through under until very were what when where which while will with would your yours make made want need \
please show tell help like using used want find look file files code";

/// The words in a message worth looking up: names, paths and longer words, minus common ones.
pub fn query_terms(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in words(text) {
        let special = word.contains(['.', '_', '/', '-']);
        if word.len() < 4 && !special {
            continue;
        }
        if is_stop(&word) {
            continue;
        }
        if !out.contains(&word) {
            out.push(word);
        }
        if out.len() == 16 {
            break;
        }
    }
    out
}

fn is_stop(word: &str) -> bool {
    STOP.split_whitespace().any(|s| s == word)
}

/// `[a-z0-9][a-z0-9_./-]*[a-z0-9]` over the lowercased text.
fn words(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in lower.chars() {
        let body = ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '.' | '/' | '-');
        if body {
            cur.push(ch);
        } else {
            push_word(&mut cur, &mut out);
        }
    }
    push_word(&mut cur, &mut out);
    out
}

/// The first and last character must be alphanumeric, so trailing dots or slashes do not count.
fn push_word(cur: &mut String, out: &mut Vec<String>) {
    let w = std::mem::take(cur);
    if w.len() >= 2 && w.starts_with(|c: char| c.is_ascii_alphanumeric()) && w.ends_with(|c: char| c.is_ascii_alphanumeric()) {
        out.push(w);
    }
}

/// Strip the recall block from a stored user message (the transcript shows only what the user typed).
pub fn strip_recall(text: &str) -> String {
    let mut out = text.to_string();
    loop {
        let trimmed = out.trim_end();
        let mut cut = None;
        for tag in ["memory", "graph", "vault"] {
            let close = format!("</harness-{tag}>");
            if !trimmed.ends_with(&close) {
                continue;
            }
            let open = format!("<harness-{tag}>");
            if let Some(at) = trimmed.rfind(&open) {
                cut = Some(trimmed[..at].trim_end().len());
            }
        }
        match cut {
            Some(len) => out.truncate(len),
            None => return out,
        }
    }
}

/// What memory has on this message, as a block appended to the prompt; empty when nothing matches.
pub fn memory_recall(ws: &Workspace, projects: &[Project], facts: &[Fact], pid: &str, text: &str) -> String {
    if ws.memory == MemoryBackend::Mcp || ws.use_memory == Some(false) || ws.memory_recall == Some(false) {
        return String::new();
    }
    let terms = query_terms(text);
    if terms.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = Vec::new();
    if matches!(ws.memory, MemoryBackend::Facts | MemoryBackend::Both) {
        let mut scopes = vec!["ws".to_string()];
        scopes.extend(project_path(projects, Some(pid)).iter().map(|p| p.id.clone()));
        let mut hits: Vec<(usize, &Fact)> = facts
            .iter()
            .filter(|f| f.status == FactStatus::Approved && scopes.contains(&f.scope))
            .map(|f| {
                let text = f.text.to_lowercase();
                (terms.iter().filter(|t| text.contains(*t)).count(), f)
            })
            .filter(|(s, _)| *s > 0)
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0));
        hits.truncate(6);
        if !hits.is_empty() {
            lines.push("Facts:".into());
            lines.extend(hits.iter().map(|(_, f)| format!("- {}", f.text)));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let tools = match ws.memory {
        MemoryBackend::Facts => "memory_search",
        MemoryBackend::Graph => "graphify_query",
        _ => "graphify_query and memory_search",
    };
    let body: String = lines.join("\n").chars().take(2400).collect();
    format!("\n\n{OPEN}\nFrom this workspace's memory, matched to the message above (use what is relevant; call {tools} for more):\n{body}\n{CLOSE}")
}

/// What memory holds, for the start of every session: approved facts for the project chain and the workspace.
pub fn memory_brief(ws: &Workspace, projects: &[Project], facts: &[Fact], pid: &str) -> String {
    if ws.memory == MemoryBackend::Mcp || ws.use_memory == Some(false) {
        return String::new();
    }
    let mut out: Vec<String> = Vec::new();
    if matches!(ws.memory, MemoryBackend::Facts | MemoryBackend::Both) {
        let mut scopes: Vec<String> = project_path(projects, Some(pid)).iter().rev().map(|p| p.id.clone()).collect();
        scopes.push("ws".into());
        let known: Vec<&Fact> = scopes
            .iter()
            .flat_map(|s| facts.iter().filter(move |f| &f.scope == s))
            .filter(|f| f.status == FactStatus::Approved)
            .take(15)
            .collect();
        if !known.is_empty() {
            out.push("Facts you already know:".into());
            out.extend(known.iter().map(|f| format!("- {}", f.text)));
        }
    }
    if out.is_empty() {
        return String::new();
    }
    let body: String = out.join("\n").chars().take(2400).collect();
    format!("### What memory holds now\n{body}")
}

/// The Graphify nodes a chat is about. Graph memory is deferred with the graph backend, so this is always empty.
pub fn chat_memory(_ws: &Workspace, _projects: &[Project], _chat: &Session) -> Vec<Value> {
    Vec::new()
}
