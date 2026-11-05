//! The command palette (Ctrl+K): everything you can jump to or do, filtered as you type.

use crate::settings;
use crate::types::{Snapshot, WorkspaceSummary};

pub const MAX_RESULTS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Action,
    Workspace,
    Chat,
    /// A settings page.
    Page,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub kind: Kind,
    pub id: String,
    pub label: String,
    pub hint: String,
}

fn item(kind: Kind, id: &str, label: &str, hint: &str) -> Item {
    Item { kind, id: id.into(), label: label.into(), hint: hint.into() }
}

/// Actions, workspaces, the workspace's chats (newest first, archived hidden) and settings pages.
pub fn build(workspaces: &[WorkspaceSummary], snap: &Snapshot, is_org: bool) -> Vec<Item> {
    let mut out = vec![item(Kind::Action, "new-chat", "New chat", "action"), item(Kind::Action, "settings", "Open settings", "action")];
    out.extend(workspaces.iter().map(|w| item(Kind::Workspace, &w.id, &w.name, &w.kind)));
    let mut chats: Vec<_> = snap.chats.iter().filter(|c| !c.archived).collect();
    chats.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
    out.extend(chats.into_iter().map(|c| item(Kind::Chat, &c.id, if c.title.is_empty() { "New chat" } else { &c.title }, "chat")));
    out.extend(settings::pages(is_org).into_iter().map(|p| item(Kind::Page, p.id, &format!("Settings: {}", p.title), "settings")));
    out
}

/// 0 prefix, 1 word start, 2 contained, 3 letters in order; `None` when it does not match.
fn score(label: &str, q: &str) -> Option<u8> {
    if q.is_empty() || label.starts_with(q) {
        return Some(0);
    }
    let word_start = label.char_indices().any(|(i, _)| (i == 0 || !label[..i].chars().next_back().is_some_and(char::is_alphanumeric)) && label[i..].starts_with(q));
    if word_start {
        return Some(1);
    }
    if label.contains(q) {
        return Some(2);
    }
    let mut chars = label.chars();
    q.chars().all(|c| chars.any(|l| l == c)).then_some(3)
}

/// Matches for `query`, best first, ties in the original order, at most `MAX_RESULTS`.
pub fn filter<'a>(items: &'a [Item], query: &str) -> Vec<&'a Item> {
    let q = query.trim().to_lowercase();
    let mut scored: Vec<(u8, &Item)> = items.iter().filter_map(|i| score(&i.label.to_lowercase(), &q).map(|s| (s, i))).collect();
    scored.sort_by_key(|(s, _)| *s);
    scored.into_iter().map(|(_, i)| i).take(MAX_RESULTS).collect()
}
