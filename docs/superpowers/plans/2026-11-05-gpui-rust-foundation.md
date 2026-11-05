# GPUI Rust Rewrite: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the Rust workspace, settle whether a web view can live inside a GPUI window (phase 0), and port the first two `shared` modules (`schedule`, `commands`) with their tests.

**Architecture:** Cargo workspace at the repo root with one library crate per TS package (`workspaceai-shared` first). The spike is a throwaway crate, kept out of the workspace members that ship. Each ported module keeps the TS module's public function names in snake_case and its test file as the spec.

**Tech Stack:** Rust (stable), `chrono`, `chrono-tz`, `croner` (cron), `serde`, `regex`; `gpui` and `wry` for the spike only.

**Spec:** `docs/superpowers/specs/2026-11-05-gpui-rust-rewrite-design.md`

**Scope of this plan:** phase 0 plus the start of phase 1. Later plans, one each: rest of `shared` + `supervisor` core; GPUI shell and core views; remaining views + orchestrator runner; embedded surfaces and packaging.

## Global Constraints

- One Rust workspace, one desktop binary; no Tauri, React, Vite (spec Goals).
- `apps/desktop/src-tauri` and the TS packages stay in the repo until phase 3 parity; the new workspace must not break `npm` workspaces or `tauri dev`.
- Linux is the dev platform; macOS must stay possible; Windows best effort.
- Every ported TS test gets a Rust counterpart, or is marked N/A with a reason in `docs/superpowers/test-matrix.md`.
- Graph, BPMN (`bpmnPreview`, orchestrator `layout`/`lint`) are not ported.
- Keep today's on-disk formats and keychain names where free (spec call).

## Review Focus

- Schedule phrase with non-ASCII text (`“every weekday at 9”`, `9–18` en dash) must parse, not panic. Test pinned in Task 3.
- DST day: "every day at 2:30" in `Europe/Warsaw` on 2026-03-29 (nonexistent local time) must still return a `next`, not `None`. Task 3.
- Unknown timezone string must return `ok == false` with an error, not panic. Task 3.
- `mention_at` / `insert_mention` with multi-byte characters before the caret must not slice inside a char. Task 4.
- Empty or whitespace-only input to `parse_schedule` and `parse_slash` must return the failure/`None` result. Task 3, Task 4.

---

### Task 1: Cargo workspace scaffold

**Files:**
- Create: `Cargo.toml` (root), `crates/shared/Cargo.toml`, `crates/shared/src/lib.rs`, `rust-toolchain.toml`
- Modify: `.gitignore` (add `/target`)

**Interfaces:**
- Produces: workspace with member `crates/shared` (package `workspaceai-shared`, lib `workspaceai_shared`), empty module list in `lib.rs`.

- [ ] **Step 1: Write the check.** `crates/shared/src/lib.rs` contains `#[test] fn crate_builds() {}`.
- [ ] **Step 2: Create root `Cargo.toml`** with `[workspace] resolver = "2"`, `members = ["crates/*"]`, `exclude = ["apps/desktop/src-tauri", "crates/spike"]`. The exclude is required: the Tauri crate lives under the root and is not a member, and the spike pulls git deps that must not slow normal builds.
- [ ] **Step 3: Create `crates/shared`** (edition 2021) with deps `chrono`, `chrono-tz`, `croner`, `regex`, `serde` (derive).
- [ ] **Step 4: Verify.** Run `cargo test -p workspaceai-shared`. Expected: 1 passed. Run `cd apps/desktop/src-tauri && cargo metadata --no-deps -q >/dev/null`. Expected: exit 0 (Tauri crate still resolves on its own).
- [ ] **Step 5: Commit** `build: add Cargo workspace with shared crate`.

---

### Task 2: Phase 0 spike, wry web view inside a GPUI window (throwaway)

**Files:**
- Create: `crates/spike/Cargo.toml`, `crates/spike/src/main.rs`, `docs/superpowers/spike-webview.md`

