//! PTY + terminal model over `alacritty_terminal`, with no GPUI dependency.
//!
//! `TermSession::spawn` starts a child on a PTY; alacritty's reader thread parses its
//! output into the grid and reports through the returned `Receiver<TermEvent>`.
//! Replies the terminal owes the child (DSR, OSC 10/11 colour queries, size reports)
//! are written back here, so headless sessions behave like real ones.

mod keys;

pub use alacritty_terminal::grid::{Dimensions, Scroll};
pub use alacritty_terminal::index::{Column, Line, Point, Side};
pub use alacritty_terminal::selection::{SelectionRange, SelectionType};
pub use alacritty_terminal::term::cell::{Cell, Flags};
pub use alacritty_terminal::term::TermMode;
pub use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
pub use keys::{encode_key, paste_bytes, Key, Mods};

use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, State};
use alacritty_terminal::selection::Selection;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{point_to_viewport, viewport_to_point, Config, Term};
use alacritty_terminal::tty::{self, Pty};

pub const SCROLLBACK: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermEvent {
    /// New output was parsed; repaint (coalesce on the receiving side).
    Wakeup,
    Title(Option<String>),
    Bell,
    /// OSC 52 copy request from the child.
    ClipboardStore(String),
    /// The child exited; the code is `None` when it died from a signal or was not reaped.
    Exit(Option<i32>),
}

pub struct SpawnOptions {
    /// Program and arguments; must not be empty.
    pub argv: Vec<String>,
    /// Added on top of the inherited environment (after `TERM`/`COLORTERM` defaults).
    pub env: HashMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub size: Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Size {
    pub fn new(cols: u16, rows: u16) -> Size {
        Size { cols, rows, cell_width: 8, cell_height: 16 }
    }
    fn window(self) -> WindowSize {
        WindowSize { num_lines: self.rows, num_cols: self.cols, cell_width: self.cell_width, cell_height: self.cell_height }
    }
    fn dims(self) -> TermSize {
        TermSize::new(self.cols.max(1) as usize, self.rows.max(1) as usize)
    }
}

#[derive(Default)]
struct Shared {
    title: Mutex<Option<String>>,
    exit: Mutex<Option<ExitStatus>>,
}

struct Listener {
    tx: Sender<TermEvent>,
    shared: Arc<Shared>,
    size: Arc<Mutex<Size>>,
    /// Set right after the event loop exists (the `Term` needs the listener first).
    pty: Arc<OnceLock<EventLoopSender>>,
}

impl Listener {
    fn reply(&self, s: String) {
        if let Some(pty) = self.pty.get() {
            let _ = pty.send(Msg::Input(Cow::Owned(s.into_bytes())));
        }
    }
    fn emit(&self, e: TermEvent) {
        let _ = self.tx.send(e);
    }
}

impl EventListener for Listener {
    // Runs on the reader thread with the Term locked: never lock the Term here.
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => self.emit(TermEvent::Wakeup),
            Event::Bell => self.emit(TermEvent::Bell),
            Event::Title(t) => {
                *self.shared.title.lock().unwrap() = Some(t.clone());
                self.emit(TermEvent::Title(Some(t)));
            }
            Event::ResetTitle => {
                *self.shared.title.lock().unwrap() = None;
                self.emit(TermEvent::Title(None));
            }
            Event::ClipboardStore(_, text) => self.emit(TermEvent::ClipboardStore(text)),
            Event::PtyWrite(s) => self.reply(s),
            Event::ColorRequest(i, format) => self.reply(format(default_rgb(i))),
            Event::TextAreaSizeRequest(format) => self.reply(format(self.size.lock().unwrap().window())),
            Event::ChildExit(status) => *self.shared.exit.lock().unwrap() = Some(status),
            Event::Exit => {
                let code = self.shared.exit.lock().unwrap().and_then(|s| s.code());
                self.emit(TermEvent::Exit(code));
            }
            // OSC 52 paste is disabled by the default `Osc52::OnlyCopy`; cursor/blink are paint concerns.
            Event::ClipboardLoad(..) | Event::MouseCursorDirty | Event::CursorBlinkingChange => {}
        }
    }
}

