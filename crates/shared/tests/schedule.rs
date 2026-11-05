use chrono::{DateTime, TimeZone, Utc};
use workspaceai_shared::schedule::*;

const TZ: &str = "Europe/Warsaw";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 29, 10, 0, 0).unwrap() // a Thursday
}

fn iso(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse(phrase: &str) -> ParsedSchedule {
    parse_schedule(phrase, Some(TZ), now())
}

#[test]
fn tomorrow_at_9_is_once() {
    let r = parse("tomorrow at 9");
    assert!(r.ok);
    assert_eq!(r.kind, ScheduleKind::Once);
    assert_eq!(r.cron, "0 9 30 10 *");
    assert_eq!(iso(r.next.unwrap()), "2026-10-30T08:00:00Z");
    assert!(r.matched.iter().any(|m| m == "tomorrow at 9"));
}

#[test]
fn every_2_hours_9_18_weekdays() {
    let r = parse("every 2 hours 9–18 weekdays");
    assert!(r.ok);
    assert_eq!(r.cron, "0 9-18/2 * * 1-5");
    assert_eq!(iso(r.next.unwrap()), "2026-10-29T12:00:00Z"); // 13:00 Warsaw, next slot after 11:00
    assert!(r.human.contains("Every 2 hours from 9:00 to 18:00 on weekdays"), "{}", r.human);
}

#[test]
fn first_day_of_each_month_at_noon() {
    let r = parse("first day of each month at noon");
    assert_eq!(r.cron, "0 12 1 * *");
    assert_eq!(iso(r.next.unwrap()), "2026-11-01T11:00:00Z");
}

#[test]
fn every_weekday_at_930() {
    assert_eq!(parse("every weekday at 9:30").cron, "30 9 * * 1-5");
}

#[test]
fn mondays_and_thursdays_at_10() {
    assert_eq!(parse("mondays and thursdays at 10").cron, "0 10 * * 1,4");
}

#[test]
fn weekends_at_8am() {
    assert_eq!(parse("every weekend at 8am").cron, "0 8 * * 0,6");
}

#[test]
fn every_15_minutes() {
    assert_eq!(parse("every 15 minutes").cron, "*/15 * * * *");
}

#[test]
fn next_monday_at_10_is_once() {
    let r = parse("next Monday at 10");
    assert_eq!(r.kind, ScheduleKind::Once);
    assert_eq!(iso(r.next.unwrap()), "2026-11-02T09:00:00Z");
}

#[test]
fn in_3_hours_is_once() {
    let r = parse("in 3 hours");
    assert_eq!(r.kind, ScheduleKind::Once);
    assert_eq!(r.next, Some(now().timestamp_millis() + 3 * 3_600_000));
}

#[test]
fn push_event() {
    let r = parse("on push to main");
    assert_eq!(r.event, Some(EventTrigger { event: "git.push".into(), detail: "main".into() }));
}

#[test]
fn email_event() {
    assert_eq!(parse("when an email arrives").event.unwrap().event, "email.received");
}

#[test]
fn rejects_nonsense() {
    assert!(!parse("whenever you feel like it").ok);
}

#[test]
fn empty_input_fails_with_hint() {
    for s in ["", "   "] {
        let r = parse(s);
        assert!(!r.ok);
        assert!(r.error.unwrap().starts_with("Write when it should run"));
    }
}

#[test]
fn unknown_timezone_fails_not_panics() {
    let r = parse_schedule("every day at 9", Some("Not/AZone"), now());
    assert!(!r.ok);
    assert!(r.error.is_some());
}

#[test]
fn dst_gap_still_has_next() {
    // 2026-03-29 02:30 does not exist in Warsaw.
    let day_before = Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).unwrap();
    let r = parse_schedule("every day at 2:30", Some(TZ), day_before);
    assert!(r.ok, "{:?}", r.error);
    assert!(r.next.is_some());
}

#[test]
fn format_when_labels_today_and_tomorrow() {
    let ms = Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap().timestamp_millis(); // 13:00 Warsaw
    assert_eq!(format_when(ms, TZ, now().timestamp_millis()), "Today 13:00");
    assert_eq!(format_when(ms + 86_400_000, TZ, now().timestamp_millis()), "Tomorrow 13:00");
    assert_eq!(format_when(ms + 3 * 86_400_000, TZ, now().timestamp_millis()), "Sun 1 Nov 13:00");
}
