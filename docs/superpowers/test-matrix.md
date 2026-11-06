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
| `supervisor/test/core.test.ts` › rejects requests without the per-launch token | — | N/A here: in-process `handle()` has no token; loopback token lands in B9 |
| `supervisor/test/core.test.ts` › creates workspaces; organizations only join through SSO | `core.rs::creates_workspaces_and_organizations_only_join_through_sso` | ported |
| › keeps everything per workspace (R-06) | `core.rs::keeps_everything_per_workspace` | ported (projects/facts/secrets/config paths; `POST providers`/`PUT secrets` routes are B3) |
| › org workspace offers only policy providers, locks changes, blocks org defaultModel | `security.rs::org_workspace_offers_only_policy_providers_and_locks_changes` | ported (live `/models` is B3) |
| › a policy that fails verification blocks sending (R-36) | `security.rs::a_policy_that_fails_verification_blocks_sending_until_refreshed` | ported (`send_allowed` directly; chats route is B5) |
| › writes custom providers to opencode.json with key references only | — | pending B3 (`POST /api/ws/:id/providers`, `engineEnv`); `build_opencode_json` itself is B1 |
| › nests sub-projects / layers instructions / deletes a project subtree / exports without secrets | — | pending B4 |
| › chats are opencode sessions; a chat is never stranded | — | pending B5 |
| › switching workspaces keeps the other running / managed server loopback+password | — | pending B3 |
| `supervisor/test/security.test.ts` › provider key lives in the OS keychain, per workspace, nowhere on disk | `keychain.rs::keychain_memory_round_trip_is_per_workspace`, `keychain_os_backend_round_trip_or_skip` | ported (keychain layer; provider route is B3) |
| › remote opencode must be HTTPS or loopback | — | pending B3 (`remoteUrlAllowed`) |
| › org uploads its audit log without content unless policy allows (R-36) | `audit.rs::org_uploads_new_audit_entries_only_and_withholds_content_without_log_content` | ported |
| › the supervisor API only listens on loopback and needs its token | — | N/A here: no listener in B2; loopback + token is B9 |
| `supervisor/test/core.test.ts` › workspace patch (allowed keys, unknown key, org locks) | `core.rs::workspace_patch_updates_allowed_fields_and_snapshot`, `workspace_patch_rejects_unknown_keys` | ported |
| › facts add/update/remove/clear, memory-changed, audit list | `core.rs::facts_lifecycle_and_delete_needs_confirm`, `memory_changed_is_audited_and_emits_nothing_surprising`, `audit_route_returns_events_newest_first_with_limit` | ported (bulk clear needs `confirm: true`: deviation) |
| › the only personal workspace cannot be deleted | `core.rs::the_last_personal_workspace_cannot_be_deleted` | ported |
| › `/api/settings` patch and workspace activate | `core.rs::settings_patch_and_activate` | ported |
| › unknown route/method | `core.rs::unknown_routes_and_methods_are_404` | ported |
| `supervisor/src/policy.ts` `canonical`/`signPolicy`/`verifyPolicy` (no TS test existed) | `policy.rs::js_numbers_match_json_stringify`, `canonical_sorts_keys_like_js_and_keeps_strings_escaped`, `ts_signed_policy_verifies_in_rust`, `rust_signed_policy_verifies_in_ts` | ported + cross-language fixtures (B2 acceptance) |
| › ids that are empty or dot segments | `security.rs::valid_id_rejects_path_like_segments`, `workspaces_without_harness_json_are_ignored` | ported (known debt closed) |
| other `supervisor/test/*`, `orchestrator/test/rules.test.ts` | — | pending (later plans) |
