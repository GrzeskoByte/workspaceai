//! Small, safe Markdown renderer shared by the app and the supervisor: everything is escaped first, then a few
//! constructs (headings, lists, code, quotes, tables, links) are rendered. Port of `packages/shared/src/markdown.ts`.

use regex::Regex;
use std::sync::LazyLock;

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

re!(CODE, r"`([^`]+)`");
re!(BOLD, r"\*\*([^*]+)\*\*");
re!(ITALIC, r"(^|[^*])\*([^*\n]+)\*");
re!(STRIKE, r"~~([^~\n]+)~~");
re!(LINK, r"\[([^\]]+)\]\((https?://[^)\s]+)\)");
re!(BARE, r"(^|[\s(])(https?://[^\s<)]*[^\s<).,;:])");
re!(LIST_ITEM, r"^\s*([-*]|\d+\.)\s+");
re!(INDENT, r"^\s*");
re!(ORDERED, r"^\s*\d+\.");
re!(TASK, r"^\[( |x|X)\]\s+");
re!(HEADING, r"^(#{1,4})\s+(.*)$");
re!(HR, r"^\s*(-{3,}|\*{3,})\s*$");
re!(QUOTE, r"^>\s?");
re!(ROW, r"^\s*\|.*\|\s*$");
re!(SEPARATOR, r"^\s*\|?\s*:?-+:?\s*(\||$)");
re!(HEADING_START, r"^#{1,4}\s");

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn inline(s: &str) -> String {
    let s = esc(s);
    let s = CODE.replace_all(&s, "<code>$1</code>");
    let s = BOLD.replace_all(&s, "<b>$1</b>");
    let s = ITALIC.replace_all(&s, "${1}<i>${2}</i>");
    let s = STRIKE.replace_all(&s, "<s>$1</s>");
    let s = LINK.replace_all(&s, r#"<a href="${2}" target="_blank" rel="noreferrer noopener">${1}</a>"#);
    // A bare address. One inside the link above follows `"` or `>`, so it isn't matched again.
    BARE.replace_all(&s, r#"${1}<a href="${2}" target="_blank" rel="noreferrer noopener">${2}</a>"#).into_owned()
}

/// Consecutive list lines as nested lists, by indent, with `- [ ]` as a checkbox.
fn render_list(lines: &[&str]) -> String {
    let mut open: Vec<(usize, &str)> = Vec::new();
    let mut html = String::new();
    let close = |open: &mut Vec<(usize, &str)>, html: &mut String| {
        let (_, tag) = open.pop().unwrap();
        html.push_str(&format!("</li></{tag}>"));
    };
    for line in lines {
        let indent = INDENT.find(&line.replace('\t', "    ")).map_or(0, |m| m.end());
        while open.len() > 1 && open.last().unwrap().0 > indent {
            close(&mut open, &mut html);
        }
        match open.last() {
            Some((top, _)) if indent <= *top => html.push_str("</li>"),
            _ => {
                let tag = if ORDERED.is_match(line) { "ol" } else { "ul" };
                open.push((indent, tag));
                html.push_str(&format!("<{tag}>"));
            }
        }
        let text = LIST_ITEM.replace(line, "");
        if let Some(t) = TASK.captures(&text) {
            let checked = if &t[1] == " " { "" } else { " checked" };
            html.push_str(&format!("<li class=\"task\"><input type=\"checkbox\" disabled{checked}> {}", inline(&text[t[0].len()..])));
        } else {
            html.push_str(&format!("<li>{}", inline(&text)));
        }
    }
    while !open.is_empty() {
        close(&mut open, &mut html);
    }
    html
}

/// A GitLab description ready for the renderer above: comments and raw tags gone, images and relative links as links to GitLab.
pub fn gitlab_markdown(text: &str, ticket_url: &str) -> String {
    re!(COMMENT, r"(?s)<!--.*?-->");
    re!(DETAILS, r"(?i)</?(?:details|summary)[^>]*>");
    re!(BR, r"(?i)<br\s*/?>");
    re!(MD_LINK, r"(!?)\[([^\]]*)\]\((/[^)\s]*|https?://[^)\s]*)[^)]*\)");
    re!(ORIGIN, r"^https?://[^/]+");
    let project = ticket_url.split("/-/").next().unwrap_or("");
    let origin = ORIGIN.find(ticket_url).map_or("", |m| m.as_str());
    let t = COMMENT.replace_all(text, "");
    let t = DETAILS.replace_all(&t, "");
    let t = BR.replace_all(&t, "\n");
    let t = MD_LINK.replace_all(&t, |c: &regex::Captures| {
        let (image, label, url) = (&c[1], &c[2], &c[3]);
        let full = if url.starts_with("/uploads/") {
            format!("{project}{url}")
        } else if url.starts_with('/') {
            format!("{origin}{url}")
        } else {
            url.to_string()
        };
        let label = if label.is_empty() { if image.is_empty() { full.as_str() } else { "Image" } } else { label };
        format!("[{label}]({full})")
    });
    t.trim().to_string()
}

/// `shift` demotes headings (chat replies render `#` as h2); artifacts keep them as written.
pub fn render_markdown(md: &str, shift: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    let md = md.replace('\r', "");
    let lines: Vec<&str> = md.split('\n').collect();
    let at = |i: usize| lines.get(i).copied().unwrap_or("");
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if l.starts_with("```") {
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].starts_with("```") {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            out.push(format!("<pre><code>{}</code></pre>", esc(&body.join("\n"))));
            continue;
        }
        if let Some(h) = HEADING.captures(l) {
            let n = (h[1].len() + shift).min(6);
            out.push(format!("<h{n}>{}</h{n}>", inline(&h[2])));
            i += 1;
            continue;
        }
        if HR.is_match(l) {
            out.push("<hr>".into());
            i += 1;
            continue;
        }
        if QUOTE.is_match(l) {
            let mut q = Vec::new();
            while i < lines.len() && QUOTE.is_match(lines[i]) {
                q.push(QUOTE.replace(lines[i], "").into_owned());
                i += 1;
            }
            out.push(format!("<blockquote>{}</blockquote>", render_markdown(&q.join("\n"), shift)));
            continue;
        }
        if ROW.is_match(l) && SEPARATOR.is_match(at(i + 1)) {
            let cells = |r: &str| -> Vec<String> {
                let t = r.trim();
                let t = t.strip_prefix('|').unwrap_or(t);
                let t = t.strip_suffix('|').unwrap_or(t);
                t.split('|').map(|c| inline(c.trim())).collect()
            };
            let head = cells(l);
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && ROW.is_match(lines[i]) {
                rows.push(cells(lines[i]));
                i += 1;
            }
            let th: String = head.iter().map(|c| format!("<th>{c}</th>")).collect();
            let body: String = rows.iter().map(|r| format!("<tr>{}</tr>", r.iter().map(|c| format!("<td>{c}</td>")).collect::<String>())).collect();
            out.push(format!("<table><thead><tr>{th}</tr></thead><tbody>{body}</tbody></table>"));
            continue;
        }
        if LIST_ITEM.is_match(l) {
            let mut items = Vec::new();
            // A blank line between items doesn't end the list.
            while i < lines.len() && (LIST_ITEM.is_match(lines[i]) || (lines[i].trim().is_empty() && LIST_ITEM.is_match(at(i + 1)))) {
                if !lines[i].trim().is_empty() {
                    items.push(lines[i]);
                }
                i += 1;
            }
            out.push(render_list(&items));
            continue;
        }
        if l.trim().is_empty() {
            i += 1;
            continue;
        }
        // The first line always belongs to the paragraph (a lone `| a |` row is text), so the loop always advances.
        let mut para = vec![lines[i]];
        i += 1;
        while i < lines.len() && !lines[i].trim().is_empty() && !lines[i].starts_with("```") && !HEADING_START.is_match(lines[i]) && !lines[i].starts_with('>') && !HR.is_match(lines[i]) && !LIST_ITEM.is_match(lines[i]) {
            para.push(lines[i]);
            i += 1;
        }
        out.push(format!("<p>{}</p>", para.iter().map(|p| inline(p)).collect::<Vec<_>>().join("<br>")));
    }
    out.concat()
}