**Interfaces:**
- Produces: `docs/superpowers/spike-webview.md` with a go/no-go per session type (X11, Wayland), the working embedding approach or the failure, and the pinned `gpui` git rev and `wry` version. Nothing else depends on spike code; it is not shipped.

- [ ] **Step 1: Pick and pin.** In `crates/spike/Cargo.toml` depend on `gpui` (git `https://github.com/zed-industries/zed`, pinned `rev`) and `wry`. Record the rev in the doc.
- [ ] **Step 2: Build the window.** `main.rs` opens a GPUI window with a title bar area and a content area (`fn main()`).
- [ ] **Step 3: Attempt A, child web view.** Create a `wry::WebView` as a child of the GPUI window via its raw window handle, loading `data:text/html,<h1>spike</h1><script>document.title='js ok'</script>`, positioned over the content area and resized with the window. Run under X11 (`GDK_BACKEND=x11`, `WAYLAND_DISPLAY` unset) and under Wayland. Pass criteria: page visible, JS runs, window resize and focus switching between GPUI text input and the web view work.
- [ ] **Step 4: If A fails, attempt B:** web view in its own native window opened and closed from a GPUI button (the fallback named in the spec). Pass criteria: opens, loads, closes with the app.
- [ ] **Step 5: Test the sandbox rule.** In whichever approach works, load a page from `http://127.0.0.1:<port>` and confirm an `<iframe sandbox="allow-scripts">` cannot read the parent origin (`window.parent.document` throws).
- [ ] **Step 6: Write `spike-webview.md`** with results for each attempt/session, and a recommendation. If neither A nor B works, stop and take the options to the owner before phase 2.
- [ ] **Step 7: Commit** `spike: gpui + wry web view findings`. Spike code stays labeled throwaway; keeping any of it is a new request.

---

### Task 3: Port `schedule` to `workspaceai_shared::schedule`

**Files:**
- Create: `crates/shared/src/schedule.rs`
- Modify: `crates/shared/src/lib.rs` (`pub mod schedule;`)
- Test: `crates/shared/tests/schedule.rs`

**Interfaces:**
- Produces (all in `schedule`):
  - `pub enum ScheduleKind { Repeat, Once, Event }` (serde lowercase)
  - `pub struct EventTrigger { pub event: String, pub detail: String }`
  - `pub struct ParsedSchedule { pub ok: bool, pub kind: ScheduleKind, pub cron: String, pub tz: String, pub once: Option<i64>, pub event: Option<EventTrigger>, pub next: Option<i64>, pub human: String, pub matched: Vec<String>, pub error: Option<String> }` (epoch ms, as TS)
  - `pub fn local_tz() -> String` (falls back to `"UTC"`)
  - `pub fn tz_offset_minutes(tz: &str, at: DateTime<Utc>) -> Option<i32>`
  - `pub fn next_run(cron: &str, tz: &str, from: DateTime<Utc>) -> Option<i64>`
  - `pub fn next_runs(cron: &str, tz: &str, count: usize, from: DateTime<Utc>) -> Vec<i64>`
  - `pub fn format_when(ms: i64, tz: &str, now_ms: i64) -> String` ("Today HH:MM", "Tomorrow HH:MM", else "Thu 29 Oct HH:MM")
  - `pub fn describe_next(cron: &str, tz: &str) -> String` ("not scheduled" when none)
  - `pub fn parse_schedule(phrase: &str, tz: Option<&str>, now: DateTime<Utc>) -> ParsedSchedule`