/// xterm default colours: 0..16 named, 16..232 the 6x6x6 cube, 232..256 greys,
/// 256 foreground, 257 background, 258 cursor. Answers OSC 4/10/11 queries.
pub fn default_rgb(index: usize) -> Rgb {
    const ANSI: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00), (0xcd, 0x00, 0x00), (0x00, 0xcd, 0x00), (0xcd, 0xcd, 0x00),
        (0x00, 0x00, 0xee), (0xcd, 0x00, 0xcd), (0x00, 0xcd, 0xcd), (0xe5, 0xe5, 0xe5),
        (0x7f, 0x7f, 0x7f), (0xff, 0x00, 0x00), (0x00, 0xff, 0x00), (0xff, 0xff, 0x00),
        (0x5c, 0x5c, 0xff), (0xff, 0x00, 0xff), (0x00, 0xff, 0xff), (0xff, 0xff, 0xff),
    ];
    let (r, g, b) = match index {
        0..=15 => ANSI[index],
        16..=231 => {
            let i = index - 16;
            let v = |c: usize| if c == 0 { 0 } else { (55 + 40 * c) as u8 };
            (v(i / 36), v(i / 6 % 6), v(i % 6))
        }
        232..=255 => {
            let v = (8 + 10 * (index - 232)) as u8;
            (v, v, v)
        }
        257 => (0x1e, 0x1e, 0x1e),
        _ => (0xe5, 0xe5, 0xe5), // 256 foreground, 258 cursor
    };
    Rgb { r, g, b }
}

/// The visible viewport, copied out of the grid so painting never holds the lock.
pub struct Snapshot {
    pub cols: usize,
    pub rows: usize,
    /// Row-major, `rows * cols`.
    pub cells: Vec<Cell>,
    /// Viewport position of the cursor; `None` when it is scrolled out of view.
    pub cursor: Option<Point<usize>>,
    pub cursor_shape: CursorShape,
    pub mode: TermMode,
    pub display_offset: usize,
    selection: Option<SelectionRange>,
}

impl Snapshot {
    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.cells[row * self.cols + col]
    }
    pub fn row_text(&self, row: usize) -> String {
        let cells = &self.cells[row * self.cols..(row + 1) * self.cols];
        let s: String = cells.iter().filter(|c| !c.flags.contains(Flags::WIDE_CHAR_SPACER)).map(|c| c.c).collect();
        s.trim_end().to_string()
    }
    pub fn is_selected(&self, row: usize, col: usize) -> bool {
        let p = viewport_to_point(self.display_offset, Point::new(row, Column(col)));
        self.selection.is_some_and(|s| s.contains(p))
    }
}

pub struct TermSession {
    term: Arc<FairMutex<Term<Listener>>>,
    pty: EventLoopSender,
    size: Arc<Mutex<Size>>,
    shared: Arc<Shared>,
    pid: u32,
    reader: Option<JoinHandle<(EventLoop<Pty, Listener>, State)>>,
}

