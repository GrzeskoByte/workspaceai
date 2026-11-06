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
| › keeps everything per workspace (R-06) | `core.rs::keeps_everything_per_workspace`, `secrets.rs::a_secret_round_trips_through_the_api_without_ever_being_echoed` | ported |
| › org workspace offers only policy providers, locks changes, blocks org defaultModel | `security.rs::org_workspace_offers_only_policy_providers_and_locks_changes`, `secrets.rs::an_org_that_blocks_personal_keys_refuses_them_unless_marked_org`, `providers.rs::an_org_workspace_cannot_change_providers` | ported |
| › a policy that fails verification blocks sending (R-36) | `security.rs::a_policy_that_fails_verification_blocks_sending_until_refreshed` | ported (`send_allowed` directly; chats route is B5) |
| › writes custom providers to opencode.json with key references only | `providers.rs::a_custom_provider_is_saved_as_an_env_reference_and_can_be_tested`, `engine.rs::opencode_json_reports_the_path_the_builder_and_the_written_text` | ported |
| › nests sub-projects to any depth | `projects.rs::nests_sub_projects_to_any_depth_and_refuses_a_move_inside_itself` | ported |
| › layers instructions broad to narrow, inheritance off | `projects.rs::layers_go_broad_to_narrow_and_inheritance_can_be_turned_off` | ported |
| › a chat is never stranded (project rules: General protected, archive/restore) | `projects.rs::the_general_project_cannot_be_archived_deleted_or_moved`, `archive_hides_the_subtree_and_its_chats_and_restore_brings_them_back` | ported (chat creation is B5) |
| › deleting a project removes the whole sub-tree | `projects.rs::deleting_a_project_removes_the_whole_subtree_but_keeps_the_folders` | ported (no engine-side chat delete; chats' records go) |
| › exports a workspace without secrets | `export.rs::exports_config_projects_and_memory_but_never_secrets`, `exporting_a_workspace_without_projects_still_works` | ported (+ credential-file/private-key hardening) |
| › instructions persist and do not rewrite opencode.json | `projects.rs::instructions_persist_and_do_not_rewrite_opencode_json` | ported |
| `supervisor/test/memory.test.ts` › facts per scope, approve/forget, nothing crosses workspaces | `workspace_api.rs::facts_lifecycle_and_delete_needs_confirm`, `core.rs::keeps_everything_per_workspace` | ported in B2 |
| › the agent's memory MCP server (stdio) reads approved facts | — | N/A here: MCP servers are B9 |
| `supervisor/test/recall.test.ts` › looks up the words that matter | `recall.rs::looks_up_the_words_that_matter` | ported (`queryTerms`; `rankNodes` is graph-only) |
| › keeps the recall block out of the transcript | `recall.rs::keeps_the_recall_block_out_of_the_transcript` | ported |
| › tells agents to use the tools their memory has | `shared/defaults.rs::memory_rule_*` | ported in B1 |
| › lean-ctx config / signed-out detection | — | N/A: leanctx is B12, sign-in B11 |
| `supervisor/test/files.test.ts` (6 cases: files/folders, folder completion, cap, gitignore, missing folder) | `files.rs::offers_files_and_folders_best_match_first_and_skips_dependencies`, `completes_inside_a_folder_already_typed_in_full_and_caps_the_results`, `a_folder_that_does_not_exist_returns_nothing`, `honours_gitignore_in_a_repository` | ported (git case skips with a printed reason without git) |
| `supervisor/test/formats.test.ts` › picks Markdown or HTML / offers the exports | `formats.rs::picks_markdown_or_html_per_kind`, `offers_the_exports_each_format_can_give` | ported |
| › Markdown tables / deck skeleton / Open Design page | — | deferred to B9: `markdownDeck`/`toHtml` need Open Design |
| `supervisor/test/permissions.test.ts` (project rules) | — | N/A: no engine permission layer; terminal sessions ask in their TUI |
| › chats are opencode sessions; a chat is never stranded | — | pending B5 |
| › switching workspaces keeps the other running / managed server loopback+password | `engine.rs::two_workspaces_get_their_own_xdg_dirs_and_ports`, `the_server_password_lives_in_the_env_and_never_in_argv` | ported |
| `supervisor/test/security.test.ts` › provider key lives in the OS keychain, per workspace, nowhere on disk | `keychain.rs::keychain_memory_round_trip_is_per_workspace`, `keychain_os_backend_round_trip_or_skip` | ported (keychain layer; provider route is B3) |
| › remote opencode must be HTTPS or loopback | `engine.rs::remote_servers_must_be_https_or_loopback`, `providers.rs::connection_tests_reject_plain_http_remote_hosts_and_web_pages` | ported (+ userinfo rejected: deviation) |
| › org uploads its audit log without content unless policy allows (R-36) | `audit.rs::org_uploads_new_audit_entries_only_and_withholds_content_without_log_content` | ported |
| › the supervisor API only listens on loopback and needs its token | — | N/A here: no listener in B2; loopback + token is B9 |
| `supervisor/test/core.test.ts` › workspace patch (allowed keys, unknown key, org locks) | `core.rs::workspace_patch_updates_allowed_fields_and_snapshot`, `workspace_patch_rejects_unknown_keys` | ported |
| › facts add/update/remove/clear, memory-changed, audit list | `core.rs::facts_lifecycle_and_delete_needs_confirm`, `memory_changed_is_audited_and_emits_nothing_surprising`, `audit_route_returns_events_newest_first_with_limit` | ported (bulk clear needs `confirm: true`: deviation) |
| › the only personal workspace cannot be deleted | `core.rs::the_last_personal_workspace_cannot_be_deleted` | ported |
| › `/api/settings` patch and workspace activate | `core.rs::settings_patch_and_activate` | ported |
| › unknown route/method | `core.rs::unknown_routes_and_methods_are_404` | ported |
| `supervisor/src/policy.ts` `canonical`/`signPolicy`/`verifyPolicy` (no TS test existed) | `policy.rs::js_numbers_match_json_stringify`, `canonical_sorts_keys_like_js_and_keeps_strings_escaped`, `ts_signed_policy_verifies_in_rust`, `rust_signed_policy_verifies_in_ts` | ported + cross-language fixtures (B2 acceptance) |
| › ids that are empty or dot segments | `security.rs::valid_id_rejects_path_like_segments`, `workspaces_without_harness_json_are_ignored` | ported (known debt closed) |
| `supervisor/test/engine.test.ts` › installs/uses a pinned binary | `engine.rs::a_pin_that_is_not_installed_is_refused_then_the_pinned_binary_runs` | ported (skips without `opencode`) |
| › settings changes restart the managed server | `engine.rs::settings_changes_reach_the_engine` | ported (skips without `opencode`) |
| › shutdown stops every managed server | `engine.rs::shutdown_stops_every_managed_server_and_is_idempotent` | ported (skips without `opencode`) |
| `supervisor/test/providers.test.ts` › web providers table | `providers.rs::web_providers_match_the_shared_table` | ported |
| `supervisor/test/mcp.test.ts` › add/toggle/delete a connector | `providers.rs::mcp_connectors_add_toggle_and_delete` | ported |
| `supervisor/test/secrets.test.ts` › normalised names, audited puts | `secrets.rs::names_are_normalised_and_puts_are_audited` | ported |
| `supervisor/test/*` (Open Design part of `shutdown.test.ts`) | — | N/A: Open Design install/health lands in B10 |
| other `supervisor/test/*`, `orchestrator/test/rules.test.ts` | — | pending (later plans) |