- [ ] **Step 1: Write the failing tests** in `tests/schedule.rs`, `now = 2026-10-29T10:00:00Z`, `tz = "Europe/Warsaw"`. One test per TS case in `packages/shared/test/schedule.test.ts`, same assertions:
  - `tomorrow_at_9_is_once`: `kind == Once`, `cron == "0 9 30 10 *"`, `next` ISO `2026-10-30T08:00:00Z`, `matched` contains `"tomorrow at 9"`.
  - `every_2_hours_9_18_weekdays`: input uses an en dash `9–18`; `cron == "0 9-18/2 * * 1-5"`, `next` = `2026-10-29T12:00:00Z`, `human` matches `Every 2 hours from 9:00 to 18:00 on weekdays`.
  - `first_day_of_each_month_at_noon`: `cron == "0 12 1 * *"`, `next` = `2026-11-01T11:00:00Z`.
  - `weekday_at_930` → `"30 9 * * 1-5"`; `mondays_and_thursdays_at_10` → `"0 10 * * 1,4"`; `weekends_at_8am` ("every weekend at 8am") → `"0 8 * * 0,6"`; `every_15_minutes` → `"*/15 * * * *"`.
  - `next_monday_at_10_is_once` → next `2026-11-02T09:00:00Z`; `in_3_hours_is_once` → `next == now_ms + 3*3600_000`.
  - `events`: `"on push to main"` → `EventTrigger { event: "git.push", detail: "main" }`; `"when an email arrives"` → event name `"email.received"`.
  - `rejects_nonsense`: `"whenever you feel like it"` → `ok == false`.
  - Review Focus tests: `empty_input_fails_with_hint` (`""` and `"   "` → `ok == false`, error starts `Write when it should run`); `unknown_timezone_fails_not_panics` (`tz = Some("Not/AZone")` → `ok == false`); `dst_gap_still_has_next` (`"every day at 2:30"`, tz Warsaw, now `2026-03-28T12:00:00Z` → `next.is_some()`).
- [ ] **Step 2: Run** `cargo test -p workspaceai-shared --test schedule`. Expected: FAIL, module/functions undefined.
- [ ] **Step 3: Implement** the functions above in `schedule.rs`, line for line from `packages/shared/src/schedule.ts` (same branch order: event, every-N, monthly, repeat, one-off). Library choices: `croner` with `chrono-tz` for `next_run`/`next_runs`; `regex` for every pattern (copy patterns verbatim; normalize `–`/`—` to `-` first). `chrono-node` has no Rust equivalent: implement a small `time_of` and `one_off` parser for exactly these inputs and nothing wider: times (`9`, `9:30`, `8am`, `noon|midday|midnight|morning|afternoon|evening|night`), `tomorrow`, `next <weekday>`, `in N minutes|hours|days`, `today`. Anything else falls to the failure message "I couldn’t find a time in that. Try “every weekday at 9” or “tomorrow at 10”." Mark the cut with `// ponytail: narrow date grammar, swap for a date crate if routines need more phrases`.
- [ ] **Step 4: Run** the same command. Expected: all pass.
- [ ] **Step 5: Commit** `feat(shared): port schedule parser`.

---

### Task 4: Port `commands` to `workspaceai_shared::commands`

**Files:**
- Create: `crates/shared/src/commands.rs`
- Modify: `crates/shared/src/lib.rs` (`pub mod commands;`)
- Test: `crates/shared/tests/commands.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (in `commands`), all names snake_case ports of `packages/shared/src/commands.ts`:
  - `pub enum CommandAction` with variants matching TS `kind`s: `Ui { action: UiAction, then: Option<String> }`, `Native { command: String }`, `OcCommand { name: String }`, `OcCompact`; `pub enum UiAction { Status, Model, Effort, New, Help, Graph }`.
  - `pub struct HarnessCommand` (fields as the TS interface, same table contents copied verbatim).
  - `pub struct NativeCommand { pub name: String, pub description: Option<String>, pub hint: Option<String> }`
  - `pub struct ChatCommand { pub name: String, pub description: Option<String>, pub hint: Option<String>, pub source: CommandSource, pub runs_as: Option<String> }`; `pub enum CommandSource { Harness, Agent }`
  - `pub struct Mention { pub start: usize, pub query: String }`
  - `pub fn command_action(cmd: &HarnessCommand, agent: &str) -> Option<CommandAction>`
  - `pub fn find_harness_command(name: &str) -> Option<&'static HarnessCommand>` (aliases: `clear`→`new`, `cost`→`usage`)
  - `pub fn mention_at(text: &str, caret: usize) -> Option<Mention>`
  - `pub fn insert_mention(text: &str, start: usize, caret: usize, path: &str, open: bool) -> (String, usize)`
  - `pub fn chat_commands(agent: &str, agent_name: &str, native: &[NativeCommand]) -> Vec<ChatCommand>`
  - `pub fn parse_slash(text: &str) -> Option<(String, String)>` (name, args)
  - Offsets (`start`, `caret`) are **byte offsets on char boundaries** (call: GPUI text APIs are UTF-8 byte based; TS used UTF-16 units, identical for ASCII).

