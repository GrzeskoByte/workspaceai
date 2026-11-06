//! Port of `packages/shared/test/sheet.test.ts` (action sheets read an agent's /usage and /context text).
use workspaceai_shared::sheet::{read_sheet, Meter, RankedRow};

const USAGE: &str = "You are currently using your subscription to power your Claude Code usage

Current session: 67% used · resets Oct 30, 9:20pm (Europe/Madrid)
Current week (all models): 19% used · resets Nov 6, 10am (Europe/Madrid)

What's contributing to your usage?
Approximate · sessions on this machine only · shares overlap

Last 24h · 870 requests · 2 sessions
  90%  Start fresh when you switch tasks · run /clear
  60%  Stop sessions you no longer need · some ran 8 hours or more
  Top skills: /loop 22%";

const CONTEXT: &str = "## Context Usage

**Model:** claude-opus-5-5  
**Tokens:** 24.9k / 1m (2%)

### Estimated usage by category

| Category | Tokens | Percentage |
|----------|--------|------------|
| System prompt | 2.8k | 0.3% |
| Messages | 10 | 0.0% |
| Free space | 942.1k | 94.2% |

### MCP Tools

| Tool | Server | Tokens |
|------|--------|--------|
| mcp__a__b | a | 154 |";

fn meter(label: &str, pct: f64, sub: &str) -> Meter {
    Meter { label: label.into(), pct, sub: Some(sub.into()) }
}

#[test]
fn reads_plan_usage_into_meters_and_ranked_tips() {
    let r = read_sheet(USAGE);
    assert_eq!(r.intro, vec!["You are currently using your subscription to power your Claude Code usage"]);
    assert_eq!(
        r.meters,
        vec![
            meter("Current session", 67.0, "resets Oct 30, 9:20pm (Europe/Madrid)"),
            meter("Current week (all models)", 19.0, "resets Nov 6, 10am (Europe/Madrid)"),
        ]
    );
    assert_eq!(r.ranked.len(), 1);
    assert_eq!(r.ranked[0].heading, "Last 24h · 870 requests · 2 sessions");
    let row = |pct: f64, text: &str| RankedRow { pct, text: text.into() };
    assert_eq!(
        r.ranked[0].rows,
        vec![
            row(90.0, "Start fresh when you switch tasks · run /clear"),
            row(60.0, "Stop sessions you no longer need · some ran 8 hours or more"),
            row(-1.0, "Top skills: /loop 22%"),
        ]
    );
}

#[test]
fn reads_the_context_window_and_its_categories() {
    let r = read_sheet(CONTEXT);
    assert_eq!(r.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(r.meters, vec![meter("Context window", 2.0, "24.9k of 1m tokens")]);
    let cats: Vec<(&str, f64)> = r.categories.iter().map(|c| (c.name.as_str(), c.pct)).collect();
    assert_eq!(cats, vec![("System prompt", 0.3), ("Messages", 0.0), ("Free space", 94.2)]);
    assert!(r.rest.contains("### MCP Tools")); // the rest stays available as written
    assert!(!r.rest.contains("Context Usage"));
}