impl TermSession {
    pub fn spawn(opts: SpawnOptions) -> io::Result<(TermSession, Receiver<TermEvent>)> {
        let (program, args) = opts
            .argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
        let mut env = HashMap::from([
            ("TERM".to_string(), "xterm-256color".to_string()),
            ("COLORTERM".to_string(), "truecolor".to_string()),
        ]);
        env.extend(opts.env);
        let config = tty::Options {
            shell: Some(tty::Shell::new(program.clone(), args.to_vec())),
            working_directory: opts.cwd,
            drain_on_exit: true,
            env,
        };
        let pty = tty::new(&config, opts.size.window(), 0)?;
        let pid = pty.child().id();

        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared::default());
        let size = Arc::new(Mutex::new(opts.size));
        let pty_tx = Arc::new(OnceLock::new());
        let listener = Listener { tx: tx.clone(), shared: shared.clone(), size: size.clone(), pty: pty_tx.clone() };
        let term_config = Config { scrolling_history: SCROLLBACK, ..Config::default() };
        let term = Arc::new(FairMutex::new(Term::new(term_config, &opts.size.dims(), listener)));
        let listener = Listener { tx, shared: shared.clone(), size: size.clone(), pty: pty_tx.clone() };
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let sender = event_loop.channel();
        let _ = pty_tx.set(event_loop.channel());
        let reader = Some(event_loop.spawn());
        Ok((TermSession { term, pty: sender, size, shared, pid, reader }, rx))
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        let _ = self.pty.send(Msg::Input(Cow::Owned(bytes.into())));
    }

    /// Text as a paste (bracketed when the app enabled it).
    pub fn paste(&self, text: &str) {
        self.write(paste_bytes(text, self.mode()));
    }

    /// Resizes the grid now and the PTY (SIGWINCH) before any input written after this call.
    pub fn resize(&self, size: Size) {
        *self.size.lock().unwrap() = size;
        self.term.lock().resize(size.dims());
        let _ = self.pty.send(Msg::Resize(size.window()));
    }

    pub fn size(&self) -> Size {
        *self.size.lock().unwrap()
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    pub fn title(&self) -> Option<String> {
        self.shared.title.lock().unwrap().clone()
    }

    pub fn exit_status(&self) -> Option<ExitStatus> {
        *self.shared.exit.lock().unwrap()
    }

    /// `Scroll::Delta(n)` with n > 0 moves up into history.
    pub fn scroll(&self, scroll: Scroll) {
        self.term.lock().scroll_display(scroll);
    }

    pub fn snapshot(&self) -> Snapshot {
        let term = self.term.lock();
        let grid = term.grid();
        let (cols, rows, offset) = (grid.columns(), grid.screen_lines(), grid.display_offset());
        let mut cells = Vec::with_capacity(cols * rows);
        for row in 0..rows {
            let line = &grid[Line(row as i32 - offset as i32)];
            cells.extend((0..cols).map(|c| line[Column(c)].clone()));
        }
        let content = term.renderable_content();
        Snapshot {
            cols,
            rows,
            cells,
            cursor: point_to_viewport(offset, content.cursor.point),
            cursor_shape: content.cursor.shape,
            mode: content.mode,
            display_offset: offset,
            selection: content.selection,
        }
    }

    /// Starts a selection at a viewport cell (`Simple` drag, `Semantic` double-click, `Lines` triple-click).
    pub fn select_start(&self, ty: SelectionType, row: usize, col: usize, side: Side) {
        let mut term = self.term.lock();
        let p = viewport_to_point(term.grid().display_offset(), Point::new(row, Column(col)));
        term.selection = Some(Selection::new(ty, p, side));
    }

    pub fn select_update(&self, row: usize, col: usize, side: Side) {
        let mut term = self.term.lock();
        let p = viewport_to_point(term.grid().display_offset(), Point::new(row, Column(col)));
        if let Some(sel) = term.selection.as_mut() {
            sel.update(p, side);
        }
    }

    pub fn select_clear(&self) {
        self.term.lock().selection = None;
    }

    /// Selected text; soft-wrapped lines join without a newline.
    pub fn selection_text(&self) -> Option<String> {
        self.term.lock().selection_to_string()
    }
}

impl Drop for TermSession {
    /// SIGHUP to the child's process group (it is a session leader), SIGKILL after 3 s.
    fn drop(&mut self) {
        let _ = self.pty.send(Msg::Shutdown);
        let pgid = self.pid as libc::pid_t;
        unsafe { libc::kill(-pgid, libc::SIGHUP) };
        let Some(reader) = self.reader.take() else { return };
        // Joining drops alacritty's `Pty`, which waits for the child: keep that off the caller.
        thread::spawn(move || {
            let reaped = Arc::new(AtomicBool::new(false));
            let flag = reaped.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(3));
                if !flag.load(Ordering::SeqCst) {
                    unsafe { libc::kill(-pgid, libc::SIGKILL) };
                }
            });
            drop(reader.join());
            reaped.store(true, Ordering::SeqCst);
        });
    }
}
