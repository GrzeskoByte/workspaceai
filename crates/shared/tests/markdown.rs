//! Port of `packages/shared/test/markdown.test.ts`.
use workspaceai_shared::markdown::{gitlab_markdown, render_markdown};

fn md(s: &str) -> String {
    render_markdown(s, 1)
}

#[test]
fn lists_keeps_a_flat_list_as_it_was() {
    assert_eq!(md("- a\n- b"), "<ul><li>a</li><li>b</li></ul>");
    assert_eq!(md("1. a\n2. b"), "<ol><li>a</li><li>b</li></ol>");
}

#[test]
fn lists_nests_by_indent_and_comes_back_out() {
    assert_eq!(md("- a\n  - b\n    1. c\n- d"), "<ul><li>a<ul><li>b<ol><li>c</li></ol></li></ul></li><li>d</li></ul>");
}

#[test]
fn lists_does_not_end_on_a_blank_line_between_items_but_does_before_text() {
    assert_eq!(md("- a\n\n- b\n\nend"), "<ul><li>a</li><li>b</li></ul><p>end</p>");
}

#[test]
fn lists_shows_task_items_as_disabled_checkboxes() {
    assert_eq!(
        md("- [ ] a\n- [x] b"),
        "<ul><li class=\"task\"><input type=\"checkbox\" disabled> a</li><li class=\"task\"><input type=\"checkbox\" disabled checked> b</li></ul>"
    );
}

#[test]
fn lists_escapes_list_text() {
    assert!(md("- [ ] <img src=x>").contains("&lt;img"));
}

#[test]
fn links_links_a_bare_address_without_its_trailing_punctuation() {
    assert_eq!(
        md("See https://a.io/x?y=1&z=2."),
        "<p>See <a href=\"https://a.io/x?y=1&amp;z=2\" target=\"_blank\" rel=\"noreferrer noopener\">https://a.io/x?y=1&amp;z=2</a>.</p>"
    );
}

#[test]
fn links_leaves_a_written_link_alone() {
    assert_eq!(md("[a](https://a.io)"), "<p><a href=\"https://a.io\" target=\"_blank\" rel=\"noreferrer noopener\">a</a></p>");
}

#[test]
fn links_never_makes_a_link_of_another_scheme() {
    assert!(!md("[a](javascript:alert(1))").contains("<a "));
}

const URL: &str = "https://gl.io/g/app/-/issues/11";

#[test]
fn gitlab_markdown_drops_comments_and_raw_tags() {
    assert_eq!(gitlab_markdown("<!-- template -->\n<details><summary>More</summary>\nText</details>", URL), "More\nText");
}

#[test]
fn gitlab_markdown_turns_images_and_relative_links_into_links_to_gitlab() {
    assert_eq!(gitlab_markdown("![shot](/uploads/ab/s.png){width=10}", URL), "[shot](https://gl.io/g/app/uploads/ab/s.png){width=10}");
    assert_eq!(gitlab_markdown("![](https://i.io/a.png)", URL), "[Image](https://i.io/a.png)");
    assert_eq!(gitlab_markdown("[mr](/g/app/-/merge_requests/2)", URL), "[mr](https://gl.io/g/app/-/merge_requests/2)");
}

// Beyond the TS suite: the other block constructs and the shift.
#[test]
fn headings_shift_and_tables_render() {
    assert_eq!(render_markdown("# T", 1), "<h2>T</h2>");
    assert_eq!(render_markdown("# T", 0), "<h1>T</h1>");
    assert_eq!(md("| a | b |\n|---|---|\n| 1 | **2** |"), "<table><thead><tr><th>a</th><th>b</th></tr></thead><tbody><tr><td>1</td><td><b>2</b></td></tr></tbody></table>");
    assert_eq!(md("```\n<x>\n```\n> q\n---"), "<pre><code>&lt;x&gt;</code></pre><blockquote><p>q</p></blockquote><hr>");
}
