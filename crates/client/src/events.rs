//! The supervisor's server-sent event stream (`GET /api/events?token=…`).

use crate::api::Api;
use crate::types::UiEvent;
use std::io::{BufRead, BufReader};

/// One SSE line to an event. Comments, blanks, other fields and anything that is not a `UiEvent` give `None`.
pub fn parse_sse_line(line: &str) -> Option<UiEvent> {
    let json = line.strip_prefix("data:")?.trim_start();
    serde_json::from_str(json).ok()
}

/// Reads the stream until the supervisor closes it; the caller decides whether to reconnect.
pub fn stream(api: &Api, mut on_event: impl FnMut(UiEvent)) -> Result<(), String> {
    let res = ureq::get(&format!("{}/api/events?token={}", api.base, api.token)).call().map_err(|e| e.to_string())?;
    for line in BufReader::new(res.into_reader()).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if let Some(e) = parse_sse_line(&line) {
            on_event(e);
        }
    }
    Ok(())
}
