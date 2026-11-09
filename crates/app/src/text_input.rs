// Adapted from Zed's `crates/gpui/examples/input.rs` (Apache-2.0, Copyright Zed Industries, Inc.).
// Changes: public constructor and accessors, an Enter action that emits `TextInputEvent::Submit`, platform key
// bindings, and app styling.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
    actions, div, fill, hsla, point, prelude::*, px, relative,
    rgb, rgba, size, white,
};
use unicode_segmentation::*;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
        Enter,
        Up,
        Down,
    ]
);

/// Shortest text area, in lines.
const MIN_LINES: usize = 6;

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    /// Draw bullets instead of the text, and refuse to copy or cut it (API keys).
    masked: bool,
    /// Enter inserts a line break; the field grows with its text and Up/Down move between lines.
    multiline: bool,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    /// Multi-line only: (byte offset where the line starts, shaped line).
    last_lines: Vec<(usize, ShapedLine)>,
    last_bounds: Option<Bounds<Pixels>>,
    line_height: Pixels,
    is_selecting: bool,
}

impl TextInput {
    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), window, cx);
        } else {
            self.move_to(self.selected_range.start, window, cx)
        }
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), window, cx);
        } else {
            self.move_to(self.selected_range.end, window, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.content.len(), window, cx)
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.move_to(lines::vertical(&self.content, self.cursor_offset(), false), window, cx);
        }
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.move_to(lines::vertical(&self.content, self.cursor_offset(), true), window, cx);
        }
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        let to = if self.multiline { lines::start(&self.content, self.cursor_offset()) } else { 0 };
        self.move_to(to, window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        let to = if self.multiline { lines::end(&self.content, self.cursor_offset()) } else { self.content.len() };
        self.move_to(to, window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == prev {
                window.play_system_bell();
                return;
            }
            self.select_to(prev, window, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                window.play_system_bell();
                return;
            }
            self.select_to(next, window, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        window.request_virtual_keyboard();

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), window, cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), window, cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), window, cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = if self.multiline { text } else { text.replace("\n", " ") };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        // A masked field never puts its text on the clipboard.
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        if self.multiline {
            let (Some(bounds), false) = (self.last_bounds.as_ref(), self.last_lines.is_empty()) else {
                return 0;
            };
            let row = (((position.y - bounds.top()) / self.line_height).floor().max(0.) as usize).min(self.last_lines.len() - 1);
            let (start, line) = &self.last_lines[row];
            return start + line.closest_index_for_x(position.x - bounds.left());
        }

        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        let index = line.closest_index_for_x(position.x - bounds.left());
        if self.masked {
            mask::from_display(&self.content, index)
        } else {
            index
        }
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn reset(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.content = "".into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.last_bounds = None;
        self.is_selecting = false;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_range = self.range_from_utf16(&range_utf16);
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.content.encode_utf16().count())
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.selection_reversed = false;
        self.marked_range.take();
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        let insertion_start_utf16 = self.offset_to_utf16(range.start);
        let inserted_length_utf16 = new_text.encode_utf16().count();
        self.selected_range = new_selected_range_utf16
            .map(|selected| {
                self.range_from_utf16(
                    &(insertion_start_utf16 + selected.start.min(inserted_length_utf16)
                        ..insertion_start_utf16 + selected.end.min(inserted_length_utf16)),
                )
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;

        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        if last_layout.text != self.content {
            return None;
        }
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range_utf16, bounds, window, cx)
            .into_iter()
            .collect()
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;

        if last_layout.text != self.content {
            return None;
        }
        let utf8_index = last_layout.closest_index_for_x(line_point.x);
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    lines: Vec<(usize, ShapedLine)>,
    extra: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let lines = self.input.read(cx).multiline.then(|| self.input.read(cx).content.split('\n').count().max(MIN_LINES));
        style.size.height = (window.line_height() * lines.unwrap_or(1) as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let masked = input.masked && !content.is_empty();
        let (display_text, text_color): (SharedString, _) = if content.is_empty() {
            (input.placeholder.clone(), hsla(0., 0., 0.55, 0.8))
        } else if masked {
            (mask::masked_text(&content).into(), style.color)
        } else {
            (content.clone(), style.color)
        };
        let shown = |offset: usize| if masked { mask::to_display(&content, offset) } else { offset };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let (false, Some(marked_range)) = (masked, input.marked_range.as_ref()) {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        if input.multiline && !content.is_empty() {
            // One shaped line per text line; the marked (IME) underline is not drawn here.
            let lh = window.line_height();
            let (mut shaped, mut quads, mut caret, mut start) = (Vec::new(), Vec::new(), None, 0usize);
            for (row, text) in content.split('\n').enumerate() {
                let end = start + text.len();
                let run = TextRun { len: text.len(), ..runs[0].clone() };
                let line = window.text_system().shape_line(SharedString::from(text.to_string()), font_size, &[run].into_iter().filter(|r| r.len > 0).collect::<Vec<_>>(), None);
                let top = bounds.top() + lh * row as f32;
                if selected_range.is_empty() {
                    if cursor >= start && cursor <= end {
                        caret = Some(fill(Bounds::new(point(bounds.left() + line.x_for_index(cursor - start), top), size(px(2.), lh)), rgb(0xd97757)));
                    }
                } else if selected_range.start <= end && selected_range.end >= start {
                    let (a, b) = (selected_range.start.max(start) - start, selected_range.end.min(end) - start);
                    quads.push(fill(Bounds::from_corners(point(bounds.left() + line.x_for_index(a), top), point(bounds.left() + line.x_for_index(b) + if selected_range.end > end { px(6.) } else { px(0.) }, top + lh)), rgba(0x3311ff30)));
                }
                shaped.push((start, line));
                start = end + 1;
            }
            return PrepaintState { line: None, lines: shaped, extra: quads, cursor: caret, selection: None };
        }
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(shown(cursor));
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    rgb(0xd97757),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(shown(selected_range.start)),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(shown(selected_range.end)),
                            bounds.bottom(),
                        ),
                    ),
                    rgba(0x3311ff30),
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            lines: Vec::new(),
            extra: Vec::new(),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if !prepaint.lines.is_empty() || !prepaint.extra.is_empty() {
            let lh = window.line_height();
            for q in prepaint.extra.drain(..) {
                window.paint_quad(q);
            }
            for (row, (_, line)) in prepaint.lines.iter().enumerate() {
                let _ = line.paint(point(bounds.left(), bounds.top() + lh * row as f32), lh, gpui::TextAlign::Left, None, window, cx);
            }
            if let (true, Some(cursor)) = (focus_handle.is_focused(window), prepaint.cursor.take()) {
                window.paint_quad(cursor);
            }
            let lines = std::mem::take(&mut prepaint.lines);
            self.input.update(cx, |input, _cx| {
                input.last_lines = lines;
                input.last_bounds = Some(bounds);
                input.line_height = lh;
            });
            return;
        }
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        line.paint(
            bounds.origin,
            window.line_height(),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        )
        .unwrap();

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .bg(rgb(0xeeeeee))
            .line_height(px(30.))
            .text_size(px(24.))
            .child(
                div()
                    .when(!self.multiline, |d| d.h(px(30. + 4. * 2.)))
                    .when(self.multiline, |d| d.min_h(px(30. * MIN_LINES as f32 + 4. * 2.)))
                    .w_full()
                    .p(px(4.))
                    .bg(white())
                    .child(TextElement { input: cx.entity() }),
            )
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputEvent {
    Submit,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    pub fn new(cx: &mut Context<Self>, placeholder: &str) -> Self {
        TextInput {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.to_string().into(),
            masked: false,
            multiline: false,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_lines: Vec::new(),
            line_height: px(30.),
            last_bounds: None,
            is_selecting: false,
        }
    }

    /// A field that shows bullets and cannot be copied from, for secrets.
    pub fn new_masked(cx: &mut Context<Self>, placeholder: &str) -> Self {
        TextInput { masked: true, ..TextInput::new(cx, placeholder) }
    }

    /// A text area: Enter adds a line break (nothing is submitted), and it is at least `MIN_LINES` tall.
    pub fn new_multiline(cx: &mut Context<Self>, placeholder: &str) -> Self {
        TextInput { multiline: true, ..TextInput::new(cx, placeholder) }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    /// Replaces the text and puts the caret at the end.
    pub fn set_content(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = text.to_string().into();
        self.selected_range = text.len()..text.len();
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reset(window, cx);
    }

    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }
        cx.emit(TextInputEvent::Submit);
    }
}

