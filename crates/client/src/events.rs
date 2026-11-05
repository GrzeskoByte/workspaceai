//! The supervisor's server-sent event stream (`GET /api/events`, authenticated by the `x-harness-token` header).

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
    // The token goes in a header, never the URL: ureq error text quotes the URL, which would leak it to logs.
    let res = ureq::get(&format!("{}/api/events", api.base))
        .set("x-harness-token", &api.token)
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("the event stream returned HTTP {code}"),
            ureq::Error::Transport(t) => format!("the event stream could not connect ({:?})", t.kind()),
        })?;
    for line in BufReader::new(res.into_reader()).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if let Some(e) = parse_sse_line(&line) {
            on_event(e);
        }
    }
    Ok(())
}
