//! Headless runs (routines, pipeline): prompt an opencode session and wait for its reply. Takes a client and the
//! emitter, not the runtime, so callers drop the supervisor lock for the (long) wait.

use crate::engine::OpencodeClient;
use crate::sessions::map_oc_messages;
use crate::supervisor::{ApiError, Emitter, UiEvent};
use std::collections::HashSet;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use workspaceai_shared::util::now_ms;

/// How often pending permission asks are polled while the turn runs.
pub const PERMISSION_POLL: Duration = Duration::from_secs(2);

/// `askAndWait`: `prompt`, then `experimental/session/:id/wait` on a worker thread while this thread polls the
/// session's permission asks every 2 s and raises `UiEvent::Approval` once per ask. Returns the reply text.
pub fn ask_and_wait(client: &OpencodeClient, events: &Emitter, workspace: &str, chat: &str, sid: &str, text: &str, timeout: Duration) -> Result<String, ApiError> {
    let sent_at = now_ms();
    let deadline = Instant::now() + timeout;
    client.prompt(sid, text).map_err(|e| ApiError::bad_request(e.message))?;
    let mut seen: HashSet<String> = HashSet::new();
    while Instant::now() < deadline {
        let (tx, rx) = mpsc::channel();
        let (c, id) = (client.clone(), sid.to_string());
        let wait_for = deadline.saturating_duration_since(Instant::now()).min(Duration::from_secs(120));
        std::thread::spawn(move || {
            let _ = tx.send(c.wait_idle(&id, wait_for));
        });
        loop {
            match rx.recv_timeout(PERMISSION_POLL) {
                Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    for p in client.permissions(sid).unwrap_or_default() {
                        let pid = p["id"].as_str().unwrap_or_default().to_string();
                        if !pid.is_empty() && seen.insert(pid) {
                            events.emit(UiEvent::Approval { workspace: workspace.into(), chat: chat.into(), permission: p });
                        }
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                }
            }
        }
        let all = map_oc_messages(&client.messages(sid).map_err(|e| ApiError::bad_request(e.message))?);
        if let Some(reply) = all.iter().rev().find(|m| m["role"] == "assistant" && m["createdAt"].as_i64().unwrap_or(0) >= sent_at - 1000) {
            if let Some(e) = reply["error"].as_str() {
                return Err(ApiError::bad_request(e));
            }
            if let Some(t) = reply["text"].as_str().filter(|t| !t.trim().is_empty()) {
                return Ok(t.to_string());
            }
        }
        std::thread::sleep(Duration::from_millis(700));
    }
    Err(ApiError::bad_request("The agent did not answer in time"))
}
