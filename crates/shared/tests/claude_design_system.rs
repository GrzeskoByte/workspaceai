//! Port of `packages/shared/test/claude-design-system.test.ts`.
use flate2::{write::DeflateEncoder, Compression};
use std::io::Write;
use workspaceai_shared::claude_design_system::{claude_system_to_design_md, read_zip};

/// A minimal .zip writer: each file deflated, or stored when `store` is set.
fn zip(files: &[(&str, &[u8])], store: bool) -> Vec<u8> {
    let (mut locals, mut centrals) = (Vec::new(), Vec::new());
    let mut offset = 0u32;
    for (name, raw) in files {
        let body = if store {
            raw.to_vec()
        } else {
            let mut e = DeflateEncoder::new(Vec::new(), Compression::default());
            e.write_all(raw).unwrap();
            e.finish().unwrap()
        };
        let method: u16 = if store { 0 } else { 8 };
        let n = name.as_bytes();
        let mut local = vec![0u8; 30];
        local[0..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
        local[8..10].copy_from_slice(&method.to_le_bytes());
        local[18..22].copy_from_slice(&(body.len() as u32).to_le_bytes());
        local[22..26].copy_from_slice(&(raw.len() as u32).to_le_bytes());
        local[26..28].copy_from_slice(&(n.len() as u16).to_le_bytes());
        let mut central = vec![0u8; 46];
        central[0..4].copy_from_slice(&0x02014b50u32.to_le_bytes());
        central[10..12].copy_from_slice(&method.to_le_bytes());
        central[20..24].copy_from_slice(&(body.len() as u32).to_le_bytes());
        central[24..28].copy_from_slice(&(raw.len() as u32).to_le_bytes());
        central[28..30].copy_from_slice(&(n.len() as u16).to_le_bytes());
        central[42..46].copy_from_slice(&offset.to_le_bytes());
        offset += (local.len() + n.len() + body.len()) as u32;
        locals.extend([local, n.to_vec(), body]);
        centrals.extend([central, n.to_vec()]);
    }
    let cd: Vec<u8> = centrals.concat();
    let mut eocd = vec![0u8; 22];
    eocd[0..4].copy_from_slice(&0x06054b50u32.to_le_bytes());
    eocd[8..10].copy_from_slice(&(files.len() as u16).to_le_bytes());
    eocd[10..12].copy_from_slice(&(files.len() as u16).to_le_bytes());
    eocd[12..16].copy_from_slice(&(cd.len() as u32).to_le_bytes());
    eocd[16..20].copy_from_slice(&offset.to_le_bytes());
    [locals.concat(), cd, eocd].concat()
}

const TOKENS: &str = r##"{
  "name": "From tokens", "color": { "themes": [{ "id": "light", "name": "Light" }, { "id": "dark", "name": "Dark" }], "tokens": [
    { "name": "ink", "value": { "light": "#111111", "dark": "#eeeeee" }, "usage": "Text" },
    { "name": "link", "value": "{ink}", "usage": "Links | anchors" },
    { "name": "paper", "value": { "light": "#ffffff" } }
  ] },
  "type": { "families": { "sans": "Inter, sans-serif" }, "groups": [{ "name": "Text", "family": "sans", "styles": [{ "name": "body", "fontSize": "15px", "lineHeight": 1.5, "fontWeight": 400 }] }] },
  "spacing": { "tokens": [{ "name": "space-4", "value": "16px", "usage": "Padding" }] },
  "shadow": { "tokens": [{ "name": "raise", "value": { "light": "0 1px 2px #000", "dark": "none" } }] }
}"##;

fn system() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("design-system.json", br#"{"title":"Acme"}"#),
        ("tokens.json", TOKENS.as_bytes()),
        ("README.md", b"# Acme\n\nSharp and blue.\n\n## Colour\n\n```md\n# not a heading\n```\n"),
        ("Voice.md", b"# Voice\n\n## Tone\n\nPlain."),
        ("components/Button/README.md", b"# Button\n\n## States\n\nPrimary only."),
        ("components/Button/preview.html", b"<script>alert(1)</script>"),
        ("components/bundle.css", b"a{}"),
        ("fonts/Acme.woff2", &[1, 2, 3]),
        ("assets/Logos/mark.svg", b"<svg/>"),
        ("assets/Logos/README.md", b"Logo notes"),
        ("api/tokens.md", b"generated"),
        ("tokens.css", b"generated"),
    ]
}

