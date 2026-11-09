//! Pure artifact-format helpers from `artifacts.ts` (the parts `formats.test.ts` covers without the store):
//! which format a save takes, how a version's format is read, and the exports each kind offers.
//! Plus the page renderers (`normalizeType`, `markdownDeck`, `toHtml`) the artifact store uses.

use serde::Serialize;
use workspaceai_shared::markdown::{esc, render_markdown};
use workspaceai_shared::types::{Artifact, ArtifactFormat, ArtifactType, ArtifactVersion};

const FILE_EXT: [(ArtifactFormat, &str); 5] =
    [(ArtifactFormat::Markdown, "md"), (ArtifactFormat::Html, "html"), (ArtifactFormat::Csv, "csv"), (ArtifactFormat::Svg, "svg"), (ArtifactFormat::Json, "json")];

fn allows(kind: &ArtifactType) -> &'static [ArtifactFormat] {
    use ArtifactFormat::*;
    match kind {
        ArtifactType::Doc | ArtifactType::Deck | ArtifactType::DesignSystem => &[Markdown, Html],
        ArtifactType::Video => &[Markdown, Html, Json],
        ArtifactType::Prototype | ArtifactType::LiveArtifact => &[Html],
        ArtifactType::Sheet => &[Csv, Html],
        ArtifactType::Image => &[Svg, Html],
    }
}

pub fn looks_html(s: &str) -> bool {
    let t = s.trim_start().to_ascii_lowercase();
    if t.starts_with("<!doctype html") || t.starts_with("<html") {
        return true;
    }
    ["<head", "<body", "<main", "<section", "<div", "<style", "<svg"].iter().any(|tag| {
        t.strip_prefix(tag).is_some_and(|rest| rest.starts_with('>') || rest.starts_with(' ') || rest.starts_with('\n') || rest.starts_with('\t') || rest.starts_with('\r'))
    })
}

/// The format a save is in: the one asked for if this kind takes it, else HTML when the content is HTML,
/// else the kind's default. `"md"` is accepted for `markdown`.
pub fn resolve_format(kind: &ArtifactType, content: &str, requested: Option<&str>) -> ArtifactFormat {
    let allowed = allows(kind);
    let asked = match requested {
        Some("md") | Some("markdown") => Some(ArtifactFormat::Markdown),
        Some("html") => Some(ArtifactFormat::Html),
        Some("csv") => Some(ArtifactFormat::Csv),
        Some("svg") => Some(ArtifactFormat::Svg),
        Some("json") => Some(ArtifactFormat::Json),
        _ => None,
    };
    if let Some(f) = asked.filter(|f| allowed.contains(f)) {
        return f;
    }
    if allowed.contains(&ArtifactFormat::Html) && looks_html(content) {
        return ArtifactFormat::Html;
    }
    allowed[0].clone()
}

/// A version's format: what it records, else the file extension, else Markdown.
pub fn format_of(version: &ArtifactVersion) -> ArtifactFormat {
    if let Some(f) = &version.format {
        return f.clone();
    }
    let ext = version.file.rsplit('.').next().unwrap_or_default();
    FILE_EXT.iter().find(|(_, e)| *e == ext).map(|(f, _)| f.clone()).unwrap_or(ArtifactFormat::Markdown)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArtifactExport {
    pub label: String,
    pub ext: String,
    pub format: String,
}

fn export(label: &str, ext: &str, format: &str) -> ArtifactExport {
    ArtifactExport { label: label.into(), ext: ext.into(), format: format.into() }
}

/// The exports each format can give, as the panel lists them.
pub fn exports_for(a: &Artifact) -> Vec<ArtifactExport> {
    let f = a.versions.last().map(format_of).unwrap_or(ArtifactFormat::Markdown);
    let md = if f == ArtifactFormat::Markdown { vec![export("Markdown", ".md", "md")] } else { vec![] };
    let html = export("Single-file HTML", ".html", "html");
    let pdf = export("PDF", ".pdf", "pdf");
    match a.kind {
        ArtifactType::Deck => {
            let mut out = vec![export("PowerPoint", ".pptx", "pptx"), pdf, export("Single-file HTML deck", ".html", "html")];
            out.extend(md);
            out
        }
        ArtifactType::Sheet if f == ArtifactFormat::Csv => vec![export("CSV", ".csv", "csv"), export("HTML table", ".html", "html")],
        ArtifactType::Sheet => vec![html],
        _ if f == ArtifactFormat::Svg => vec![export("SVG", ".svg", "svg"), html],
        _ if f == ArtifactFormat::Json => vec![export("Storyboard JSON", ".json", "json")],
        _ => {
            let mut out = md;
            out.push(html);
            out.push(pdf);
            out
        }
    }
}

/// File extension a format is saved under.
pub fn file_ext(f: &ArtifactFormat) -> &'static str {
    FILE_EXT.iter().find(|(x, _)| x == f).map(|(_, e)| *e).unwrap_or("md")
}