- [ ] **Step 1: Write the failing tests** in `tests/commands.rs`, same assertions as `packages/shared/test/commands.test.ts`:
  - `mention_at`: `("@",1)` → `Some{0,""}`; `("look at @src/ma",15)` → `Some{8,"src/ma"}`; `("look at @src now",16)` → `None`; `("look at @src now",12)` → `Some{8,"src"}`; `("mail me@example.com",19)` → `None`.
  - `insert_mention`: `("see @ma please",4,7,"src/main.ts",false)` → `("see @src/main.ts  please", 17)`; `("@sr",0,3,"src/",true)` → `("@src/",5)`; `("@my",0,3,"my file.md",false).0 == "@\"my file.md\" "`.
  - `commandAction`: `status`+`claude` → `Ui{Status, then:"/context"}`; `grok` → `then:"/session-info"`; `opencode` → `Ui{Status, None}`; `review`: claude `Native("/code-review")`, grok `Native("/review")`, opencode `OcCommand("review")`; `compact`+opencode → `OcCompact`; `init`+grok → `None`; `loop`+opencode → `None`; aliases `clear`→`new`, `cost`→`usage`.
  - `chat_commands("claude","Claude Code", [compact "theirs", model, clear, simplify "Clean up code" hint "[target]"])`: exactly one `compact`, `source == Harness`, `runs_as == "Claude Code /compact"`; no agent-sourced `model` or `clear`; last item is the `simplify` agent command; `status` has `runs_as == "status card + Claude Code /context"`; `chat_commands("grok","Grok",&[])` has no `init`.
  - `parse_slash`: `"/effort high"` → `("effort","high")`; `"  /data:analyze  why did it drop?\nmore "` → `("data:analyze","why did it drop?\nmore")`; `"not a /command"`, `"/"`, `"/usr/bin is a path"` → `None`.
  - Review Focus: `multibyte_before_caret` (`mention_at("héllo @sr", <byte offset of end>)` → `Some` with correct `start`, no panic); `parse_slash("")` and `parse_slash("   ")` → `None`.
- [ ] **Step 2: Run** `cargo test -p workspaceai-shared --test commands`. Expected: FAIL, module undefined.
- [ ] **Step 3: Implement** by porting `commands.ts` function by function; copy the harness command table and per-agent mapping verbatim; build the table once (`std::sync::LazyLock<Vec<HarnessCommand>>`). Use `str::is_char_boundary` guards in `mention_at`/`insert_mention` so non-boundary offsets return `None`/unchanged instead of panicking.
- [ ] **Step 4: Run** the same command. Expected: all pass.
- [ ] **Step 5: Create `docs/superpowers/test-matrix.md`** with a table `TS test | Rust test | status` and fill the `schedule.test.ts` and `commands.test.ts` rows (status `ported`).
- [ ] **Step 6: Commit** `feat(shared): port slash commands and mentions`.

---

## Self-review

- **Spec coverage:** workspace + crate layout (T1), phase 0 spike incl. fallback and sandbox rule (T2), first phase-1 ports with test matrix rule (T3, T4). Rest of phase 1–4 is deferred to later plans, as stated at the top.
- **Types:** `ParsedSchedule`, `Mention`, `CommandAction` names match between interface blocks and tests.
- **Proportion:** plan is longer than the doc it came from only in test detail, which pins exact values; bodies are left to the implementer.