#[test]
fn reads_stored_and_deflated_archives_sorting_text_from_the_rest() {
    for store in [false, true] {
        let r = read_zip(&zip(&[("README.md", b"# Hi"), ("fonts/a.woff2", &[0]), ("dir/", b"")], store)).unwrap();
        assert_eq!(r.text.get("README.md").map(String::as_str), Some("# Hi"));
        assert_eq!(r.other, vec!["fonts/a.woff2"]);
    }
}

#[test]
fn turns_tokens_the_brand_book_and_component_guidelines_into_one_design_md() {
    let m = claude_system_to_design_md(&read_zip(&zip(&system(), false)).unwrap(), "file").unwrap();
    let md = &m.markdown;
    assert_eq!(m.title, "Acme");
    assert!(md.starts_with("# Acme\n\n> Sharp and blue.\n"));
    assert!(md.contains("| ink | #111111 | #eeeeee | Text |"));
    assert!(md.contains("| link | #111111 | #eeeeee | Links \\| anchors |"));
    assert!(md.contains("| paper | #ffffff | #ffffff |  |"));
    assert!(md.contains("| body | sans | 15px | 1.5 | 400 |  |  |"));
    assert!(md.contains("| space-4 | 16px | Padding |"));
    assert!(md.contains("light: 0 1px 2px #000; dark: none"));
    assert!(md.contains("### Colour"));
    assert!(md.contains("```md\n# not a heading"));
    assert!(md.contains("### Tone"));
    assert!(md.contains("### Button\n\n#### States\n\nPrimary only."));
    assert!(md.find("## Colour tokens").unwrap() < md.find("### Colour").unwrap());
    assert!(!md.contains("alert(1)"));
    assert!(!md.contains("generated"));
    assert_eq!(m.not_carried, vec!["1 component preview", "1 font file", "1 logo or image file", "1 component code file"]);
}

#[test]
fn finds_the_system_inside_a_wrapping_folder_or_project() {
    let mut files: Vec<(String, &[u8])> = system().into_iter().map(|(k, v)| (format!("Acme/project/{k}"), v)).collect();
    files.retain(|(k, _)| k != "Acme/project/components/Button/README.md");
    files.push(("Acme/project/components/Button/README.md".into(), b"# Button\n\nInside."));
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let m = claude_system_to_design_md(&read_zip(&zip(&refs, false)).unwrap(), "file").unwrap();
    assert!(m.markdown.contains("### Button\n\nInside."));
    assert!(m.markdown.contains("| ink |"));
}

#[test]
fn names_it_from_its_tokens_its_heading_or_the_file_in_that_order() {
    let name = |files: &[(&str, &[u8])]| claude_system_to_design_md(&read_zip(&zip(files, false)).unwrap(), "file-name").unwrap().title;
    assert_eq!(name(&[("tokens.json", TOKENS.as_bytes())]), "From tokens");
    assert_eq!(name(&[("README.md", b"# Heading\n\nText")]), "Heading");
    assert_eq!(name(&[("README.md", b"Just text")]), "file-name");
}

#[test]
fn refuses_what_is_not_a_design_system_with_a_reason() {
    let err = read_zip(b"not a zip, and long enough to look for an end record").unwrap_err();
    assert!(err.contains("not a .zip"), "{err}");
    let empty = read_zip(&zip(&[("photo.png", b"x"), ("notes.txt", b"y")], false)).unwrap();
    assert!(claude_system_to_design_md(&empty, "f").unwrap_err().contains("not a Claude design system"));
    let bad = read_zip(&zip(&[("tokens.json", b"{oops"), ("README.md", b"# A")], false)).unwrap();
    assert!(claude_system_to_design_md(&bad, "f").unwrap_err().contains("tokens.json is not valid JSON"));
}

#[test]
fn refuses_a_file_that_inflates_past_the_limit() {
    let big = vec![b'x'; 3_000_000];
    let err = read_zip(&zip(&[("README.md", &big)], false)).unwrap_err();
    assert!(err.contains("too large"), "{err}");
}