// The shortcut modifier is Cmd on macOS and Ctrl elsewhere. GPUI has no `secondary-` modifier at this revision.
const SELECT_ALL: &str = if cfg!(target_os = "macos") { "cmd-a" } else { "ctrl-a" };
const PASTE: &str = if cfg!(target_os = "macos") { "cmd-v" } else { "ctrl-v" };
const COPY: &str = if cfg!(target_os = "macos") { "cmd-c" } else { "ctrl-c" };
const CUT: &str = if cfg!(target_os = "macos") { "cmd-x" } else { "ctrl-x" };

/// Masked fields (API keys) draw one bullet per character; these map offsets between the real text and the bullets.
pub mod mask {
    const BULLET: &str = "•";

    pub fn masked_text(content: &str) -> String {
        BULLET.repeat(content.chars().count())
    }

    /// Byte offset in the real text → byte offset in the bullets.
    pub fn to_display(content: &str, offset: usize) -> usize {
        content.get(..offset).map_or(0, |head| head.chars().count()) * BULLET.len()
    }

    /// Byte offset in the bullets → byte offset of the nearest character at or before it in the real text.
    pub fn from_display(content: &str, display: usize) -> usize {
        let n = display / BULLET.len();
        content.char_indices().nth(n).map_or(content.len(), |(i, _)| i)
    }
}

