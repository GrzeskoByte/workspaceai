//! Chat Markdown as flat blocks with styled byte ranges, so any renderer can draw it. Lists are flattened to
//! items with a marker and depth. Soft line breaks stay line breaks, as people type them in chat.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    /// Shared, so a long URL on a long link costs one allocation, not one per styled range.
    pub link: Option<Arc<str>>,
}

/// Plain text plus styled byte ranges into it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inline {
    pub text: String,
    pub styles: Vec<(Range<usize>, Style)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Inline),
    Heading { level: u8, text: Inline },
    Code { lang: String, text: String },
    ListItem { marker: String, depth: usize, text: Inline },
    Quote(Inline),
    Rule,
}

#[derive(Clone, Copy)]
enum Target {
    Paragraph,
    Heading(u8),
    Item,
}

struct Builder {
    blocks: Vec<Block>,
    inline: Inline,
    target: Option<Target>,
    bold: u32,
    italic: u32,
    /// One entry per open link; `None` for a destination the allowlist refused (it must not fall back to an outer URL).
    link: Vec<Option<Arc<str>>>,
    lists: Vec<Option<u64>>,
    marker: String,
    /// Nesting depth of the item being built, fixed when it starts (its sublist may open before it is flushed).
    depth: usize,
    code: Option<(String, String)>,
    in_quote: bool,
}

impl Builder {
    fn push_text(&mut self, text: &str, code: bool) {
        if text.is_empty() {
            return;
        }
        let start = self.inline.text.len();
        self.inline.text.push_str(text);
        let style = Style { bold: self.bold > 0, italic: self.italic > 0, code, link: self.link.last().cloned().flatten() };
        if style != Style::default() {
            self.inline.styles.push((start..self.inline.text.len(), style));
        }
    }

    fn flush(&mut self) {
        let inline = std::mem::take(&mut self.inline);
        let Some(target) = self.target.take() else { return };
        if inline.text.is_empty() {
            return;
        }
        self.blocks.push(match target {
            Target::Paragraph if self.in_quote => Block::Quote(inline),
            Target::Paragraph => Block::Paragraph(inline),
            Target::Heading(level) => Block::Heading { level, text: inline },
            Target::Item => Block::ListItem { marker: std::mem::take(&mut self.marker), depth: self.depth, text: inline },
        });
    }
}

/// Longest link kept; longer ones render as plain text.
const MAX_LINK: usize = 2048;

/// Only web and mail links stay links. Everything else (`javascript:`, `file:`, custom schemes) is plain text.
fn allowed_link(url: &str) -> Option<Arc<str>> {
    let u = url.trim();
    let scheme = u.split(':').next().unwrap_or("").to_ascii_lowercase();
    (u.len() <= MAX_LINK && matches!(scheme.as_str(), "http" | "https" | "mailto") && u.contains(':')).then(|| Arc::from(u))
}

pub fn parse(md: &str) -> Vec<Block> {
    let mut b = Builder { blocks: vec![], inline: Inline::default(), target: None, bold: 0, italic: 0, link: vec![], lists: vec![], marker: String::new(), depth: 0, code: None, in_quote: false };
    for event in Parser::new_ext(md, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(Tag::Paragraph) => b.target = Some(b.target.take().unwrap_or(Target::Paragraph)),
            Event::End(TagEnd::Paragraph) => b.flush(),
            Event::Start(Tag::Heading { level, .. }) => b.target = Some(Target::Heading(level as u8)),
            Event::End(TagEnd::Heading(_)) => b.flush(),
            Event::Start(Tag::BlockQuote(_)) => b.in_quote = true,
            Event::End(TagEnd::BlockQuote(_)) => {
                b.flush();
                b.in_quote = false;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                b.code = Some((lang, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((lang, text)) = b.code.take() {
                    b.blocks.push(Block::Code { lang, text });
                }
            }
            Event::Start(Tag::List(first)) => b.lists.push(first),
            Event::End(TagEnd::List(_)) => {
                b.lists.pop();
            }
            Event::Start(Tag::Item) => {
                b.flush();
                let marker = match b.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}.");
                        *n += 1;
                        m
                    }
                    _ => "•".to_string(),
                };
                b.marker = marker;
                b.depth = b.lists.len().saturating_sub(1);
                b.target = Some(Target::Item);
            }
            Event::End(TagEnd::Item) => b.flush(),
            Event::Start(Tag::Strong) => b.bold += 1,
            Event::End(TagEnd::Strong) => b.bold = b.bold.saturating_sub(1),
            Event::Start(Tag::Emphasis) => b.italic += 1,
            Event::End(TagEnd::Emphasis) => b.italic = b.italic.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. }) => b.link.push(allowed_link(&dest_url)),
            Event::End(TagEnd::Link) => {
                b.link.pop();
            }
            Event::Text(t) => match &mut b.code {
                Some((_, text)) => text.push_str(&t),
                None => {
                    b.target.get_or_insert(Target::Paragraph);
                    b.push_text(&t, false)
                }
            },
            Event::Code(t) => {
                b.target.get_or_insert(Target::Paragraph);
                b.push_text(&t, true)
            }
            Event::SoftBreak | Event::HardBreak => b.push_text("\n", false),
            Event::Rule => b.blocks.push(Block::Rule),
            _ => {}
        }
    }
    b.flush();
    b.blocks
}
