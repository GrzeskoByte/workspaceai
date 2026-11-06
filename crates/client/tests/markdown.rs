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

fn link_of(md: &str) -> Option<String> {
    let b = parse(md).remove(0);
    inline(&b).styles.iter().find_map(|(_, s)| s.link.as_deref().map(str::to_string))
}

#[test]
fn only_http_https_and_mailto_links_keep_their_url() {
    assert_eq!(link_of("[a](https://x.example)").as_deref(), Some("https://x.example"));
    assert_eq!(link_of("[a](HTTP://x.example)").as_deref(), Some("HTTP://x.example"));
    assert_eq!(link_of("[a](mailto:me@x.example)").as_deref(), Some("mailto:me@x.example"));
    for bad in ["javascript:alert(1)", "JaVaScRiPt:alert(1)", " javascript:alert(1)", "file:///etc/passwd", "myapp://open", "data:text/html,x"] {
        assert_eq!(link_of(&format!("[a](<{bad}>)")), None, "{bad}");
    }
    // The text still shows without a link.
    assert_eq!(inline(&parse("[click](javascript:alert(1))")[0]).text, "click");
}

#[test]
fn a_disallowed_inner_link_does_not_inherit_the_outer_url() {
    let b = parse("[outer [inner](javascript:x) tail](https://ok.example)").remove(0);
    let i = inline(&b);
    let inner = i.text.find("inner").unwrap();
    assert!(!i.styles.iter().any(|(r, s)| r.contains(&inner) && s.link.is_some()), "{i:?}");
}

#[test]
fn very_long_urls_are_not_kept_per_range() {
    let url = format!("https://x.example/{}", "a".repeat(5000));
    assert_eq!(link_of(&format!("[a]({url})")), None);
}

#[test]
fn shorten_leaves_short_text_alone_and_cuts_long_text_on_a_char_boundary() {
    assert_eq!(shorten("short", 10), "short");
    let long = "ż".repeat(50);
    let cut = shorten(&long, 10);
    assert!(cut.starts_with(&"ż".repeat(10)) && cut.ends_with("(shortened)"), "{cut}");
    assert_eq!(cut.chars().filter(|c| *c == 'ż').count(), 10);
    assert_eq!(shorten("exactly10!", 10), "exactly10!");
}

#[test]
fn parse_caps_its_input() {
    let huge = "word ".repeat(MAX_INPUT); // far more than the cap
    let blocks = parse(&huge);
    let total: usize = blocks.iter().map(|b| match b { Block::Paragraph(i) => i.text.chars().count(), _ => 0 }).sum();
    assert!(total <= MAX_INPUT + 40, "parsed {total} characters");
    let last = match blocks.last().unwrap() { Block::Paragraph(i) => i.text.clone(), other => panic!("{other:?}") };
    assert!(last.ends_with("(shortened)"), "{last}");
}
