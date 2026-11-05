//! Routines: how a trigger reads, how a new one is described, and the requests behind the screen.

use crate::api::seg;
use crate::types::{Routine, RoutineTrigger};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use workspaceai_shared::schedule::{format_when, next_run, parse_schedule, ScheduleKind};

/// "Next: Tomorrow 09:00", "Once: …", "On git.push (main)" or "Not scheduled".
pub fn trigger_text(r: &Routine, now: DateTime<Utc>) -> String {
    match &r.trigger {
        RoutineTrigger::Schedule { once: Some(ms), tz, .. } => format!("Once: {}", format_when(*ms, tz, now.timestamp_millis())),
        RoutineTrigger::Schedule { cron, tz, .. } => match next_run(cron, tz, now) {
            Some(ms) => format!("Next: {}", format_when(ms, tz, now.timestamp_millis())),
            None => "Not scheduled".into(),
        },
        RoutineTrigger::Event { event, detail } if detail.is_empty() || detail == "*" => format!("On {event}"),
        RoutineTrigger::Event { event, detail } => format!("On {event} ({detail})"),
    }
}

/// "Last run: ok (note)", "Never run", with a "Paused · " prefix when paused.
pub fn last_run_text(r: &Routine) -> String {
    let last = match r.runs.iter().max_by_key(|x| x.at) {
        None => "Never run".to_string(),
        Some(run) if run.note.is_empty() => format!("Last run: {}", run.status),
        Some(run) => format!("Last run: {} ({})", run.status, run.note),
    };
    if r.paused {
        format!("Paused · {last}")
    } else {
        last
    }
}

/// `POST …/routines` body; both the phrase and the prompt are required.
pub fn create_body(phrase: &str, prompt: &str, tz: &str) -> Option<Value> {
    let (phrase, prompt) = (phrase.trim(), prompt.trim());
    if phrase.is_empty() || prompt.is_empty() {
        return None;
    }
    Some(json!({ "name": phrase.chars().take(80).collect::<String>(), "phrase": phrase, "tz": tz, "what": { "kind": "prompt", "prompt": prompt } }))
}

pub struct Preview {
    pub ok: bool,
    pub text: String,
}

/// How the phrase was understood, shown before anything is saved.
pub fn preview(phrase: &str, tz: &str, now: DateTime<Utc>) -> Preview {
    let p = parse_schedule(phrase, Some(tz), now);
    if !p.ok {
        return Preview { ok: false, text: p.error.unwrap_or_else(|| "Could not read that schedule".into()) };
    }
    let text = match (p.kind, p.next) {
        (ScheduleKind::Repeat, Some(ms)) => format!("{} · Next: {}", p.human, format_when(ms, tz, now.timestamp_millis())),
        _ => p.human,
    };
    Preview { ok: true, text }
}

pub fn item_path(workspace: &str, routine: &str) -> String {
    format!("/api/ws/{}/routines/{}", seg(workspace), seg(routine))
}

pub fn run_path(workspace: &str, routine: &str) -> String {
    format!("{}/run", item_path(workspace, routine))
}

pub fn pause_body(paused: bool) -> Value {
    json!({ "paused": paused })
}
