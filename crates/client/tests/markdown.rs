use workspaceai_client::markdown::*;

fn inline(b: &Block) -> &Inline {
    match b {
        Block::Paragraph(i) | Block::Quote(i) => i,
        Block::Heading { text, .. } | Block::ListItem { text, .. } => text,
        other => panic!("no inline text in {other:?}"),
    }
}

#[test]
fn bold_and_code_spans_are_byte_ranges_into_the_plain_text() {
    let b = &parse("Hello **bold** and `code`")[0];
    let i = inline(b);
    assert_eq!(i.text, "Hello bold and code");
    assert_eq!(i.styles.len(), 2);
    assert_eq!((&i.text[i.styles[0].0.clone()], i.styles[0].1.bold), ("bold", true));
    assert_eq!((&i.text[i.styles[1].0.clone()], i.styles[1].1.code), ("code", true));
}

#[test]
fn nested_emphasis_combines_styles() {
    let i = parse("***both***").remove(0);
    let i = inline(&i);
    assert!(i.styles.iter().any(|(_, s)| s.bold) && i.styles.iter().any(|(_, s)| s.italic));
}

#[test]
fn headings_keep_their_level() {
    match &parse("## Title")[0] {
        Block::Heading { level, text } => assert_eq!((*level, text.text.as_str()), (2, "Title")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn fenced_code_keeps_language_and_text() {
    match &parse("```rust\nfn x() {}\n```")[0] {
        Block::Code { lang, text } => assert_eq!((lang.as_str(), text.as_str()), ("rust", "fn x() {}\n")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn lists_are_flat_items_with_markers_and_depth() {
    let items: Vec<(String, usize, String)> = parse("- a\n- b\n  - c\n\n1. one\n2. two")
        .iter()
        .map(|b| match b {
            Block::ListItem { marker, depth, text } => (marker.clone(), *depth, text.text.clone()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        items,
        vec![
            ("•".to_string(), 0, "a".to_string()),
            ("•".to_string(), 0, "b".to_string()),
            ("•".to_string(), 1, "c".to_string()),
            ("1.".to_string(), 0, "one".to_string()),
            ("2.".to_string(), 0, "two".to_string()),
        ]
    );
}

#[test]
fn links_carry_their_url() {
    let i = parse("see [docs](https://a.example/x)").remove(0);
    let i = inline(&i);
    assert_eq!(i.text, "see docs");
    assert_eq!(i.styles[0].1.link.as_deref(), Some("https://a.example/x"));
}

#[test]
fn soft_line_breaks_stay_line_breaks_as_typed_in_chat() {
    let b = &parse("one\ntwo")[0];
    assert_eq!(inline(b).text, "one\ntwo");
}

#[test]
fn quote_and_rule() {
    let blocks = parse("> quoted\n\n---");
    assert!(matches!(&blocks[0], Block::Quote(i) if i.text == "quoted"));
    assert_eq!(blocks[1], Block::Rule);
}

#[test]
fn unicode_ranges_land_on_char_boundaries() {
    let b = &parse("Zażółć **gęślą** jaźń")[0];
    let i = inline(b);
    assert_eq!(&i.text[i.styles[0].0.clone()], "gęślą");
}

#[test]
fn empty_unterminated_and_plain_text_do_not_panic() {
    assert!(parse("").is_empty());
    assert_eq!(inline(&parse("**bold")[0]).text, "**bold");
    assert_eq!(inline(&parse("plain")[0]).styles.len(), 0);
    let _ = parse("```\nnever closed");
}