/// The latest version's format (Markdown when there is none).
pub fn latest_format(a: &Artifact) -> ArtifactFormat {
    a.versions.last().map(format_of).unwrap_or(ArtifactFormat::Markdown)
}

/// A free-form kind ("slides", "web page", "csv") as an artifact type; anything else is a Doc.
pub fn normalize_type(t: &str) -> ArtifactType {
    let s = t.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| s.contains(w));
    let design_system = s.match_indices("design").any(|(i, _)| {
        let rest = &s[i + 6..];
        rest.starts_with("system") || rest.chars().next().is_some_and(|c| rest[c.len_utf8()..].starts_with("system"))
    });
    if any(&["deck", "slide", "present", "pptx"]) {
        ArtifactType::Deck
    } else if any(&["proto", "html", "page", "web", "site", "app", "screen"]) {
        ArtifactType::Prototype
    } else if any(&["sheet", "csv", "table", "xlsx", "spread"]) {
        ArtifactType::Sheet
    } else if any(&["image", "svg", "picture"]) {
        ArtifactType::Image
    } else if any(&["video", "motion"]) {
        ArtifactType::Video
    } else if s.contains("live") {
        ArtifactType::LiveArtifact
    } else if design_system {
        ArtifactType::DesignSystem
    } else {
        ArtifactType::Doc
    }
}

/// Open Design's mode name for a kind (`od.mode` in the manifest).
pub fn od_mode(t: &ArtifactType) -> &'static str {
    match t {
        ArtifactType::Doc => "doc",
        ArtifactType::Deck => "deck",
        ArtifactType::Prototype => "prototype",
        ArtifactType::Sheet => "sheet",
        ArtifactType::Image => "image",
        ArtifactType::Video => "video",
        ArtifactType::LiveArtifact => "live-artifact",
        ArtifactType::DesignSystem => "design-system",
    }
}

