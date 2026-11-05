//! Draws `workspaceai_client::markdown` blocks with GPUI text.

use gpui::{div, prelude::*, px, rgb, Div, FontStyle, FontWeight, HighlightStyle, SharedString, StyledText, UnderlineStyle};
use workspaceai_client::markdown::{Block, Inline};

const CODE_BG: u32 = 0x2f2f36;
const LINK: u32 = 0xd97757;
const MUTED: u32 = 0x8b8b95;

fn styled(inline: &Inline) -> StyledText {
    let highlights = inline.styles.iter().map(|(range, s)| {
        let link = s.link.is_some();
        (
            range.clone(),
            HighlightStyle {
                font_weight: s.bold.then_some(FontWeight::BOLD),
                font_style: s.italic.then_some(FontStyle::Italic),
                background_color: s.code.then(|| rgb(CODE_BG).into()),
                color: link.then(|| rgb(LINK).into()),
                underline: link.then(|| UnderlineStyle { thickness: px(1.), color: None, wavy: false }),
                ..Default::default()
            },
        )
    });
    StyledText::new(SharedString::from(inline.text.clone())).with_highlights(highlights)
}

/// One message body: blocks stacked with a small gap.
pub fn blocks(blocks: &[Block]) -> Div {
    div().flex().flex_col().gap_2().children(blocks.iter().map(|b| match b {
        Block::Paragraph(i) => div().child(styled(i)),
        Block::Heading { level, text } => {
            let d = div().font_weight(FontWeight::BOLD);
            match level {
                1 => d.text_size(px(22.)),
                2 => d.text_size(px(19.)),
                _ => d.text_size(px(16.)),
            }
            .child(styled(text))
        }
        Block::Code { text, .. } => div().font_family("monospace").text_size(px(13.)).bg(rgb(CODE_BG)).rounded_md().p_2().child(SharedString::from(text.trim_end().to_string())),
        Block::ListItem { marker, depth, text } => div().flex().gap_2().pl(px(14. * *depth as f32)).child(div().text_color(rgb(MUTED)).child(SharedString::from(marker.clone()))).child(div().flex_1().min_w_0().child(styled(text))),
        Block::Quote(i) => div().pl_3().border_l_2().border_color(rgb(MUTED)).text_color(rgb(MUTED)).child(styled(i)),
        Block::Rule => div().h(px(1.)).bg(rgb(MUTED)),
    }))
}
