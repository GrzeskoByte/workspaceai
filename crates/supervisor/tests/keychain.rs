//! Keychain backends (R-34): the in-memory one used by every test, and the OS one when this machine has it.

use workspaceai_supervisor::keychain::Keychain;

#[test]
fn keychain_memory_round_trip_is_per_workspace() {
    let k = Keychain::memory();
    assert_eq!(k.backend_name(), "memory");
    assert!(k.get("ws_a", "KEY").is_none());
    k.set("ws_a", "KEY", "value").unwrap();
    assert_eq!(k.get("ws_a", "KEY").as_deref(), Some("value"));
    assert_eq!(k.get("ws_b", "KEY"), None);
    assert!(k.has("ws_a", "KEY"));
    assert_eq!(k.list("ws_a"), vec!["KEY".to_string()]);
    k.set("ws_a", "SECOND", "two").unwrap();
    assert_eq!(k.list("ws_a"), vec!["KEY".to_string(), "SECOND".to_string()]);

    k.delete("ws_a", "KEY").unwrap();
    assert_eq!(k.get("ws_a", "KEY"), None);
    assert_eq!(k.list("ws_a"), vec!["SECOND".to_string()]);
    k.purge("ws_a");
    assert!(k.list("ws_a").is_empty());
    assert_eq!(k.get("ws_a", "SECOND"), None);
}

#[test]
fn keychain_os_backend_round_trip_or_skip() {
    let k = Keychain::os();
    assert_eq!(k.backend_name(), "os");
    let ws = format!("probe-{}", std::process::id());
    if k.set(&ws, "X", "y").is_err() {
        eprintln!("skipping OS keychain test: no Secret Service / Keychain available here");
        return;
    }
    let value = k.get(&ws, "X");
    k.purge(&ws);
    assert_eq!(value.as_deref(), Some("y"));
    assert_eq!(k.get(&ws, "X"), None);
}
