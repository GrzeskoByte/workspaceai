use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use workspaceai_client::supervisor::{parse_ready, Supervisor};

#[test]
fn parse_ready_reads_port_and_token() {
    let info = parse_ready(r#"HARNESS_READY {"port":4242,"token":"abc"}"#).unwrap();
    assert_eq!((info.port, info.token.as_str()), (4242, "abc"));
}

#[test]
fn parse_ready_rejects_bad_lines() {
    assert!(parse_ready("hello").is_err());
    assert!(parse_ready("HARNESS_READY not json").unwrap_err().contains("json"));
    assert!(parse_ready(r#"HARNESS_READY {"token":"t"}"#).unwrap_err().contains("port"));
    assert!(parse_ready(r#"HARNESS_READY {"port":1}"#).unwrap_err().contains("token"));
}

/// A stand-in for `node`: prints the ready line, records its pid, and exits when stdin closes.
fn fake_node(dir: &PathBuf, ready: &str) -> PathBuf {
    let p = dir.join("node");
    fs::write(&p, format!("#!/bin/sh\necho $$ > \"{}/pid\"\n{ready}\ncat > /dev/null\n", dir.display())).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wsai-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn start_reads_ready_line_and_drop_stops_the_process() {
    let dir = temp("start");
    let node = fake_node(&dir, r#"echo 'HARNESS_READY {"port":5555,"token":"tok"}'"#);
    let (info, sup) = Supervisor::start(&dir, &node).unwrap();
    assert_eq!((info.port, info.token.as_str()), (5555, "tok"));
    let pid: i32 = fs::read_to_string(dir.join("pid")).unwrap().trim().parse().unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "process should be alive");
    drop(sup);
    // Reaped by Drop: the pid no longer exists.
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "process should be gone");
}

#[test]
fn start_fails_readably_when_no_ready_line_arrives() {
    let dir = temp("noready");
    let node = fake_node(&dir, "exit 3");
    let err = Supervisor::start(&dir, &node).err().unwrap();
    assert!(err.contains("did not become ready"), "{err}");
}
