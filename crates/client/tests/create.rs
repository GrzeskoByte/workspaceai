use serde_json::json;
use workspaceai_client::create::*;
use workspaceai_client::workspace::DesignSystem;

fn ds(id: &str, title: &str, summary: &str) -> DesignSystem {
    DesignSystem { id: id.into(), title: title.into(), summary: summary.into(), swatches: vec![], source: String::new() }
}

#[test]
fn start_needs_a_brief_and_a_project_and_defaults_the_title_to_the_type() {
    assert_eq!(start_body("p", "Doc", "", "  "), None);
    assert_eq!(start_body("", "Doc", "", "x"), None);
    assert_eq!(start_body("p", "Deck", " ", " Six slides "), Some(json!({ "project": "p", "type": "Deck", "title": "Deck", "brief": "Six slides" })));
}

#[test]
fn directions_carry_the_answers_as_q_a_pairs() {
    let b = directions_body("Doc", "T", "b", &[("Who?".into(), "Me".into())]);
    assert_eq!(b["answers"], json!([{ "q": "Who?", "a": "Me" }]));
}

#[test]
fn replies_parse_and_tolerate_junk() {
    assert_eq!(parse_questions(&json!({ "questions": ["a", 3, "b"] })), vec!["a", "b"]);
    assert!(parse_questions(&json!({})).is_empty());
    let d = parse_directions(&json!({ "directions": [{ "name": "Warm", "summary": "s", "tone": "calm" }] }));
    assert_eq!(d[0].tone, "calm");
    let c = parse_critique(&json!({ "scores": { "designer": 9 }, "issues": [{ "role": "a11y", "issue": "i", "fix": "f" }], "composite": 8.2, "threshold": 8, "passed": true, "roles": [{ "id": "designer", "name": "Designer", "scores": "x", "weight": 0.3 }] })).unwrap();
    assert!(c.passed && c.score("designer") == 9.0 && c.score("critic") == 0.0);
}

#[test]
fn tweaks_become_one_issue_and_blank_ones_are_ignored() {
    assert!(tweak_issues("  ").is_none());
    let i = tweak_issues(" Shorter ").unwrap();
    assert_eq!(fix_body("a1", &i)["issues"][0], json!({ "role": "tweak", "issue": "Requested change", "fix": "Shorter" }));
}

#[test]
fn systems_filter_by_title_and_summary_and_pick_the_workspace_one() {
    let all = vec![ds("a", "Warm", "editorial"), ds("b", "Cool", "dashboards")];
    assert_eq!(filter_systems(&all, "DASH").len(), 1);
    assert_eq!(filter_systems(&all, "").len(), 2);
    assert_eq!(initial_system(&all, "Cool"), "b");
    assert_eq!(initial_system(&all, "Gone"), DEFAULT_SYSTEM);
}

#[test]
fn stages_unlock_up_to_the_current_one_and_after_generation() {
    assert!(stage_enabled(1, 1, false) && !stage_enabled(2, 1, false));
    assert!(stage_enabled(6, 4, true) && !stage_enabled(3, 1, true));
    assert_eq!(score_tone(8.0), 0x7bd88f);
    assert_eq!(step_path("w s", "c", "generate"), "/api/ws/w%20s/pipeline/c/generate");
}
