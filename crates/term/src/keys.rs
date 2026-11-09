//! Key -> bytes (xterm sequences, modifyOtherKeys off) and paste wrapping.

use alacritty_terminal::term::TermMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// The character the keyboard layout produced (Shift already applied).
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1..=F12.
    F(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { ctrl: false, alt: false, shift: false };
    pub const CTRL: Mods = Mods { ctrl: true, alt: false, shift: false };
    pub const ALT: Mods = Mods { ctrl: false, alt: true, shift: false };
    pub const SHIFT: Mods = Mods { ctrl: false, alt: false, shift: true };

    /// xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl; `None` when no modifier.
    fn param(self) -> Option<u8> {
        let m = self.shift as u8 + 2 * self.alt as u8 + 4 * self.ctrl as u8;
        (m != 0).then_some(m + 1)
    }
}

/// Bytes to send to the PTY for a key press, or `None` when the key has no encoding.
pub fn encode_key(key: Key, mods: Mods, mode: TermMode) -> Option<Vec<u8>> {
    let alt = |mut b: Vec<u8>| {
        if mods.alt {
            b.insert(0, 0x1b);
        }
        Some(b)
    };
    // CSI letter keys: plain = SS3 in app-cursor mode for arrows/Home/End, else CSI.
    let letter = |c: u8, ss3: bool| {
        Some(match mods.param() {
            Some(m) => format!("\x1b[1;{m}{}", c as char).into_bytes(),
            None if ss3 => vec![0x1b, b'O', c],
            None => vec![0x1b, b'[', c],
        })
    };
    let tilde = |n: u8| {
        Some(match mods.param() {
            Some(m) => format!("\x1b[{n};{m}~").into_bytes(),
            None => format!("\x1b[{n}~").into_bytes(),
        })
    };
    let app = mode.contains(TermMode::APP_CURSOR);
    match key {
        Key::Char(c) => {
            if mods.ctrl {
                let b = match c {
                    'a'..='z' => c as u8 - b'a' + 1,
                    'A'..='Z' => c as u8 - b'A' + 1,
                    ' ' | '@' | '2' => 0,
                    '[' | '3' => 0x1b,
                    '\\' | '4' => 0x1c,
                    ']' | '5' => 0x1d,
                    '^' | '6' => 0x1e,
                    '_' | '/' | '7' => 0x1f,
                    '?' | '8' => 0x7f,
                    _ => return alt(c.to_string().into_bytes()),
                };
                alt(vec![b])
            } else {
                alt(c.to_string().into_bytes())
            }
        }
        Key::Enter => alt(vec![b'\r']),
        Key::Tab if mods.shift => Some(b"\x1b[Z".to_vec()),
        Key::Tab => alt(vec![b'\t']),
        Key::Backspace => alt(vec![if mods.ctrl { 0x08 } else { 0x7f }]),
        Key::Escape => alt(vec![0x1b]),
        Key::Up => letter(b'A', app),
        Key::Down => letter(b'B', app),
        Key::Right => letter(b'C', app),
        Key::Left => letter(b'D', app),
        Key::Home => letter(b'H', app),
        Key::End => letter(b'F', app),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F(n @ 1..=4) => letter(b'P' + n - 1, true),
        Key::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][n as usize - 5]),
        Key::F(_) => None,
    }
}

/// Bytes for pasted text: bracketed when the app asked for it (ESC stripped so the
/// paste cannot close the bracket early), otherwise newlines become CR like typed Enter.
pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_table() {
        let n = TermMode::empty();
        let app = TermMode::APP_CURSOR;
        let ctrl_shift = Mods { ctrl: true, shift: true, alt: false };
        let cases: &[(Key, Mods, TermMode, &[u8])] = &[
            (Key::Char('c'), Mods::CTRL, n, b"\x03"),
            (Key::Char('C'), ctrl_shift, n, b"\x03"),
            (Key::Char(' '), Mods::CTRL, n, b"\x00"),
            (Key::Char('['), Mods::CTRL, n, b"\x1b"),
            (Key::Char('b'), Mods::ALT, n, b"\x1bb"),
            (Key::Char('x'), Mods { ctrl: true, alt: true, shift: false }, n, b"\x1b\x18"),
            (Key::Char('ł'), Mods::NONE, n, "ł".as_bytes()),
            (Key::Up, Mods::NONE, n, b"\x1b[A"),
            (Key::Up, Mods::NONE, app, b"\x1bOA"),
            (Key::Up, Mods::CTRL, app, b"\x1b[1;5A"),
            (Key::Left, Mods::SHIFT, n, b"\x1b[1;2D"),
            (Key::Home, Mods::NONE, app, b"\x1bOH"),
            (Key::End, Mods::NONE, n, b"\x1b[F"),
            (Key::Enter, Mods::NONE, n, b"\r"),
            (Key::Enter, Mods::ALT, n, b"\x1b\r"),
            (Key::Tab, Mods::SHIFT, n, b"\x1b[Z"),
            (Key::Backspace, Mods::NONE, n, b"\x7f"),
            (Key::Backspace, Mods::CTRL, n, b"\x08"),
            (Key::Escape, Mods::NONE, n, b"\x1b"),
            (Key::Delete, Mods::NONE, n, b"\x1b[3~"),
            (Key::PageUp, Mods::NONE, n, b"\x1b[5~"),
            (Key::PageDown, Mods::CTRL, n, b"\x1b[6;5~"),
            (Key::F(1), Mods::NONE, n, b"\x1bOP"),
            (Key::F(4), Mods::SHIFT, n, b"\x1b[1;2S"),
            (Key::F(5), Mods::NONE, n, b"\x1b[15~"),
            (Key::F(12), Mods::NONE, n, b"\x1b[24~"),
        ];
        for (key, mods, mode, want) in cases {
            assert_eq!(encode_key(*key, *mods, *mode).as_deref(), Some(*want), "{key:?} {mods:?} {mode:?}");
        }
        assert_eq!(encode_key(Key::F(13), Mods::NONE, n), None);
    }

    #[test]
    fn paste() {
        assert_eq!(paste_bytes("a\r\nb\nc", TermMode::empty()), b"a\rb\rc");
        assert_eq!(
            paste_bytes("x\x1b[201~rm -rf\n", TermMode::BRACKETED_PASTE),
            b"\x1b[200~x[201~rm -rf\n\x1b[201~"
        );
    }
}