/// Markdown slides: sections split by a line of three or more dashes (`md.split(/\n-{3,}\n/)`).
pub fn split_slides(md: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let (mut start, mut i) = (0, 0);
    let b = md.as_bytes();
    while i < b.len() {
        if b[i] == b'\n' {
            let dashes = b[i + 1..].iter().take_while(|&&c| c == b'-').count();
            if dashes >= 3 && b.get(i + 1 + dashes) == Some(&b'\n') {
                parts.push(&md[start..i]);
                i += dashes + 2;
                start = i;
                continue;
            }
        }
        i += 1;
    }
    parts.push(&md[start..]);
    parts.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

const PAGE_CSS: &str = "body{font-family:Inter,system-ui,sans-serif;background:#faf9f5;color:#141413;max-width:820px;margin:40px auto;padding:0 24px;line-height:1.6}h1,h2,h3,h4{font-family:Georgia,serif;font-weight:500;line-height:1.2}a{color:#cc785c}code{font:0.9em ui-monospace,monospace;background:#efe9de;padding:1px 4px;border-radius:4px}pre{background:#efe9de;padding:12px 14px;border-radius:8px;overflow:auto}pre code{background:none;padding:0}blockquote{margin:0;padding-left:16px;border-left:3px solid #cc785c;color:#5e5d59}hr{border:0;border-top:1px solid #e6dfd8;margin:28px 0}table{border-collapse:collapse}td,th{border:1px solid #e6dfd8;padding:6px 10px;text-align:left}.slide{aspect-ratio:16/9;border:1px solid #e6dfd8;border-radius:10px;padding:40px;margin:0 0 24px;page-break-after:always;display:flex;flex-direction:column;justify-content:center}";

/// Slide typography for Markdown decks on Open Design's 1920x1080 canvas (its per-deck style slot).
const MD_DECK_CSS: &str = "
    .md-slide{height:100%;padding:120px 160px;display:flex;flex-direction:column;justify-content:center;gap:40px;font-family:Inter,system-ui,sans-serif}
    .slide.dark{background:var(--fg)}
    .slide.dark .md-slide,.slide.dark .md-slide p,.slide.dark .md-slide li,.slide.dark .md-slide td,.slide.dark .md-slide th{color:var(--bg)}
    .slide.hero .md-slide h1{font-size:168px}
    .md-slide h1{font:500 128px/1.02 Georgia,serif;letter-spacing:-0.025em}
    .md-slide h2{font:500 96px/1.06 Georgia,serif;letter-spacing:-0.02em}
    .md-slide h3,.md-slide h4{font:600 56px/1.2 Inter,system-ui,sans-serif}
    .md-slide p,.md-slide li,.md-slide td,.md-slide th{font-size:46px;line-height:1.4;color:var(--fg)}
    .md-slide ul,.md-slide ol{padding-left:1.1em;display:flex;flex-direction:column;gap:18px}
    .md-slide blockquote{border-left:6px solid var(--accent);padding-left:36px;color:var(--muted)}
    .md-slide table{border-collapse:collapse}.md-slide td,.md-slide th{border-bottom:2px solid #e6dfd8;padding:14px 28px 14px 0;text-align:left}
    .md-slide code{font-family:ui-monospace,monospace;font-size:0.9em}
    .md-slide a{color:var(--accent)}";

/// A Markdown deck as an Open Design deck: its fixed framework with one slide per `---` section.
/// Empty when the skeleton lacks the stage or chrome markers.
pub fn markdown_deck(title: &str, md: &str, skeleton: &str) -> String {
    let parts = split_slides(md);
    let n = parts.len();
    // Open Design's theme rhythm: a hero cover, never three same-theme slides in a row, a hero close on long decks.
    let theme = |i: usize| if i == 0 { "hero dark" } else if n >= 8 && i == n - 1 { "hero light" } else if i % 3 == 0 { "dark" } else { "light" };
    let slides: Vec<String> = parts
        .iter()
        .enumerate()
        .map(|(i, s)| {
            format!(
                "      <section class=\"slide {}{}\" data-od-id=\"s{}\" data-screen-label=\"{:02}\"><div class=\"md-slide\">{}</div></section>",
                theme(i),
                if i == 0 { " active" } else { "" },
                i + 1,
                i + 1,
                render_markdown(s, 0)
            )
        })
        .collect();
    let Some(stage) = skeleton.find("id=\"deck-stage\"") else { return String::new() };
    let Some(gt) = skeleton[stage..].find('>') else { return String::new() };
    let open = stage + gt + 1;
    let Some(chrome) = skeleton.find("<!-- Framework chrome") else { return String::new() };
    // The second `</div>` back from the chrome closes the stage.
    let Some(last) = skeleton[..chrome].rfind("</div>") else { return String::new() };
    let Some(close) = skeleton[..last].rfind("</div>") else { return String::new() };
    if close < open {
        return String::new();
    }
    let mut out = format!("{}\n{}\n    {}", &skeleton[..open], slides.join("\n"), &skeleton[close..]);
    out = out.replacen("<title><!-- SLOT: deck title --></title>", &format!("<title>{}</title>", esc(title)), 1);
    // The framework's own comment names a slide tag; Open Design's lint would count it as an unlabelled slide.
    out = out.replacen("inside <section class=\"slide\"> bodies", "inside the slide sections", 1);
    if let Some(slot) = out.find("SLOT: per-deck styles").and_then(|i| out[i..].find("*/").map(|j| i + j)) {
        out.insert_str(slot + 2, MD_DECK_CSS);
    }
    out
}

const PRINT: &str = "<script>window.onload=()=>window.print()</script>";

/// The single HTML page for a version: HTML as written, Markdown rendered (decks on `deck_skeleton` when given).
pub fn to_html(a: &Artifact, content: &str, print: bool, format: &ArtifactFormat, deck_skeleton: Option<&str>) -> String {
    let wrap = |body: String| {
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>{PAGE_CSS}</style></head><body>{body}{}</body></html>",
            esc(&a.title),
            if print { PRINT } else { "" }
        )
    };
    match format {
        ArtifactFormat::Html | ArtifactFormat::Svg => {
            let page = if *format == ArtifactFormat::Svg {
                format!("<!doctype html><html><body style=\"margin:0;display:grid;place-items:center;min-height:100vh\">{content}</body></html>")
            } else {
                content.to_string()
            };
            match (print, page.contains("</body>")) {
                (false, _) => page,
                (true, true) => page.replacen("</body>", &format!("{PRINT}</body>"), 1),
                (true, false) => page + PRINT,
            }
        }
        ArtifactFormat::Csv => {
            let rows: String = content
                .split('\n')
                .filter(|r| !r.is_empty())
                .enumerate()
                .map(|(i, r)| {
                    let cells: String = r.split(',').map(|c| if i > 0 { format!("<td>{}</td>", esc(c)) } else { format!("<th>{}</th>", esc(c)) }).collect();
                    format!("<tr>{cells}</tr>")
                })
                .collect();
            wrap(format!("<table>{rows}</table>"))
        }
        ArtifactFormat::Json => wrap(format!("<pre><code>{}</code></pre>", esc(content))),
        ArtifactFormat::Markdown if a.kind == ArtifactType::Deck => {
            let deck = match deck_skeleton {
                Some(sk) if !print => markdown_deck(&a.title, content, sk),
                _ => String::new(),
            };
            if !deck.is_empty() {
                return deck;
            }
            wrap(split_slides(content).iter().map(|s| format!("<section class=\"slide\">{}</section>", render_markdown(s, 0))).collect::<Vec<_>>().join("\n"))
        }
        ArtifactFormat::Markdown => wrap(render_markdown(content, 0)),
    }
}
