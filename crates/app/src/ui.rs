//! Small widgets shared by the settings pages.

use gpui::{div, prelude::*, px, rgb, Div, FontWeight, SharedString, Stateful};

pub const PANEL: u32 = 0x232329;
pub const MUTED: u32 = 0x8b8b95;
pub const ACCENT: u32 = 0xd97757;
pub const RAISED: u32 = 0x33333b;
pub const DANGER: u32 = 0x4a2b2b;

pub fn button(id: impl Into<SharedString>, label: impl Into<SharedString>, bg: u32) -> Stateful<Div> {
    div().id(id.into()).px_3().py_1().rounded_md().cursor_pointer().bg(rgb(bg)).text_sm().child(label.into())
}

/// A switch: a pill with a knob. `locked` dims it and the caller leaves out `on_click`.
pub fn switch(id: impl Into<SharedString>, on: bool, locked: bool) -> Stateful<Div> {
    let track = div().id(id.into()).w(px(40.)).h(px(22.)).rounded_full().p(px(2.)).flex().bg(rgb(if on { ACCENT } else { RAISED })).opacity(if locked { 0.5 } else { 1.0 });
    let track = if on { track.justify_end() } else { track };
    let track = if locked { track } else { track.cursor_pointer() };
    track.child(div().size(px(18.)).rounded_full().bg(rgb(0xffffff)))
}

/// A settings row: label and note on the left, the control on the right.
pub fn row(label: &str, note: &str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .p_3()
        .rounded_lg()
        .bg(rgb(PANEL))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(div().font_weight(FontWeight::MEDIUM).child(SharedString::from(label.to_string())))
                .when(!note.is_empty(), |d| d.child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(note.to_string())))),
        )
        .child(control)
}

pub fn page_title(title: &str, lead: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_size(px(26.)).font_weight(FontWeight::BOLD).child(SharedString::from(title.to_string())))
        .when(!lead.is_empty(), |d| d.child(div().text_color(rgb(MUTED)).child(SharedString::from(lead.to_string()))))
}
