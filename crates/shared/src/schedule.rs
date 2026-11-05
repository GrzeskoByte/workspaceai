//! Routines in plain words: repeats ("every weekday at 9", "every 2 hours 9-18"), one-offs
//! ("tomorrow at 9", "in 3 hours") and events ("on push to main"). Port of `packages/shared/src/schedule.ts`.
//! The result says how the phrase was understood before it is saved.

use chrono::{DateTime, Datelike, Duration, NaiveDate, Offset, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use croner::Cron;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

macro_rules! re {
    ($s:expr) => {{
        static R: LazyLock<Regex> = LazyLock::new(|| Regex::new($s).unwrap());
        &*R
    }};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleKind {
    Repeat,
    Once,
    Event,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTrigger {
    pub event: String,
    pub detail: String,
}

/// Epoch milliseconds throughout, as in the TS original.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedSchedule {
    pub ok: bool,
    pub kind: ScheduleKind,
    pub cron: String,
    pub tz: String,
    pub once: Option<i64>,
    pub event: Option<EventTrigger>,
    pub next: Option<i64>,
    pub human: String,
    pub matched: Vec<String>,
    pub error: Option<String>,
}

const DAY_NAMES: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

fn day_number(name: &str) -> Option<u32> {
    Some(match name {
        "sunday" | "sun" => 0,
        "monday" | "mon" => 1,
        "tuesday" | "tue" | "tues" => 2,
        "wednesday" | "wed" => 3,
        "thursday" | "thu" | "thur" | "thurs" => 4,
        "friday" | "fri" => 5,
        "saturday" | "sat" => 6,
        _ => return None,
    })
}

pub fn local_tz() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into())
}

/// Offset of `tz` from UTC in minutes at `at`; `None` for an unknown zone.
pub fn tz_offset_minutes(tz: &str, at: DateTime<Utc>) -> Option<i32> {
    let tz: Tz = tz.parse().ok()?;
    Some(at.with_timezone(&tz).offset().fix().local_minus_utc() / 60)
}

fn cron_in(cron: &str) -> Option<Cron> {
    Cron::new(cron).parse().ok()
}

pub fn next_run(cron: &str, tz: &str, from: DateTime<Utc>) -> Option<i64> {
    let tz: Tz = tz.parse().ok()?;
    let next = cron_in(cron)?.find_next_occurrence(&from.with_timezone(&tz), false).ok()?;
    Some(next.timestamp_millis())
}

pub fn next_runs(cron: &str, tz: &str, count: usize, from: DateTime<Utc>) -> Vec<i64> {
    let (Ok(tz), Some(c)) = (tz.parse::<Tz>(), cron_in(cron)) else { return vec![] };
    let mut at = from.with_timezone(&tz);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let Ok(n) = c.find_next_occurrence(&at, false) else { return vec![] };
        out.push(n.timestamp_millis());
        at = n;
    }
    out
}

/// "Today 13:00", "Tomorrow 13:00" or "Thu 29 Oct 13:00" in `tz`.
pub fn format_when(ms: i64, tz: &str, now_ms: i64) -> String {
    let tz: Tz = tz.parse().unwrap_or(Tz::UTC);
    let at = |x: i64| DateTime::from_timestamp_millis(x).unwrap_or_default().with_timezone(&tz);
    let d = at(ms);
    let time = d.format("%H:%M").to_string();
    let key = |x: i64| at(x).date_naive();
    if key(ms) == key(now_ms) {
        format!("Today {time}")
    } else if key(ms) == key(now_ms + 86_400_000) {
        format!("Tomorrow {time}")
    } else {
        format!("{} {time}", d.format("%a %-d %b"))
    }
}

pub fn describe_next(cron: &str, tz: &str) -> String {
    match next_run(cron, tz, Utc::now()) {
        Some(n) => format_when(n, tz, Utc::now().timestamp_millis()),
        None => "not scheduled".into(),
    }
}

struct TimeOfDay {
    hour: u32,
    minute: u32,
    start: usize,
    end: usize,
}

