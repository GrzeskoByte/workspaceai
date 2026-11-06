//! Pure artifact-format helpers from `artifacts.ts` (the parts `formats.test.ts` covers without the store):
//! which format a save takes, how a version's format is read, and the exports each kind offers.
//! `normalizeType`, `markdownDeck` and `toHtml` need the artifact store / Open Design and arrive with B9.

use serde::Serialize;
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

fn looks_html(s: &str) -> bool {
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
