//! Port of `formats.test.ts`: which format a save takes, the exports each kind offers, Markdown tables, and a
//! Markdown deck laid onto a deck skeleton. The Open Design deck-framework case needs Open Design (B10).

use workspaceai_shared::markdown::render_markdown;
use workspaceai_shared::types::{Artifact, ArtifactFormat, ArtifactType, ArtifactVersion};
use workspaceai_supervisor::formats::{exports_for, format_of, markdown_deck, normalize_type, resolve_format, split_slides, to_html};

fn art(kind: ArtifactType, file: &str) -> Artifact {
    Artifact {
        id: "a".into(),
        title: "T".into(),
        kind,
        versions: vec![ArtifactVersion { v: 1, file: file.into(), ..Default::default() }],
        ..Default::default()
    }
}

#[test]
fn picks_markdown_or_html_per_kind() {
    assert_eq!(resolve_format(&ArtifactType::Doc, "# Plan", None), ArtifactFormat::Markdown);
    assert_eq!(resolve_format(&ArtifactType::Doc, "<!doctype html><html></html>", None), ArtifactFormat::Html);
    assert_eq!(resolve_format(&ArtifactType::Doc, "# Plan", Some("html")), ArtifactFormat::Html);
    assert_eq!(resolve_format(&ArtifactType::Deck, "# One\n\n---\n\n# Two", Some("md")), ArtifactFormat::Markdown);
    // Prototypes are HTML only.
    assert_eq!(resolve_format(&ArtifactType::Prototype, "<div>x</div>", Some("markdown")), ArtifactFormat::Html);
    assert_eq!(resolve_format(&ArtifactType::Sheet, "a,b\n1,2", None), ArtifactFormat::Csv);
    assert_eq!(format_of(&ArtifactVersion { file: "v3.html".into(), ..Default::default() }), ArtifactFormat::Html);
    assert_eq!(
        format_of(&ArtifactVersion { file: "v1.md".into(), format: Some(ArtifactFormat::Markdown), ..Default::default() }),
        ArtifactFormat::Markdown
    );
}

#[test]
fn offers_the_exports_each_format_can_give() {
    let formats = |kind, file| exports_for(&art(kind, file)).into_iter().map(|e| e.format).collect::<Vec<_>>();
    assert_eq!(formats(ArtifactType::Doc, "v1.md"), ["md", "html", "pdf"]);
    assert_eq!(formats(ArtifactType::Doc, "v1.html"), ["html", "pdf"]);
    assert_eq!(formats(ArtifactType::Deck, "v1.html"), ["pptx", "pdf", "html"]);
    assert_eq!(formats(ArtifactType::Sheet, "v1.csv"), ["csv", "html"]);
}

#[test]
fn renders_markdown_tables_quotes_and_rules() {
    let html = render_markdown("# Title\n\n> said\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n---", 0);
    assert_eq!(html, "<h1>Title</h1><blockquote><p>said</p></blockquote><table><thead><tr><th>a</th><th>b</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table><hr>");
}

#[test]
fn lays_a_markdown_deck_onto_a_deck_skeleton() {
    let skeleton = "<html><head><title><!-- SLOT: deck title --></title><style>/* SLOT: per-deck styles */</style></head><body><div class=\"deck-shell\">\n<div class=\"deck-stage\" id=\"deck-stage\">\n<section class=\"slide active\">SLOT</section>\n</div>\n</div>\n<!-- Framework chrome --><nav></nav></body></html>";
    let out = markdown_deck("Pitch <1>", "# Hello\n\n---\n\n- point", skeleton);
    assert!(out.contains("<title>Pitch &lt;1&gt;</title>"), "{out}");
    assert!(out.contains("<section class=\"slide hero dark active\" data-od-id=\"s1\" data-screen-label=\"01\"><div class=\"md-slide\"><h1>Hello</h1></div></section>"), "{out}");
    assert!(out.contains("<section class=\"slide light\" data-od-id=\"s2\" data-screen-label=\"02\"><div class=\"md-slide\"><ul><li>point</li></ul></div></section>"), "{out}");
    assert!(!out.contains("SLOT</section>"));
    assert!(out.contains(".md-slide{"));
    assert!(out.contains("<!-- Framework chrome --><nav></nav>"));
    assert_eq!(markdown_deck("x", "# a", "<html>no stage</html>"), "");
}

#[test]
fn uses_open_designs_own_deck_framework_when_installed() {
    eprintln!("skipped: Open Design's deck framework is read by opendesign.rs (B10)");
}

#[test]
fn markdown_decks_without_a_framework_and_other_pages() {
    let deck = art(ArtifactType::Deck, "v1.md");
    let html = to_html(&deck, "# One\n\n---\n\n# Two", false, &ArtifactFormat::Markdown, None);
    assert_eq!(html.matches("<section class=\"slide\">").count(), 2);
    let sheet = art(ArtifactType::Sheet, "v1.csv");
    assert!(to_html(&sheet, "a,b\n<1>,2", false, &ArtifactFormat::Csv, None).contains("<tr><th>a</th><th>b</th></tr><tr><td>&lt;1&gt;</td><td>2</td></tr>"));
    let page = to_html(&art(ArtifactType::Prototype, "v1.html"), "<html><body>hi</body></html>", true, &ArtifactFormat::Html, None);
    assert!(page.ends_with("window.print()</script></body></html>"));
    assert_eq!(split_slides("a\n---\nb\n\n-----\n\nc\n---"), ["a", "b", "c\n---"]);
    assert_eq!(normalize_type("slides"), ArtifactType::Deck);
    assert_eq!(normalize_type("web page"), ArtifactType::Prototype);
    assert_eq!(normalize_type("design-system"), ArtifactType::DesignSystem);
    assert_eq!(normalize_type("Design System"), ArtifactType::DesignSystem);
    assert_eq!(normalize_type("doc"), ArtifactType::Doc);
}
