# TS → Rust test matrix

Every TypeScript test gets a Rust counterpart, or is marked N/A with a reason. Rust tests live in `crates/<name>/tests/`.

| TS test | Rust test | Status |
|---|---|---|
| `shared/test/schedule.test.ts` › tomorrow at 9 | `schedule.rs::tomorrow_at_9_is_once` | ported |
| › every 2 hours 9–18 weekdays | `every_2_hours_9_18_weekdays` | ported |
| › first day of each month at noon | `first_day_of_each_month_at_noon` | ported |
| › every weekday at 9:30 | `every_weekday_at_930` | ported |
| › mondays and thursdays at 10 | `mondays_and_thursdays_at_10` | ported |
| › weekends at 8am | `weekends_at_8am` | ported |
| › every 15 minutes | `every_15_minutes` | ported |
| › next Monday at 10 | `next_monday_at_10_is_once` | ported |
| › in 3 hours | `in_3_hours_is_once` | ported |
| › events | `push_event`, `email_event` | ported |
| › rejects nonsense | `rejects_nonsense` | ported |
| `shared/test/commands.test.ts` › @ references (finds) | `commands.rs::mention_at_finds_the_reference_being_typed` | ported |
| › @ references (inserts) | `insert_mention_says_where_the_caret_goes` | ported |
| › maps each harness command | `command_action_maps_each_harness_command_to_the_agents_equivalent` | ported |
| › lists harness commands first | `chat_commands_list_harness_first_then_uncovered_agent_commands` | ported |
| › parses a command and its arguments | `parse_slash_splits_command_and_arguments` | ported |
| `shared/test/sheet.test.ts` › reads plan usage into meters and ranked tips | `sheet.rs::reads_plan_usage_into_meters_and_ranked_tips` | ported |
| › reads the context window and its categories | `reads_the_context_window_and_its_categories` | ported |
| `shared/test/markdown.test.ts` (lists ×5, links ×3, gitlabMarkdown ×2) | `markdown.rs::lists_*`, `links_*`, `gitlab_markdown_*` | ported |
| `shared/test/claude-design-system.test.ts` (×6) | `claude_design_system.rs::*` (same names) | ported |
| `shared/test/scroll.test.ts` (×4) | `util.rs::follows_bottom_*` | ported |
| `shared/test/session-reads.test.ts` | — | N/A: `readsFromTools` reads chat tool rows; sessions are terminals now |
| `orchestrator/test/bpmn.test.ts` | — | N/A: BPMN deferred (design doc, Non-goals) |
| `supervisor/test/graphfiles.test.ts`, `graphify.test.ts`, `graphrun.test.ts`, `maps.test.ts` | — | N/A: graph backends not ported in v1 |
| other `supervisor/test/*`, `orchestrator/test/rules.test.ts` | — | pending (later plans) |