/// Key bindings for every `TextInput`.
pub fn bindings() -> Vec<KeyBinding> {
    let ctx = Some("TextInput");
    vec![
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new(SELECT_ALL, SelectAll, ctx),
        KeyBinding::new(PASTE, Paste, ctx),
        KeyBinding::new(COPY, Copy, ctx),
        KeyBinding::new(CUT, Cut, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("enter", Enter, ctx),
        KeyBinding::new("up", Up, ctx),
        KeyBinding::new("down", Down, ctx),
    ]
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys(bindings());
}

#[cfg(test)]
mod tests {
    use super::mask::*;

    #[test]
    fn masked_text_is_one_bullet_per_character() {
        assert_eq!(masked_text("abc"), "•••");
        assert_eq!(masked_text("żółw"), "••••");
        assert_eq!(masked_text(""), "");
    }

    #[test]
    fn offsets_map_between_the_real_text_and_the_bullets() {
        // Each bullet is 3 bytes; "żółw" is 4 characters in 7 bytes.
        assert_eq!(to_display("abc", 2), 6);
        assert_eq!(to_display("żółw", 4), 6);
        assert_eq!(to_display("żółw", 7), 12);
        assert_eq!(from_display("żółw", 6), 4);
        assert_eq!(from_display("żółw", 7), 4, "inside a bullet rounds down to a character");
        assert_eq!(from_display("abc", 100), 3, "past the end is the end");
        assert_eq!(from_display("", 3), 0);
    }

    #[test]
    fn every_binding_is_a_valid_keystroke() {
        // KeyBinding::new panics on a keystroke GPUI cannot parse, which used to kill the app at startup.
        assert_eq!(super::bindings().len(), 15);
    }
}

/// Line geometry for multi-line fields, in byte offsets.
pub mod lines {
    /// Start of the line holding `offset`.
    pub fn start(text: &str, offset: usize) -> usize {
        text[..offset.min(text.len())].rfind('\n').map_or(0, |i| i + 1)
    }

    /// End of the line holding `offset` (before its line break).
    pub fn end(text: &str, offset: usize) -> usize {
        let o = offset.min(text.len());
        text[o..].find('\n').map_or(text.len(), |i| o + i)
    }

    /// One line up or down, keeping the character column where the line is long enough.
    pub fn vertical(text: &str, offset: usize, down: bool) -> usize {
        let o = offset.min(text.len());
        let s = start(text, o);
        let col = text[s..o].chars().count();
        let target = if down {
            let e = end(text, o);
            if e >= text.len() {
                return text.len();
            }
            e + 1
        } else {
            if s == 0 {
                return 0;
            }
            start(text, s - 1)
        };
        let t_end = end(text, target);
        text[target..t_end].char_indices().nth(col).map_or(t_end, |(i, _)| target + i)
    }
}

#[cfg(test)]
mod line_tests {
    use super::lines::*;

    #[test]
    fn vertical_keeps_the_column() {
        let t = "abc\nde\nfghij";
        assert_eq!(vertical(t, 2, true), 6); // "de" ends at 6 (col 2 clamps to its end)
        assert_eq!(vertical(t, 6, true), 9); // col 2 of "fghij"
        assert_eq!(vertical(t, 10, false), 6);
        assert_eq!(vertical(t, 1, false), 0);
        assert_eq!(vertical(t, 12, true), t.len());
    }

    #[test]
    fn line_bounds() {
        let t = "ab\ncd";
        assert_eq!((start(t, 4), end(t, 4), start(t, 1), end(t, 1)), (3, 5, 0, 2));
    }
}