fn meridiem(hour: u32, ampm: Option<&str>) -> u32 {
    match ampm {
        Some("pm") if hour < 12 => hour + 12,
        Some("am") if hour == 12 => 0,
        _ => hour,
    }
}

/// A clock time in `lower` ("at 9", "9:30", "8am") or a named one ("noon", "evening").
// ponytail: narrow time grammar instead of chrono-node, swap for a date crate if routines need more phrases
fn time_of(lower: &str) -> Option<TimeOfDay> {
    let clock = re!(r"\b(?:at\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)?|(\d{1,2}):(\d{2})\s*(am|pm)?|(\d{1,2})\s*(am|pm))\b");
    if let Some(c) = clock.captures(lower) {
        let m = c.get(0).unwrap();
        let num = |i: usize| c.get(i).and_then(|g| g.as_str().parse::<u32>().ok());
        let (h, min, ap) = if let Some(h) = num(1) {
            (h, num(2).unwrap_or(0), c.get(3))
        } else if let Some(h) = num(4) {
            (h, num(5).unwrap_or(0), c.get(6))
        } else {
            (num(7)?, 0, c.get(8))
        };
        if h <= 23 && min <= 59 {
            return Some(TimeOfDay { hour: meridiem(h, ap.map(|g| g.as_str())), minute: min, start: m.start(), end: m.end() });
        }
    }
    let named = re!(r"\b(noon|midday|midnight|morning|afternoon|evening|night)\b").captures(lower)?;
    let g = named.get(1)?;
    let hour = match g.as_str() {
        "noon" | "midday" => 12,
        "midnight" => 0,
        "morning" => 8,
        "afternoon" => 14,
        "evening" => 18,
        _ => 21,
    };
    Some(TimeOfDay { hour, minute: 0, start: g.start(), end: g.end() })
}

struct Event {
    event: &'static str,
    detail: String,
    text: String,
}

fn event_of(t: &str) -> Option<Event> {
    let ev = |event, detail: &str, text: &str| Some(Event { event, detail: detail.to_string(), text: text.to_string() });
    if let Some(m) = re!(r"(?i)\bon (?:a )?push(?:es)? to ([\w./-]+)").captures(t) {
        return ev("git.push", &m[1], &m[0]);
    }
    if let Some(m) = re!(r"(?i)\bon (?:a )?(?:new )?(?:pull request|pr|merge request)\b(?: (?:to|on) ([\w./-]+))?").captures(t) {
        return ev("git.pull_request", m.get(1).map_or("*", |g| g.as_str()), &m[0]);
    }
    if let Some(m) = re!(r"(?i)\bon commit\b").find(t) {
        return ev("git.commit", "*", m.as_str());
    }
    if let Some(m) = re!(r"(?i)\bwhen an? (?:new )?(e-?mail|message) (?:arrives|comes in)(?: from ([^\s,]+))?").captures(t) {
        let event = if m[1].to_lowercase().starts_with('e') { "email.received" } else { "message.received" };
        return ev(event, m.get(2).map_or("*", |g| g.as_str()), &m[0]);
    }
    if let Some(m) = re!(r"(?i)\bwhen (?:a )?files? (?:in |under )?([~\w./-]+)? ?changes?\b|\bwhen ([~\w./-]+) changes\b").captures(t) {
        let detail = m.get(1).or(m.get(2)).map_or(".", |g| g.as_str());
        return ev("file.changed", detail, &m[0]);
    }
    if let Some(m) = re!(r"(?i)\bon (?:a )?webhook\b(?: ([\w/-]+))?").captures(t) {
        return ev("webhook", m.get(1).map_or("default", |g| g.as_str()), &m[0]);
    }
    None
}

fn event_human(e: &Event) -> String {
    let d = &e.detail;
    match e.event {
        "git.push" => format!("On every push to {d}"),
        "git.pull_request" => format!("On every pull request{}", if d != "*" { format!(" to {d}") } else { String::new() }),
        "git.commit" => "On every commit".into(),
        "email.received" => format!("When an email arrives{}", if d != "*" { format!(" from {d}") } else { String::new() }),
        "message.received" => "When a message arrives".into(),
        "file.changed" => format!("When files in {d} change"),
        "webhook" => "When the webhook is called".into(),
        other => other.into(),
    }
}

