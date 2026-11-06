//! Port of the first two `formats.test.ts` cases: which format a save takes and the exports each kind offers.
//! The Markdown-table, deck-skeleton and Open Design page cases live with the artifact store (B9).

use workspaceai_shared::types::{Artifact, ArtifactFormat, ArtifactType, ArtifactVersion};
use workspaceai_supervisor::formats::{exports_for, format_of, resolve_format};

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
