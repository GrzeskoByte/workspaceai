//! A GPUI view over a `workspaceai_term::TermSession`: paints the grid, sends keys/IME text/paste/mouse
//! reports to the PTY, keeps the grid sized to the element, and reports bell/title/exit/link clicks.
//!
//! Key routing: GPUI matches key bindings before `on_key_down`, so any app shortcut (Ctrl+K palette, …)
//! wins without help from here. Keys with an xterm encoding (Ctrl/Alt combos, arrows, F-keys, Enter…) are
//! encoded in `on_key_down`; plain printable text is left to the platform, which delivers it (and IME
//! commits, dead keys, compose) through `EntityInputHandler::replace_text_in_range`.
//! The pure parts (colour/run grouping, key mapping, cell geometry, mouse reports, link detection,
//! status) are free functions with unit tests at the bottom.

use std::ops::Range;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use gpui::{
    actions, div, fill, font, outline, point, prelude::*, px, relative, size, App, Bounds, BorderStyle, ClipboardItem, Context, CursorStyle,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, FontStyle, FontWeight, GlobalElementId,
    Hsla, KeyBinding, KeyDownEvent, Keystroke, LayoutId, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoAction,
    PaintQuad, Pixels, ScrollEvent, ShapedLine, SharedString, StrikethroughStyle, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use workspaceai_term::{
    default_rgb, encode_key, Cell, Color, CursorShape, Flags, Key, Mods, NamedColor, Rgb, Scroll, SelectionType, Side, Size,
    SpawnOptions, TermEvent, TermMode, TermSession,
};

actions!(terminal, [TermCopy, TermPaste]);

/// Selection highlight; the rest of the palette is `default_rgb` (same table the PTY answers OSC 4/10/11 with).
const SELECTION: Rgb = Rgb { r: 0x26, g: 0x4f, b: 0x78 };
const FONT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 1.3;
/// Output within this window counts as "working".
const WORKING_FOR: Duration = Duration::from_secs(2);

pub fn bind_keys(cx: &mut App) {
    let (copy, paste) = if cfg!(target_os = "macos") { ("cmd-c", "cmd-v") } else { ("ctrl-shift-c", "ctrl-shift-v") };
    cx.bind_keys([
        KeyBinding::new(copy, TermCopy, Some("Terminal")),
        KeyBinding::new(paste, TermPaste, Some("Terminal")),
        // The app binds a global Escape (close palette); inside the terminal Escape belongs to the PTY.
        KeyBinding::new("escape", NoAction, Some("Terminal")),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Working,
    NeedsYou,
    Idle,
    Exited(Option<i32>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermViewEvent {
    /// The status dot changed (bell, output started/stopped, exit).
    Status(Status),
    Title(Option<String>),
    /// Ctrl/Cmd+click on a link; the receiver picks browser panel (local address) or system browser.
    OpenLink(String),
}

const CLIPBOARD_MAX: usize = 100_000;
const CLIPBOARD_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

/// Last laid-out grid geometry, in window coordinates.
#[derive(Clone, Copy, Default)]
struct Geom {
    origin: gpui::Point<Pixels>,
    cell_w: f32,
    cell_h: f32,
    cols: usize,
    rows: usize,
}

struct HoverLink {
    row: usize,
    cols: Range<usize>,
    url: String,
}

pub struct TerminalView {
    session: Option<TermSession>,
    events: Option<Receiver<TermEvent>>,
    error: Option<String>,
    focus: FocusHandle,
    geom: Geom,
    /// IME preedit text, drawn at the cursor and never sent.
    marked: Option<String>,
    title: Option<String>,
    bell: bool,
    /// When the user last typed or pasted here: OSC 52 clipboard writes are only honoured shortly after.
    last_input: Option<Instant>,
    /// Size of the last OSC 52 write that was applied or refused, for the app to show (`take_clipboard_notice`).
    clipboard_notice: Option<(usize, bool)>,
    last_output: Option<Instant>,
    exit: Option<Option<i32>>,
    status: Status,
    hover: Option<HoverLink>,
    /// Button code of a press reported to the app (mouse mode), so the release is reported too.
    reported: Option<u8>,
    last_motion: Option<(usize, usize)>,
    selecting: bool,
    /// Fractional wheel lines carried between events (touchpads send small deltas).
    wheel: f32,
}

impl EventEmitter<TermViewEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TerminalView {
    pub fn new(opts: SpawnOptions, cx: &mut Context<Self>) -> Self {
        let (session, events, error) = match TermSession::spawn(opts) {
            Ok((s, rx)) => (Some(s), Some(rx), None),
            Err(e) => (None, None, Some(format!("Could not start the terminal: {e}"))),
        };
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(16)).await;
            if this.update(cx, |v, cx| v.pump(cx)).is_err() {
                break;
            }
        })
        .detach();
        TerminalView {
            session,
            events,
            error,
            focus: cx.focus_handle(),
            geom: Geom::default(),
            marked: None,
            title: None,
            bell: false,
            last_input: None,
            clipboard_notice: None,
            last_output: None,
            exit: None,
            status: Status::Idle,
            hover: None,
            reported: None,
            last_motion: None,
            selecting: false,
            wheel: 0.,
        }
    }

    /// `(bytes, applied)` for the last OSC 52 clipboard request, once; the app shows "copied" or "refused".
    pub fn take_clipboard_notice(&mut self) -> Option<(usize, bool)> {
        self.clipboard_notice.take()
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Drains PTY events: one repaint per frame however many wakeups arrived.
    fn pump(&mut self, cx: &mut Context<Self>) {
        let mut dirty = false;
        while let Some(e) = self.events.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match e {
                TermEvent::Wakeup => {
                    dirty = true;
                    self.last_output = Some(Instant::now());
                }
                TermEvent::Bell => self.bell = true,
                TermEvent::Title(t) => {
                    self.title = t.clone();
                    cx.emit(TermViewEvent::Title(t));
                    dirty = true;
                }
                // OSC 52: whatever is printed in a terminal (a `cat` of a hostile file) must not silently replace the
                // clipboard. Only shortly after the user's own input, capped in size, and always reported.
                TermEvent::ClipboardStore(s) => {
                    let allowed = s.len() <= CLIPBOARD_MAX && self.last_input.is_some_and(|t| t.elapsed() < CLIPBOARD_WINDOW);
                    self.clipboard_notice = Some((s.len(), allowed));
                    if allowed {
                        cx.write_to_clipboard(ClipboardItem::new_string(s));
                    }
                    dirty = true;
                }
                TermEvent::Exit(code) => {
                    self.exit = Some(code);
                    dirty = true;
                }
            }
        }
        let s = status(self.exit, self.bell, self.last_output, Instant::now());
        if s != self.status {
            self.status = s;
            cx.emit(TermViewEvent::Status(s));
            dirty = true;
        }
        if dirty {
            cx.notify();
        }
    }

    /// The user acted: clear "needs you" and jump back to the live screen.
    fn user_input(&mut self) {
        self.bell = false;
        self.last_input = Some(Instant::now());
        if let Some(s) = &self.session {
            s.scroll(Scroll::Bottom);
        }
    }

    fn write(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        self.user_input();
        if let Some(s) = &self.session {
            s.write(bytes);
        }
        cx.notify();
    }

    fn mode(&self) -> TermMode {
        self.session.as_ref().map_or(TermMode::empty(), |s| s.mode())
    }

    fn on_key_down(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let m = k.modifiers;
        // Shift+PageUp/Down/Home/End scroll the scrollback locally.
        if m.shift && !m.control && !m.alt && !m.platform {
            let scroll = match k.key.as_str() {
                "pageup" => Some(Scroll::PageUp),
                "pagedown" => Some(Scroll::PageDown),
                "home" => Some(Scroll::Top),
                "end" => Some(Scroll::Bottom),
                _ => None,
            };
            if let (Some(sc), Some(s)) = (scroll, &self.session) {
                s.scroll(sc);
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        let Some((key, mods)) = key_of(k) else { return };
        if let Some(bytes) = encode_key(key, mods, self.mode()) {
            cx.stop_propagation();
            self.write(bytes, cx);
        }
    }

    fn copy(&mut self, _: &TermCopy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.session.as_ref().and_then(|s| s.selection_text()).filter(|t| !t.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, _: &TermPaste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            self.paste_text(&text, cx);
        }
    }

    /// Pastes text as the user would (bracketed when the app asked for it). B8 uses this for dropped paths.
    pub fn paste_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.user_input();
        if let Some(s) = &self.session {
            s.paste(text);
        }
        cx.notify();
    }

    fn cell(&self, pos: gpui::Point<Pixels>) -> (usize, usize, Side) {
        let g = self.geom;
        cell_at(f32::from(pos.x - g.origin.x), f32::from(pos.y - g.origin.y), g.cell_w, g.cell_h, g.cols, g.rows)
    }

    /// Mouse events go to the app when it enabled mouse mode, unless Shift is held (local selection).
    fn reports_mouse(&self, m: &Modifiers) -> bool {
        self.mode().intersects(TermMode::MOUSE_MODE) && !m.shift
    }

    fn report(&mut self, code: u8, row: usize, col: usize, press: bool, m: &Modifiers, cx: &mut Context<Self>) {
        if let Some(b) = mouse_report(code, row, col, press, mods_of(m), self.mode()) {
            if let Some(s) = &self.session {
                s.write(b);
            }
            cx.notify();
        }
    }

    fn on_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let (row, col, side) = self.cell(e.position);
        if e.button == MouseButton::Left && (e.modifiers.control || e.modifiers.platform) {
            if let Some(url) = self.link_at(row, col).map(|l| l.url) {
                cx.emit(TermViewEvent::OpenLink(url));
                return;
            }
        }
        if self.reports_mouse(&e.modifiers) {
            let code = match e.button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                _ => return,
            };
            self.reported = Some(code);
            self.report(code, row, col, true, &e.modifiers, cx);
            return;
        }
        if e.button != MouseButton::Left {
            return;
        }
        let Some(s) = &self.session else { return };
        if e.modifiers.shift && e.click_count == 1 && s.selection_text().is_some() {
            s.select_update(row, col, side);
        } else {
            let ty = match e.click_count {
                2 => SelectionType::Semantic,
                n if n >= 3 => SelectionType::Lines,
                _ => SelectionType::Simple,
            };
            s.select_start(ty, row, col, side);
        }
        self.selecting = true;
        cx.notify();
    }

    fn on_mouse_up(&mut self, e: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (row, col, _) = self.cell(e.position);
        if let Some(code) = self.reported.take() {
            self.report(code, row, col, false, &e.modifiers, cx);
        }
        self.selecting = false;
    }

    fn on_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (row, col, side) = self.cell(e.position);
        let hover = (e.modifiers.control || e.modifiers.platform).then(|| self.link_at(row, col)).flatten();
        if hover.as_ref().map(|h| (h.row, &h.cols)) != self.hover.as_ref().map(|h| (h.row, &h.cols)) {
            self.hover = hover;
            cx.notify();
        }
        if self.selecting {
            if let Some(s) = &self.session {
                s.select_update(row, col, side);
                cx.notify();
            }
            return;
        }
        let mode = self.mode();
        if !self.reports_mouse(&e.modifiers) || self.last_motion == Some((row, col)) {
            return;
        }
        let code = match (self.reported, e.pressed_button) {
            (Some(b), Some(_)) if mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) => b + 32,
            (None, None) if mode.contains(TermMode::MOUSE_MOTION) => 3 + 32,
            _ => return,
        };
        self.last_motion = Some((row, col));
        self.report(code, row, col, true, &e.modifiers, cx);
    }

    fn on_scroll(&mut self, e: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let cell_h = self.geom.cell_h.max(1.);
        self.wheel += f32::from(e.delta.pixel_delta(px(cell_h)).y) / cell_h;
        let lines = self.wheel.trunc() as i32;
        if lines == 0 {
            return;
        }
        self.wheel -= lines as f32;
        let mode = self.mode();
        let (row, col, _) = self.cell(e.position);
        if self.reports_mouse(&e.modifiers) {
            let code = if lines > 0 { 64 } else { 65 };
            for _ in 0..lines.unsigned_abs() {
                self.report(code, row, col, true, &e.modifiers, cx);
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            // Pagers and editors without mouse mode: the wheel becomes arrow keys.
            let key = if lines > 0 { Key::Up } else { Key::Down };
            let bytes: Vec<u8> = (0..lines.unsigned_abs()).filter_map(|_| encode_key(key, Mods::NONE, mode)).flatten().collect();
            if let Some(s) = &self.session {
                s.write(bytes);
            }
        } else if let Some(s) = &self.session {
            s.scroll(Scroll::Delta(lines));
            cx.notify();
        }
    }

    /// OSC 8 hyperlink under the cell, else a URL / `localhost:port` found in the row text.
    fn link_at(&self, row: usize, col: usize) -> Option<HoverLink> {
        let snap = self.session.as_ref()?.snapshot();
        if row >= snap.rows || col >= snap.cols {
            return None;
        }
        if let Some(link) = snap.cell(row, col).hyperlink().filter(|l| is_web(l.uri())) {
            let same = |c: usize| snap.cell(row, c).hyperlink().is_some_and(|h| h == link);
            let start = (0..=col).rev().take_while(|&c| same(c)).last().unwrap_or(col);
            let end = (col..snap.cols).take_while(|&c| same(c)).last().unwrap_or(col) + 1;
            return Some(HoverLink { row, cols: start..end, url: link.uri().to_string() });
        }
        let line: Vec<char> = (0..snap.cols).map(|c| row_char(snap.cell(row, c))).collect();
        link_at(&line, col).map(|(cols, url)| HoverLink { row, cols, url })
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(&mut self, range: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let m = self.marked.clone().unwrap_or_default();
        let utf16: Vec<u16> = m.encode_utf16().collect();
        let r = range.start.min(utf16.len())..range.end.min(utf16.len());
        *actual = Some(r.clone());
        Some(String::from_utf16_lossy(&utf16[r]))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let n = self.marked.as_ref().map_or(0, |m| m.encode_utf16().count());
        Some(UTF16Selection { range: n..n, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|m| 0..m.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    /// Committed text (typed characters, IME commits): straight to the PTY.
    fn replace_text_in_range(&mut self, _: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        if !text.is_empty() {
            self.write(text.as_bytes().to_vec(), cx);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(&mut self, _: Option<Range<usize>>, text: &str, _: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    /// Where the IME candidate window goes: the cursor cell.
    fn bounds_for_range(&mut self, _: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let g = self.geom;
        let c = self.session.as_ref()?.snapshot().cursor?;
        Some(Bounds::new(point(g.origin.x + px(c.column.0 as f32 * g.cell_w), g.origin.y + px(c.line as f32 * g.cell_h)), size(px(g.cell_w), px(g.cell_h))))
    }

    fn character_index_for_point(&mut self, _: gpui::Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = hsla(default_rgb(257));
        if let Some(err) = &self.error {
            return div().size_full().bg(bg).p_4().text_color(hsla(default_rgb(256))).child(SharedString::from(err.clone())).into_any_element();
        }
        div()
            .id("terminal")
            .key_context("Terminal")
            .track_focus(&self.focus)
            .size_full()
            .bg(bg)
            .cursor(if self.hover.is_some() { CursorStyle::PointingHand } else { CursorStyle::IBeam })
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll(cx.listener(Self::on_scroll))
            .child(TermElement { view: cx.entity() })
            .into_any_element()
    }
}

struct TermElement {
    view: Entity<TerminalView>,
}

struct Prepainted {
    quads: Vec<PaintQuad>,
    lines: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    /// Painted after the lines (cursor, IME underline background, link underline).
    over: Vec<PaintQuad>,
    over_lines: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    cell_h: Pixels,
}

impl IntoElement for TermElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TermElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> Prepainted {
        let base = font("monospace");
        let font_size = px(FONT_SIZE);
        let ts = window.text_system().clone();
        let font_id = ts.resolve_font(&base);
        let cell_w = ts.advance(font_id, font_size, 'm').map(|s| f32::from(s.width)).unwrap_or(FONT_SIZE * 0.6).max(1.);
        let cell_h = (FONT_SIZE * LINE_HEIGHT).round();
        let (cols, rows) = grid_size(f32::from(bounds.size.width), f32::from(bounds.size.height), cell_w, cell_h);
        let focused = self.view.read(cx).focus.is_focused(window);

        let (snap, marked, hover) = self.view.update(cx, |v, _| {
            v.geom = Geom { origin: bounds.origin, cell_w, cell_h, cols, rows };
            let s = v.session.as_ref()?;
            let want = Size { cols: cols as u16, rows: rows as u16, cell_width: cell_w.round() as u16, cell_height: cell_h as u16 };
            if s.size() != want {
                s.resize(want);
            }
            Some((s.snapshot(), v.marked.clone(), v.hover.as_ref().map(|h| (h.row, h.cols.clone()))))
        })
        .map_or((None, None, None), |(s, m, h)| (Some(s), m, h));
        let mut out = Prepainted { quads: Vec::new(), lines: Vec::new(), over: Vec::new(), over_lines: Vec::new(), cell_h: px(cell_h) };
        let Some(snap) = snap else { return out };

        let at = |row: usize, col: usize| point(bounds.origin.x + px(col as f32 * cell_w), bounds.origin.y + px(row as f32 * cell_h));
        let cell_rect = |row: usize, col: usize, n: usize| Bounds::new(at(row, col), size(px(n as f32 * cell_w), px(cell_h)));
        let shape = |text: &str, st: &CellStyle, color: Rgb| {
            let mut f = base.clone();
            if st.bold {
                f.weight = FontWeight::BOLD;
            }
            if st.italic {
                f.style = FontStyle::Italic;
            }
            let c = hsla(color);
            let run = TextRun {
                len: text.len(),
                font: f,
                color: c,
                background_color: None,
                underline: st.underline.then(|| UnderlineStyle { color: Some(c), thickness: px(1.), wavy: false }),
                strikethrough: st.strike.then(|| StrikethroughStyle { thickness: px(1.), color: Some(c) }),
            };
            ts.shape_line(SharedString::from(text.to_string()), font_size, &[run], None)
        };

        let default_bg = default_rgb(257);
        let mut cursor_run = None;
        for row in 0..snap.rows.min(rows) {
            let cells = &snap.cells[row * snap.cols..(row + 1) * snap.cols];
            let runs = row_runs(cells, |c| snap.is_selected(row, c));
            for (col, n, bg) in backgrounds(&runs, default_bg) {
                out.quads.push(fill(cell_rect(row, col, n), hsla(bg)));
            }
            for r in &runs {
                if snap.cursor.is_some_and(|c| c.line == row && (r.col..r.col + r.cols).contains(&c.column.0)) {
                    cursor_run = Some(r.clone());
                }
                if r.text.trim().is_empty() && !r.style.underline && !r.style.strike {
                    continue;
                }
                out.lines.push((at(row, r.col), shape(&r.text, &r.style, r.style.fg)));
            }
        }

        if let Some((row, cols)) = hover {
            out.over.push(fill(Bounds::new(at(row, cols.start) + point(px(0.), px(cell_h - 1.)), size(px(cols.len() as f32 * cell_w), px(1.))), hsla(default_rgb(256))));
        }

        let cursor_on = snap.mode.contains(TermMode::SHOW_CURSOR) && snap.cursor_shape != CursorShape::Hidden;
        if let Some(c) = snap.cursor.filter(|_| cursor_on || marked.is_some()) {
            let (row, col) = (c.line, c.column.0);
            let color = hsla(default_rgb(258));
            if let Some(m) = &marked {
                // IME preedit: drawn over the cursor cell, underlined, never sent.
                let n = m.chars().count().max(1);
                out.over.push(fill(cell_rect(row, col, n), hsla(default_bg)));
                let st = CellStyle { fg: default_rgb(256), bg: default_bg, bold: false, italic: false, underline: true, strike: false };
                out.over_lines.push((at(row, col), shape(m, &st, st.fg)));
            } else {
                let width = cursor_run.as_ref().filter(|r| r.col == col).map_or(1, |r| r.cols.min(2));
                let rect = cell_rect(row, col, width);
                match (snap.cursor_shape, focused) {
                    (CursorShape::Block, true) => {
                        out.over.push(fill(rect, color));
                        if let Some(r) = cursor_run.filter(|r| !r.text.trim().is_empty()) {
                            // Redraw the character under the block in the background colour.
                            let ch: String = if r.cols == 2 || !r.text.is_ascii() { r.text.clone() } else { r.text.chars().nth(col - r.col).into_iter().collect() };
                            let x = if r.cols == 2 || !r.text.is_ascii() { r.col } else { col };
                            out.over_lines.push((at(row, x), shape(&ch, &r.style, r.style.bg)));
                        }
                    }
                    (CursorShape::Beam, _) => out.over.push(fill(Bounds::new(rect.origin, size(px(2.), rect.size.height)), color)),
                    (CursorShape::Underline, _) => out.over.push(fill(Bounds::new(rect.origin + point(px(0.), px(cell_h - 2.)), size(rect.size.width, px(2.))), color)),
                    _ => out.over.push(outline(rect, color, BorderStyle::Solid)),
                }
            }
        }
        out
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), p: &mut Prepainted, window: &mut Window, cx: &mut App) {
        let focus = self.view.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
        window.paint_layer(bounds, |window| {
            for q in p.quads.drain(..) {
                window.paint_quad(q);
            }
            for (at, line) in &p.lines {
                let _ = line.paint(*at, p.cell_h, gpui::TextAlign::Left, None, window, cx);
            }
            for q in p.over.drain(..) {
                window.paint_quad(q);
            }
            for (at, line) in &p.over_lines {
                let _ = line.paint(*at, p.cell_h, gpui::TextAlign::Left, None, window, cx);
            }
        });
    }
}

/// `workspaceai --term <cmd>`: a window with only a terminal and a one-line status bar (owner test harness).
pub struct TermHarness {
    term: Entity<TerminalView>,
    argv: String,
}

impl TermHarness {
    pub fn new(argv: Vec<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shown = argv.join(" ");
        let opts = SpawnOptions { argv, env: Default::default(), cwd: std::env::current_dir().ok(), size: Size::new(80, 24) };
        let term = cx.new(|cx| TerminalView::new(opts, cx));
        cx.subscribe(&term, |_, _, e: &TermViewEvent, cx| match e {
            TermViewEvent::OpenLink(url) if is_web(url) => cx.open_url(url),
            _ => cx.notify(),
        })
        .detach();
        let focus = term.read(cx).focus.clone();
        focus.focus(window, cx);
        TermHarness { term, argv: shown }
    }
}

impl Render for TermHarness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::ui::{ACCENT, DANGER, MUTED, PANEL};
        let t = self.term.read(cx);
        let (dot, label) = match t.status() {
            Status::Working => (0x5fb36b, "working".to_string()),
            Status::NeedsYou => (ACCENT, "needs you".to_string()),
            Status::Idle => (MUTED, "idle".to_string()),
            Status::Exited(c) => (DANGER, format!("exited {}", c.map_or("(signal)".to_string(), |c| c.to_string()))),
        };
        let title = t.title().unwrap_or(&self.argv).to_string();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(gpui::rgb(PANEL))
            .text_color(gpui::rgb(0xe5e5e5))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .text_sm()
                    .child(div().size(px(8.)).rounded_full().bg(gpui::rgb(dot)))
                    .child(div().flex_1().min_w_0().overflow_hidden().child(SharedString::from(title)))
                    .child(div().text_color(gpui::rgb(MUTED)).child(SharedString::from(label))),
            )
            .child(div().flex_1().min_h_0().child(self.term.clone()))
    }
}

// ---------- pure helpers (unit-tested) ----------

pub fn hsla(c: Rgb) -> Hsla {
    gpui::rgb(((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32).into()
}

fn dim(c: Rgb) -> Rgb {
    let f = |v: u8| (v as u16 * 2 / 3) as u8;
    Rgb { r: f(c.r), g: f(c.g), b: f(c.b) }
}

/// Terminal colour → RGB with the `default_rgb` palette.
/// ponytail: OSC 4/10/11 palette changes by the app are ignored (Snapshot does not carry `term.colors()`).
pub fn rgb_of(c: Color) -> Rgb {
    match c {
        Color::Spec(rgb) => rgb,
        Color::Indexed(i) => default_rgb(i as usize),
        Color::Named(n) => match n as usize {
            i @ 0..=258 => default_rgb(i),
            _ if n == NamedColor::BrightForeground => default_rgb(15),
            _ if n == NamedColor::DimForeground => dim(default_rgb(256)),
            // DimBlack..=DimWhite follow Cursor (258) in order.
            i => dim(default_rgb(i - NamedColor::DimBlack as usize)),
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellStyle {
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

pub fn cell_style(cell: &Cell, selected: bool) -> CellStyle {
    let f = cell.flags;
    let (mut fg, mut bg) = (rgb_of(cell.fg), rgb_of(cell.bg));
    if f.contains(Flags::DIM) {
        fg = dim(fg);
    }
    if f.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if selected {
        bg = SELECTION;
    }
    if f.contains(Flags::HIDDEN) {
        fg = bg;
    }
    CellStyle { fg, bg, bold: f.contains(Flags::BOLD), italic: f.contains(Flags::ITALIC), underline: f.intersects(Flags::ALL_UNDERLINES), strike: f.contains(Flags::STRIKEOUT) }
}

/// A stretch of cells painted as one shaped line starting at `col`, `cols` cells wide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub col: usize,
    pub cols: usize,
    pub text: String,
    pub style: CellStyle,
}

/// Groups a row into runs of equal style. Wide characters and any non-ASCII cell get a run of their own,
/// so a fallback font with a different advance cannot push the rest of the row off the grid.
pub fn row_runs(cells: &[Cell], selected: impl Fn(usize) -> bool) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for (col, cell) in cells.iter().enumerate() {
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let style = cell_style(cell, selected(col));
        let mut text = String::from(row_char(cell));
        if let Some(zw) = cell.zerowidth() {
            text.extend(zw);
        }
        let cols = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
        let alone = cols == 2 || !text.is_ascii();
        match out.last_mut() {
            Some(r) if !alone && r.text.is_ascii() && r.cols == r.text.len() && r.style == style && r.col + r.cols == col => {
                r.text.push_str(&text);
                r.cols += 1;
            }
            _ => out.push(Run { col, cols, text, style }),
        }
    }
    out
}

fn row_char(cell: &Cell) -> char {
    if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) || cell.c < ' ' {
        ' '
    } else {
        cell.c
    }
}

/// Merged background rectangles `(col, cols, colour)` for runs whose background is not the default.
pub fn backgrounds(runs: &[Run], default_bg: Rgb) -> Vec<(usize, usize, Rgb)> {
    let mut out: Vec<(usize, usize, Rgb)> = Vec::new();
    for r in runs.iter().filter(|r| r.style.bg != default_bg) {
        match out.last_mut() {
            Some((c, n, bg)) if *bg == r.style.bg && *c + *n == r.col => *n += r.cols,
            _ => out.push((r.col, r.cols, r.style.bg)),
        }
    }
    out
}

/// GPUI keystroke → encoder input. `None` for plain/Shift printable text (the platform delivers it through
/// the input handler, which also covers IME and dead keys) and for Cmd/Super combos (app/OS shortcuts).
pub fn key_of(k: &Keystroke) -> Option<(Key, Mods)> {
    let m = k.modifiers;
    if m.platform {
        return None;
    }
    let mods = mods_of(&m);
    let key = match k.key.as_str() {
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "escape" => Key::Escape,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "insert" => Key::Insert,
        "delete" => Key::Delete,
        "space" if m.control || m.alt => Key::Char(' '),
        f if f.len() >= 2 && f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => Key::F(f[1..].parse().unwrap()),
        s if (m.control || m.alt) && s.chars().count() == 1 => {
            let c = s.chars().next().unwrap();
            Key::Char(if m.shift { c.to_ascii_uppercase() } else { c })
        }
        _ => return None,
    };
    Some((key, mods))
}

fn mods_of(m: &Modifiers) -> Mods {
    Mods { ctrl: m.control, alt: m.alt, shift: m.shift }
}

/// Columns and rows that fit; at least 1×1.
pub fn grid_size(width: f32, height: f32, cell_w: f32, cell_h: f32) -> (usize, usize) {
    (((width / cell_w).floor() as usize).max(1), ((height / cell_h).floor() as usize).max(1))
}

/// Viewport cell `(row, col, side)` under a point relative to the grid origin, clamped to the grid.
pub fn cell_at(x: f32, y: f32, cell_w: f32, cell_h: f32, cols: usize, rows: usize) -> (usize, usize, Side) {
    let (cols, rows) = (cols.max(1), rows.max(1));
    let fx = (x / cell_w.max(1.)).max(0.);
    let col = (fx as usize).min(cols - 1);
    let side = if fx >= cols as f32 || fx - col as f32 >= 0.5 { Side::Right } else { Side::Left };
    let row = ((y / cell_h.max(1.)).max(0.) as usize).min(rows - 1);
    (row, col, side)
}

/// xterm mouse report. `code`: 0/1/2 buttons, +32 motion, 64/65 wheel. SGR when the app enabled it,
/// else the X10 byte form (`None` past column/row 223, which it cannot express).
/// ponytail: UTF-8 mouse mode (1005) is sent as X10; add when an app needs it.
pub fn mouse_report(code: u8, row: usize, col: usize, press: bool, mods: Mods, mode: TermMode) -> Option<Vec<u8>> {
    let m = 4 * mods.shift as u8 + 8 * mods.alt as u8 + 16 * mods.ctrl as u8;
    if mode.contains(TermMode::SGR_MOUSE) {
        return Some(format!("\x1b[<{};{};{}{}", code + m, col + 1, row + 1, if press { 'M' } else { 'm' }).into_bytes());
    }
    let b = if press || code >= 64 { code } else { 3 } + m;
    let (x, y) = (col + 1 + 32, row + 1 + 32);
    (x <= 255 && y <= 255).then(|| vec![0x1b, b'[', b'M', 32 + b, x as u8, y as u8])
}

fn is_web(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    (l.starts_with("http://") || l.starts_with("https://")) && url.len() > 8
}

/// URLs in a row (one char per column): `http(s)://…` and bare `localhost:port` / `127.0.0.1:port`
/// (returned as `http://…`). Ranges are columns.
/// ponytail: a URL wrapped onto the next row is cut at the row end.
pub fn find_links(line: &[char]) -> Vec<(Range<usize>, String)> {
    let starts_with = |i: usize, p: &str| p.chars().enumerate().all(|(k, c)| line.get(i + k).is_some_and(|l| l.eq_ignore_ascii_case(&c)));
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        let boundary = i == 0 || !(line[i - 1].is_alphanumeric() || "/.:@-_".contains(line[i - 1]));
        let (scheme, bare) = if starts_with(i, "https://") {
            (8, false)
        } else if starts_with(i, "http://") {
            (7, false)
        } else if boundary && (starts_with(i, "localhost:") || starts_with(i, "127.0.0.1:")) {
            (0, true)
        } else {
            i += 1;
            continue;
        };
        let mut end = i + scheme;
        while end < line.len() && !line[end].is_whitespace() && !"'\"`<>".contains(line[end]) {
            end += 1;
        }
        // Trailing punctuation belongs to the prose, a closing bracket only when it opens nowhere in the URL.
        while end > i + scheme {
            let c = line[end - 1];
            let opens = |o: char| line[i..end - 1].contains(&o);
            if ".,;:!?".contains(c) || (c == ')' && !opens('(')) || (c == ']' && !opens('[')) {
                end -= 1;
            } else {
                break;
            }
        }
        let text: String = line[i..end].iter().collect();
        let ok = if bare { text.split(':').nth(1).is_some_and(|p| p.starts_with(|c: char| c.is_ascii_digit())) } else { end > i + scheme };
        if ok {
            out.push((i..end, if bare { format!("http://{text}") } else { text }));
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

pub fn link_at(line: &[char], col: usize) -> Option<(Range<usize>, String)> {
    find_links(line).into_iter().find(|(r, _)| r.contains(&col))
}

/// Session status dot: exited > bell (needs you) > recent output (working) > idle.
pub fn status(exit: Option<Option<i32>>, bell: bool, last_output: Option<Instant>, now: Instant) -> Status {
    match exit {
        Some(code) => Status::Exited(code),
        None if bell => Status::NeedsYou,
        None if last_output.is_some_and(|t| now.duration_since(t) < WORKING_FOR) => Status::Working,
        None => Status::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Keystroke;

    fn cell(c: char) -> Cell {
        Cell { c, ..Cell::default() }
    }

    #[test]
    fn palette_matches_term() {
        assert_eq!(rgb_of(Color::Named(NamedColor::Red)), default_rgb(1));
        assert_eq!(rgb_of(Color::Named(NamedColor::Foreground)), default_rgb(256));
        assert_eq!(rgb_of(Color::Named(NamedColor::Background)), default_rgb(257));
        assert_eq!(rgb_of(Color::Indexed(196)), default_rgb(196));
        assert_eq!(rgb_of(Color::Named(NamedColor::DimRed)), dim(default_rgb(1)));
        assert_eq!(rgb_of(Color::Spec(Rgb { r: 1, g: 2, b: 3 })), Rgb { r: 1, g: 2, b: 3 });
    }

    #[test]
    fn runs_group_by_style_and_isolate_wide_and_non_ascii() {
        let mut cells: Vec<Cell> = "ab  cd".chars().map(cell).collect();
        cells[4].fg = Color::Named(NamedColor::Red);
        cells[5].fg = Color::Named(NamedColor::Red);
        let mut wide = cell('漢');
        wide.flags = Flags::WIDE_CHAR;
        let mut spacer = cell(' ');
        spacer.flags = Flags::WIDE_CHAR_SPACER;
        cells.extend([wide, spacer, cell('│'), cell('x'), cell('y')]);
        let runs = row_runs(&cells, |_| false);
        let got: Vec<(usize, usize, &str)> = runs.iter().map(|r| (r.col, r.cols, r.text.as_str())).collect();
        assert_eq!(got, vec![(0, 4, "ab  "), (4, 2, "cd"), (6, 2, "漢"), (8, 1, "│"), (9, 2, "xy")]);
        assert_eq!(runs[1].style.fg, default_rgb(1));
    }

    #[test]
    fn inverse_selection_and_backgrounds() {
        let mut cells: Vec<Cell> = "abcd".chars().map(cell).collect();
        cells[0].flags = Flags::INVERSE;
        let st = cell_style(&cells[0], false);
        assert_eq!((st.fg, st.bg), (default_rgb(257), default_rgb(256)));
        let runs = row_runs(&cells, |c| c >= 2);
        let bgs = backgrounds(&runs, default_rgb(257));
        assert_eq!(bgs, vec![(0, 1, default_rgb(256)), (2, 2, SELECTION)]);
    }

    fn ks(s: &'static str) -> Keystroke {
        Keystroke::parse(s).unwrap()
    }

    #[test]
    fn key_mapping() {
        let n = TermMode::empty();
        let enc = |s: &'static str| key_of(&ks(s)).and_then(|(k, m)| encode_key(k, m, n));
        assert_eq!(enc("ctrl-c").as_deref(), Some(&b"\x03"[..]));
        assert_eq!(enc("alt-b").as_deref(), Some(&b"\x1bb"[..]));
        assert_eq!(enc("alt-shift-b").as_deref(), Some(&b"\x1bB"[..]));
        assert_eq!(enc("enter").as_deref(), Some(&b"\r"[..]));
        assert_eq!(enc("shift-tab").as_deref(), Some(&b"\x1b[Z"[..]));
        assert_eq!(enc("ctrl-space").as_deref(), Some(&b"\x00"[..]));
        assert_eq!(enc("f5").as_deref(), Some(&b"\x1b[15~"[..]));
        assert_eq!(enc("ctrl-up").as_deref(), Some(&b"\x1b[1;5A"[..]));
        // Printable text goes through the input handler (IME), Cmd/Super combos to the app.
        assert_eq!(key_of(&ks("a")), None);
        assert_eq!(key_of(&ks("shift-a")), None);
        assert_eq!(key_of(&ks("space")), None);
        assert_eq!(key_of(&ks("cmd-c")), None);
        let app = TermMode::APP_CURSOR;
        let (k, m) = key_of(&ks("up")).unwrap();
        assert_eq!(encode_key(k, m, app).as_deref(), Some(&b"\x1bOA"[..]));
    }

    #[test]
    fn key_bindings_parse() {
        // KeyBinding::new panics on a keystroke GPUI cannot parse.
        for s in ["ctrl-shift-c", "ctrl-shift-v", "cmd-c", "cmd-v", "escape"] {
            ks(s);
        }
    }

    #[test]
    fn selection_geometry() {
        // 10px × 20px cells, 80×24 grid.
        assert_eq!(cell_at(0., 0., 10., 20., 80, 24), (0, 0, Side::Left));
        assert_eq!(cell_at(14., 0., 10., 20., 80, 24), (0, 1, Side::Left));
        assert_eq!(cell_at(16., 45., 10., 20., 80, 24), (2, 1, Side::Right));
        assert_eq!(cell_at(-5., -5., 10., 20., 80, 24), (0, 0, Side::Left));
        assert_eq!(cell_at(5000., 5000., 10., 20., 80, 24), (23, 79, Side::Right));
        assert_eq!(grid_size(805., 490., 10., 20.), (80, 24));
        assert_eq!(grid_size(3., 3., 10., 20.), (1, 1));
    }

    #[test]
    fn mouse_reports() {
        let sgr = TermMode::SGR_MOUSE | TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(mouse_report(0, 4, 9, true, Mods::NONE, sgr).unwrap(), b"\x1b[<0;10;5M");
        assert_eq!(mouse_report(0, 4, 9, false, Mods::NONE, sgr).unwrap(), b"\x1b[<0;10;5m");
        assert_eq!(mouse_report(64, 0, 0, true, Mods::CTRL, sgr).unwrap(), b"\x1b[<80;1;1M");
        let x10 = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(mouse_report(0, 0, 0, true, Mods::NONE, x10).unwrap(), vec![0x1b, b'[', b'M', 32, 33, 33]);
        assert_eq!(mouse_report(0, 0, 0, false, Mods::NONE, x10).unwrap(), vec![0x1b, b'[', b'M', 35, 33, 33]);
        assert_eq!(mouse_report(0, 0, 300, true, Mods::NONE, x10), None);
    }

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn link_detection() {
        let l = chars("see https://example.com/a_(b), or localhost:5173. ok");
        let links = find_links(&l);
        assert_eq!(links, vec![(4..29, "https://example.com/a_(b)".to_string()), (34..48, "http://localhost:5173".to_string())]);
        assert_eq!(link_at(&l, 10).map(|x| x.1), Some("https://example.com/a_(b)".to_string()));
        assert_eq!(link_at(&l, 2), None);
        assert_eq!(find_links(&chars("(http://127.0.0.1:8080/x)")), vec![(1..24, "http://127.0.0.1:8080/x".to_string())]);
        assert_eq!(find_links(&chars("http:// nothing")), vec![]);
        assert_eq!(find_links(&chars("mylocalhost:80 localhost:abc")), vec![]);
        assert_eq!(find_links(&chars("\"https://a.b/c\"")), vec![(1..14, "https://a.b/c".to_string())]);
        assert!(is_web("https://x.y") && !is_web("file:///etc/passwd") && !is_web("javascript:alert(1)"));
    }

    #[test]
    fn status_order() {
        let now = Instant::now();
        assert_eq!(status(None, false, None, now), Status::Idle);
        assert_eq!(status(None, false, Some(now), now), Status::Working);
        assert_eq!(status(None, false, Some(now - Duration::from_secs(3)), now), Status::Idle);
        assert_eq!(status(None, true, Some(now), now), Status::NeedsYou);
        assert_eq!(status(Some(Some(0)), true, Some(now), now), Status::Exited(Some(0)));
    }
}