fn fail(tz: &str, matched: Vec<String>, error: impl Into<String>) -> ParsedSchedule {
    ParsedSchedule {
        ok: false,
        kind: ScheduleKind::Repeat,
        cron: String::new(),
        tz: tz.into(),
        once: None,
        event: None,
        next: None,
        human: String::new(),
        matched,
        error: Some(error.into()),
    }
}

fn dedupe(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in v {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

fn repeat(tz: &str, now: DateTime<Utc>, matched: Vec<String>, cron: String, human: String) -> ParsedSchedule {
    match next_run(&cron, tz, now) {
        None => fail(tz, matched, "That schedule never runs."),
        Some(next) => ParsedSchedule {
            ok: true,
            kind: ScheduleKind::Repeat,
            cron,
            tz: tz.into(),
            once: None,
            event: None,
            next: Some(next),
            human,
            matched: dedupe(matched),
            error: None,
        },
    }
}

/// Local wall-clock time to UTC; a time that does not exist (DST gap) moves to the first one after it.
fn local_to_utc(tz: Tz, date: NaiveDate, hour: u32, minute: u32) -> Option<DateTime<Utc>> {
    let naive = date.and_hms_opt(hour, minute, 0)?;
    let at = tz.from_local_datetime(&naive).earliest().or_else(|| tz.from_local_datetime(&(naive + Duration::hours(1))).earliest())?;
    Some(at.with_timezone(&Utc))
}

pub fn parse_schedule(phrase: &str, tz: Option<&str>, now: DateTime<Utc>) -> ParsedSchedule {
    let tz_name = tz.map(str::to_string).unwrap_or_else(local_tz);
    let mut matched: Vec<String> = Vec::new();
    let t = phrase.trim().replace(['–', '—'], "-");
    if t.is_empty() {
        return fail(&tz_name, matched, "Write when it should run, e.g. “every weekday at 9”.");
    }
    let Ok(tzv) = tz_name.parse::<Tz>() else {
        return fail(&tz_name, matched, format!("Unknown time zone “{tz_name}”."));
    };
    let lower = t.to_ascii_lowercase();

    if let Some(ev) = event_of(&t) {
        let human = event_human(&ev);
        matched.push(ev.text);
        return ParsedSchedule {
            ok: true,
            kind: ScheduleKind::Event,
            cron: String::new(),
            tz: tz_name,
            once: None,
            event: Some(EventTrigger { event: ev.event.into(), detail: ev.detail }),
            next: None,
            human,
            matched,
            error: None,
        };
    }

    let tm = time_of(&lower);
    let hour = tm.as_ref().map_or(9, |x| x.hour);
    let minute = tm.as_ref().map_or(0, |x| x.minute);
    let hhmm = format!("{hour:02}:{minute:02}");
    let tm_text = tm.as_ref().map(|x| t[x.start..x.end].to_string());
    let is_repeat = re!(r"\b(every|each|daily|weekly|monthly|hourly|weekdays?|weekends?|mondays|tuesdays|wednesdays|thursdays|fridays|saturdays|sundays)\b").is_match(&lower);

    // Day-of-week set.
    let mut dow = "*".to_string();
    let mut dow_human = String::new();
    if let Some(m) = re!(r"\bweekdays?\b|\bmon(?:day)?\s*-\s*fri(?:day)?\b").find(&lower) {
        dow = "1-5".into();
        dow_human = "weekdays".into();
        matched.push(if m.as_str().starts_with("weekday") { m.as_str().into() } else { "mon-fri".into() });
    } else if re!(r"\bweekends?\b").is_match(&lower) {
        dow = "0,6".into();
        dow_human = "weekends".into();
        matched.push("weekends".into());
    } else {
        let days: Vec<&str> = re!(r"\b(sundays?|mondays?|tuesdays?|wednesdays?|thursdays?|fridays?|saturdays?|sun|mon|tue|tues|wed|thu|thur|thurs|fri|sat)\b")
            .find_iter(&lower)
            .map(|m| m.as_str())
            .collect();
        let mut nums: Vec<u32> = days.iter().filter_map(|d| day_number(d).or_else(|| day_number(d.trim_end_matches('s')))).collect();
        nums.sort_unstable();
        nums.dedup();
        if !nums.is_empty() && is_repeat {
            dow = nums.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            dow_human = nums.iter().map(|n| DAY_NAMES[*n as usize]).collect::<Vec<_>>().join(", ");
            matched.extend(days.iter().map(|d| d.to_string()));
        }
    }
    let on_dow = if dow_human.is_empty() { String::new() } else { format!(" on {dow_human}") };

    // Every N minutes / hours, optionally inside an hour window.
    let every_n = re!(r"\bevery\s+(\d+)\s*(minutes?|mins?|hours?|hrs?|h)\b").captures(&lower);
    let window = re!(r"\b(\d{1,2})(?::00)?\s*-\s*(\d{1,2})(?::00)?\b").captures(&lower);
    if every_n.is_some() || re!(r"\bevery hour\b|\bhourly\b|\bevery minute\b").is_match(&lower) {
        let n: u32 = every_n.as_ref().and_then(|c| c[1].parse().ok()).unwrap_or(1);
        let minutes = match &every_n {
            Some(c) => c[2].starts_with('m'),
            None => lower.contains("minute"),
        };
        matched.push(match &every_n {
            Some(c) => c[0].to_string(),
            None if lower.contains("hourly") => "hourly".into(),
            None => "every hour".into(),
        });
        let win = window.as_ref().map(|w| (w[1].parse::<u32>().unwrap_or(0), w[2].parse::<u32>().unwrap_or(0)));
        if let Some(w) = &window {
            matched.push(w[0].to_string());
        }
        if minutes {
            if !(1..=59).contains(&n) {
                return fail(&tz_name, matched, "Minutes must be between 1 and 59.");
            }
            let hours = win.map_or("*".to_string(), |(a, b)| format!("{a}-{b}"));
            let between = win.map_or(String::new(), |(a, b)| format!(" between {a}:00 and {b}:59"));
            let plural = if n == 1 { "" } else { "s" };
            return repeat(&tz_name, now, matched, format!("*/{n} {hours} * * {dow}"), format!("Every {n} minute{plural}{between}{on_dow}"));
        }
        if !(1..=23).contains(&n) {
            return fail(&tz_name, matched, "Hours must be between 1 and 23.");
        }
        let hours = win.map_or(format!("*/{n}"), |(a, b)| format!("{a}-{b}/{n}"));
        let from_to = win.map_or(String::new(), |(a, b)| format!(" from {a}:00 to {b}:00"));
        let every = if n == 1 { "Every hour".to_string() } else { format!("Every {n} hours") };
        return repeat(&tz_name, now, matched, format!("{minute} {hours} * * {dow}"), format!("{every}{from_to}{on_dow}"));
    }

    // Monthly: "first day of each month", "the 15th of every month", "monthly".
    let first_day = re!(r"\bfirst day of (?:each|every|the) month\b").find(&lower);
    let nth = re!(r"\b(?:on )?the (\d{1,2})(?:st|nd|rd|th)?(?: day)? of (?:each|every|the) month\b|\bevery month on the (\d{1,2})(?:st|nd|rd|th)?\b").captures(&lower);
    if first_day.is_some() || nth.is_some() || re!(r"\bmonthly\b").is_match(&lower) {
        let dom: u32 = if first_day.is_some() {
            1
        } else if let Some(c) = &nth {
            c.get(1).or(c.get(2)).and_then(|g| g.as_str().parse().ok()).unwrap_or(1)
        } else {
            1
        };
        if !(1..=31).contains(&dom) {
            return fail(&tz_name, matched, "Day of month must be between 1 and 31.");
        }
        matched.push(match (&first_day, &nth) {
            (Some(m), _) => m.as_str().into(),
            (None, Some(c)) => c[0].into(),
            _ => "monthly".into(),
        });
        matched.extend(tm_text.clone());
        return repeat(&tz_name, now, matched, format!("{minute} {hour} {dom} * *"), format!("On day {dom} of every month at {hhmm}"));
    }

    // Daily, weekly and day-of-week repeats.
    if is_repeat {
        if let Some(m) = re!(r"\b(every day|each day|daily|every morning|every evening|every night|every afternoon)\b").find(&lower) {
            matched.push(m.as_str().into());
        }
        if dow == "*" && re!(r"\b(every week|weekly)\b").is_match(&lower) {
            let today = now.with_timezone(&tzv).weekday().num_days_from_sunday() as usize;
            dow = today.to_string();
            dow_human = DAY_NAMES[today].into();
            matched.push("weekly".into());
        }
        matched.extend(tm_text.clone());
        let who = if dow_human.is_empty() { "Every day".to_string() } else { format!("Every {dow_human}") };
        return repeat(&tz_name, now, matched, format!("{minute} {hour} * * {dow}"), format!("{who} at {hhmm}"));
    }

    // One-off: tomorrow at 9, next Monday at 10, in 3 hours.
    // ponytail: narrow date grammar, swap for a date crate if routines need more phrases
    let date = re!(r"\b(tomorrow|today|next\s+(sunday|monday|tuesday|wednesday|thursday|friday|saturday|sun|mon|tues|tue|wed|thurs|thur|thu|fri|sat)|in\s+(\d+)\s*(minutes?|mins?|hours?|hrs?|days?|weeks?))\b").captures(&lower);
    let Some(d) = date else {
        return fail(&tz_name, matched, "I couldn’t find a time in that. Try “every weekday at 9” or “tomorrow at 10”.");
    };
    let word = d.get(1).unwrap().as_str();
    let today = now.with_timezone(&tzv).date_naive();
    let when = if let Some(n) = d.get(3) {
        let n: i64 = n.as_str().parse().unwrap_or(0);
        let unit = d.get(4).unwrap().as_str();
        let delta = if unit.starts_with('m') {
            Duration::minutes(n)
        } else if unit.starts_with('h') {
            Duration::hours(n)
        } else if unit.starts_with('d') {
            Duration::days(n)
        } else {
            Duration::weeks(n)
        };
        matched.push(d.get(0).unwrap().as_str().to_string());
        Some(now + delta)
    } else {
        let date = if word == "today" {
            today
        } else if word == "tomorrow" {
            today + Duration::days(1)
        } else {
            let target = day_number(d.get(2).unwrap().as_str()).unwrap_or(0) as i64;
            let current = today.weekday().num_days_from_sunday() as i64;
            let ahead = (target - current).rem_euclid(7);
            today + Duration::days(if ahead == 0 { 7 } else { ahead })
        };
        let (h, m) = tm.as_ref().map_or((12, 0), |x| (x.hour, x.minute));
        // One matched span for "tomorrow at 9" when the time follows the date directly.
        let dm = d.get(0).unwrap();
        match &tm {
            Some(x) if x.start >= dm.end() && lower[dm.end()..x.start].trim().is_empty() => matched.push(t[dm.start()..x.end].to_string()),
            _ => {
                matched.push(t[dm.start()..dm.end()].to_string());
                matched.extend(tm_text);
            }
        }
        local_to_utc(tzv, date, h, m)
    };
    let Some(when) = when else {
        return fail(&tz_name, matched, "That time does not exist in this time zone.");
    };
    if when <= now {
        return fail(&tz_name, matched, "That time has already passed.");
    }
    let local = when.with_timezone(&tzv);
    let ms = when.timestamp_millis();
    ParsedSchedule {
        ok: true,
        kind: ScheduleKind::Once,
        cron: format!("{} {} {} {} *", local.minute(), local.hour(), local.day(), local.month()),
        tz: tz_name.clone(),
        once: Some(ms),
        event: None,
        next: Some(ms),
        human: format!("Once, {}", format_when(ms, &tz_name, now.timestamp_millis())),
        matched,
        error: None,
    }
}
