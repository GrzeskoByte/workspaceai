//! Headless PTY tests: real child processes, no window.

use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use workspaceai_term::*;

fn spawn(script: &str, cols: u16, rows: u16) -> (TermSession, Receiver<TermEvent>) {
    TermSession::spawn(SpawnOptions {
        argv: vec!["sh".into(), "-c".into(), script.into()],
        env: HashMap::new(),
        cwd: None,
        size: Size::new(cols, rows),
    })
    .expect("spawn sh")
}

fn wait_exit(rx: &Receiver<TermEvent>) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if let Ok(TermEvent::Exit(code)) = rx.recv_timeout(left) {
            return code;
        }
    }
    panic!("no Exit event within 10 s");
}

#[test]
fn colour_cell() {
    let (s, rx) = spawn(r"printf '\033[31mhi'", 80, 24);
    wait_exit(&rx);
    let snap = s.snapshot();
    assert_eq!(snap.cell(0, 0).c, 'h');
    assert_eq!(snap.cell(0, 0).fg, Color::Named(NamedColor::Red));
    assert_eq!(snap.cell(0, 1).c, 'i');
    assert_eq!(snap.cell(0, 2).fg, Color::Named(NamedColor::Foreground));
}

#[test]
fn resize_reaches_child() {
    let (s, rx) = spawn("read x; stty size", 80, 24);
    s.resize(Size::new(40, 10));
    s.write("\r");
    assert_eq!(wait_exit(&rx), Some(0));
    let snap = s.snapshot();
    assert_eq!((snap.cols, snap.rows), (40, 10));
    let text: Vec<String> = (0..snap.rows).map(|r| snap.row_text(r)).collect();
    assert!(text.iter().any(|l| l == "10 40"), "{text:?}");
}

#[test]
fn exit_status_arrives() {
    let (s, rx) = spawn("exit 3", 80, 24);
    assert_eq!(wait_exit(&rx), Some(3));
    assert_eq!(s.exit_status().and_then(|e| e.code()), Some(3));
}

#[test]
fn title_and_bell() {
    let (s, rx) = spawn(r"printf '\033]2;hello\007\007'", 80, 24);
    let mut seen = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(10)).expect("event") {
            TermEvent::Exit(_) => break,
            TermEvent::Wakeup => {}
            e => seen.push(e),
        }
    }
    assert_eq!(seen, vec![TermEvent::Title(Some("hello".into())), TermEvent::Bell]);
    assert_eq!(s.title().as_deref(), Some("hello"));
}

#[test]
fn wrapped_line_selection_copies_without_newline() {
    let (s, rx) = spawn("printf abcdefghijklmno; printf '\\r\\nnext'", 10, 5);
    wait_exit(&rx);
    let snap = s.snapshot();
    assert_eq!(snap.row_text(0), "abcdefghij");
    assert_eq!(snap.row_text(1), "klmno");
    s.select_start(SelectionType::Simple, 0, 0, Side::Left);
    s.select_update(1, 4, Side::Right);
    assert_eq!(s.selection_text().as_deref(), Some("abcdefghijklmno"));
    assert!(s.snapshot().is_selected(1, 4));
    // A hard newline is kept.
    s.select_update(2, 3, Side::Right);
    assert_eq!(s.selection_text().as_deref(), Some("abcdefghijklmno\nnext"));
}

#[test]
fn cursor_position_report_is_answered() {
    // The child asks for the cursor position (DSR 6) and echoes what it got back.
    let (s, rx) = spawn(
        r"stty raw -echo; printf 'ab\033[6n'; r=$(dd bs=1 count=6 2>/dev/null | tr -d '\033'); stty sane; printf '\r\n%s' $r",
        80,
        24,
    );
    wait_exit(&rx);
    assert_eq!(s.snapshot().row_text(1), "[1;3R");
}

#[test]
fn drop_hangs_up_the_child() {
    let (s, _rx) = spawn("sleep 30", 80, 24);
    let pid = s.pid() as i32;
    drop(s);
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(pid, 0) } == 0 {
        assert!(Instant::now() < deadline, "child {pid} still alive");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn drop_kills_a_child_that_ignores_hangup() {
    let (s, rx) = spawn("trap '' HUP; echo ready; sleep 30", 80, 24);
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.snapshot().row_text(0) != "ready" {
        assert!(Instant::now() < deadline, "shell never got ready");
        let _ = rx.recv_timeout(Duration::from_millis(50));
    }
    let pid = s.pid() as i32;
    drop(s);
    let deadline = Instant::now() + Duration::from_secs(8);
    while unsafe { libc::kill(pid, 0) } == 0 {
        assert!(Instant::now() < deadline, "child {pid} survived SIGKILL escalation");
        std::thread::sleep(Duration::from_millis(50));
    }
}
