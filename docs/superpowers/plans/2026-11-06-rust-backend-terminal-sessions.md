# Rust backend + terminal sessions: Implementation Plan

> **For agentic workers:** one batch = one agent run. Use superpowers:executing-plans. Steps use checkbox syntax. Every batch ends with `cargo test`, a `cargo build -p workspaceai-app`, an update to `docs/superpowers/PORT-STATUS.md` and `docs/superpowers/test-matrix.md`, and one commit.

**Goal:** Port `packages/supervisor` (+ the parts of `shared`, `memory-mcp` still used) to Rust, run it in-process in the GPUI app, and replace the custom chat with an embedded terminal that runs the agent's own CLI. Then delete the TS/Tauri/React code (gated on the owner) and ship Linux/macOS packages with CI.

**Spec:** `docs/superpowers/specs/2026-11-05-gpui-rust-rewrite-design.md`. This plan changes it in four places (owner decisions 2026-11-06, ledgered here):
1. Sessions are terminals (PTY + agent CLI), not a supervisor-mediated chat. No ACP client, no transcript renderer, no permission cards, no slash layer, no model/effort picker for sessions.
2. Orchestrator is not ported at all: its only consumer is graph runs, already a non-goal. `rules.test.ts` → N/A.
3. Graphify memory backend, graph/map routes and the `graph_*`/`map_*` MCP tools are not ported (spec non-goal; now final). `memory: 'graph'|'both'` workspaces load as `facts`.
4. One Node script survives: `od-host.ts` (it runs *inside* Open Design's JS runtime and calls its `startDaemonRuntime`; it cannot be Rust). It moves to `resources/od-host.mjs`, plain JS, no build step. Node is already a required runtime for Open Design.

---

## (a) GPUI features: kept vs removed

| Feature (crate/file) | Fate | Why |
|---|---|---|
| Message list, streamed replies, tool rows (`app/main.rs` chat view, `client/chats.rs` message parts) | **Remove** | Agent TUI renders its own transcript |
| Composer, Send/Stop, `@` file popup, Attach (`main.rs`, `client/files.rs` use in composer) | **Remove** | Typing goes to the PTY. Drag/drop a file onto the terminal pastes its quoted path (one line, keeps Attach's value) |
| Slash suggestions, info panel (`client/command.rs`, `shared/commands.rs`) | **Remove** | Agent's own `/` commands |
| Model + effort picker (`client/models.rs`, header chip) | **Remove** for sessions. Keep workspace/project **default model** (written into opencode.json / agent config; the agent may still change it in its TUI) |
| Permission cards, YOLO toggle (`main.rs`, `/chats/:cid/permission*`) | **Remove** | Agent asks in its TUI. "Full access" becomes a launch flag per agent (`claude --dangerously-skip-permissions`, opencode permission `allow` in session config) behind the same toggle |
| Markdown renderer (`client/markdown.rs`, `app/md_view.rs`) | **Keep** | Library, artifact panel, AGENTS.md preview |
| Sidebar session list: rename, archive/restore, delete, fork, move, archived list | **Keep** (metadata in supervisor) | |
| Session status dot (working / needs you / idle / exited) | **Keep, new source**: PTY activity + BEL/OSC 9/OSC 777 notifications, not SSE |
| Home composer | **Keep, changed**: Enter = new session in picked project with the text as the agent's initial prompt (`opencode --prompt`, `claude "<text>"`). Modes Doc/Deck/Prototype route to New artifact as today |
| Projects, project page, instructions/AGENTS.md layers, workspaces, rail, palette, onboarding, all Settings pages | **Keep** | backend ported 1:1 |
| Memory facts + memory MCP | **Keep** | injected into every session's MCP config |
| Library, artifact panel, New artifact (pipeline), web view, browser panel, dev server | **Keep** | pipeline stays headless over the workspace's opencode server |
| Routines | **Keep**: runs are headless (opencode server prompt), each run creates a session record the user can open later as a terminal (resume) |
| Approvals view | **Keep, narrowed**: routine approvals (`/routines/:rid/approve`) + permission asks raised by *headless* runs (routines, pipeline). Terminal sessions never appear here |
| Agents page (subscriptions, sign-in) | **Keep** | sign-in also possible inside the terminal |
| Engine page | **Keep** | opencode server still runs per workspace |
| Supervisor launcher (`client/supervisor.rs`), SSE (`client/events.rs`), ureq in `client/api.rs` | **Remove** after batch 8 | in-process |

## (b) Terminal stack

Checked on this machine against gpui rev `654efae4`:
- Zed's own `terminal` / `terminal_view` crates exist at that rev but are **GPL-3.0-or-later** and pull `settings`, `theme`, `task`, `project`. Not usable (licence, weight). Read them for design ideas only; copy nothing.
- `alacritty_terminal = "0.26"` (crates.io, Apache-2.0, MSRV 1.85; toolchain is 1.98). No gpui dependency, so no version clash with the pinned rev. It already ships the PTY (`tty::new`, unix openpty + Windows ConPTY), the reader thread (`event_loop::EventLoop`), VT parsing (`vte`), grid, scrollback, selection, search, and OSC 8 hyperlinks.
- `portable-pty 0.9` would duplicate what `alacritty_terminal::tty` has. **Not used.**

**Recommendation:** `alacritty_terminal 0.26` only, split in two:
- `crates/term` (no GPUI, headless-testable): `TermSession { spawn(argv, env, cwd, size), write(bytes), resize(cols, rows, cell_px), snapshot() -> Grid, selection ops, scroll(delta), title(), exit_status() }`, event channel (`Wakeup`, `Title`, `Bell`, `Exit`, `ClipboardStore`, `PtyWrite`). Key → bytes encoder (xterm sequences, modifyOtherKeys off, application cursor mode from `TermMode`), bracketed paste wrapper.
- `crates/app/src/terminal_view.rs` (GPUI element): paints the snapshot.

| Concern | Approach |
|---|---|
| Rendering | Custom `Element`: one shaped line per row via `window.text_system().shape_line` with runs batched by style; cell backgrounds as quads; cursor as quad (block/beam/underline from `TermMode`). Repaint on `Wakeup`, coalesced to one frame. Monospace font: system monospace (`monospace` family), size from app settings |
| Colors | 16-color palette + 256 + truecolor from `alacritty_terminal::vte::ansi::Color`; palette is a constant table matching the app theme (one theme; no settings page) |
| Input | GPUI `on_key_down` → encoder → `write`. Ctrl/Alt/Shift combos, function keys, Home/End, PageUp/Down (Shift+PageUp scrolls scrollback locally). App shortcuts that must win (Ctrl+K palette, Ctrl+1..9) are taken before the terminal; everything else goes to the PTY |
| IME | Implement `EntityInputHandler` on the view: committed text → PTY; marked text drawn at the cursor (underlined), not sent |
| Selection/copy | Mouse drag → `Selection::new/update` (simple, semantic on double-click, line on triple). Ctrl+Shift+C / Cmd+C copies `selection_to_string`; Ctrl+Shift+V / Cmd+V pastes (bracketed when `TermMode::BRACKETED_PASTE`). When the app runs with mouse mode on (`TermMode::MOUSE_*`), forward mouse events as SGR reports unless Shift is held |
| Resize | On layout bounds change → cols/rows from cell size → `term.resize` + `tty.on_resize` (SIGWINCH) |
| Scrollback | 10 000 lines; wheel scrolls display offset (or sends arrows in alt-screen) |
| Links | OSC 8 hyperlinks + regex for `https?://…` and `localhost:port` on hover with Ctrl/Cmd; click → existing browser panel for local addresses (reuse `client/web.rs` address rules), system browser otherwise (http/https only, same allowlist as Markdown links) |
| Bell / notify | `Bell` and OSC 9/777 → session "needs you" + rail badge (replaces permission `!`) |
| Lifetime | One `TermSession` per opened session, kept while the app runs (switching sessions does not kill it); closed on session close/delete/app quit (SIGHUP to the process group) |

## (c) Session identity and history without SSE

`Chat` becomes `Session` in `state.json` (same file, field names kept where they still mean the same; `sid` stays the agent's session id). New fields: `agent` (adapter id), `launched: bool` (has the agent created its record yet), `fullAccess`. Dropped: `model`, `effort`, `transcript`, `graphNotes`, `web` (web search lives in the workspace config).

Per-agent adapter, a `match` on `agent` in `crates/supervisor/src/agents.rs` (no trait; three-four arms):

| Agent | New | Resume | Fork | Isolation env | MCP injection | History source |
|---|---|---|---|---|---|---|
| opencode | supervisor `POST /api/session` on the workspace server → `ses_…`; TUI: `opencode --server <ws url> --session <sid> <folder>` | same command | server `POST /api/session/:id/fork` → new sid | engine env (XDG_*, `OPENCODE_CONFIG`, keychain refs, `OPENCODE_SERVER_PASSWORD`) | already in the workspace `opencode.json` | server `GET /api/session/:id/message` |
| claude | supervisor picks a UUIDv4; `claude --session-id <uuid>` | `claude --resume <uuid>` | `claude --resume <old> --fork-session --session-id <new>` (verify combo; else fork without fixed id and pick the new jsonl by mtime) | `CLAUDE_CONFIG_DIR=<ws>/agents/claude` (as `subscriptions.ts`) | `--mcp-config <ws>/run/<sid>.mcp.json --strict-mcp-config` built from `acpMcpServers` logic | `$CLAUDE_CONFIG_DIR/projects/<slug(cwd)>/<uuid>.jsonl` |
| grok | `grok` in folder; id discovered by diffing `$GROK_HOME` session dir before/after first exit | resume flag **unknown** (CLI not installed here) → open fresh with `--continue`-style flag if it exists, else no resume | hidden | `GROK_HOME=<ws>/agents/grok` | grok config file in `GROK_HOME` (verify) | session files in `GROK_HOME` (verify) |
| pi | as today's `pi-config.ts` agent dir | verify | hidden | `pi-config.ts` env | its config | — |

Rules:
- The id is fixed **before** spawn whenever the agent allows it (opencode, claude), so a crash before the first message cannot orphan a session.
- Title: user rename wins; until renamed, the terminal title (OSC 0/2) or the first prompt sets it (debounced, written to state).
- Rename/archive/delete for opencode also call the server (as today, `sessions.test` contract); for claude, delete removes the jsonl only after a two-step confirm.
- Status is local to the app (PTY alive, output in last 2 s = working, bell = needs you); never persisted.
- Import (onboarding scan + Settings › Data): opencode via server list (as `scanSessions`); claude by listing `projects/*/*.jsonl` of the user's own `~/.claude` and **copying** the chosen files into the workspace `CLAUDE_CONFIG_DIR` (isolation rule: never point a workspace at the shared home).
- Transcript read (`GET /api/ws/:id/chats/:cid/messages` kept, read-only, plain text parts): used by Library search and by "Summarize into memory" (writes `suggested` facts through the existing facts API). No live stream.
- Artifact attribution (R-13): MCP servers get `HARNESS_CHAT=<session id>` per session where the agent takes per-session MCP config (claude, grok). opencode shares one server config, so `callingChat` falls back to "most recently active terminal session of that workspace + project". Ledger it.

## (d) Crate layout and port order

```
crates/shared      types.rs (port of shared/types.ts, serde camelCase, unknown fields kept), util, defaults, facts,
                   opencode_config, claude_design_system, schedule (done), markdown/sheet. commands.rs deleted in batch 8.
crates/supervisor  lib, no GPUI, no tokio. std threads + ureq (opencode HTTP) + tiny_http (loopback) + keyring + ed25519-dalek.
  router.rs        handle(method, path, query, body) -> Result<Value, ApiError>; same paths/JSON as routes.ts
  supervisor.rs runtime.rs paths.rs jsonfile.rs keychain.rs policy.rs audit (in runtime) opencode.rs detect.rs
  agents.rs (adapters above) sessions.rs artifacts.rs opendesign.rs pipeline.rs design_guide.rs subscriptions.rs
  signin.rs computers.rs scheduler.rs routines (screens.rs) vault.rs gitlab.rs leanctx.rs devserver.rs files.rs
  web.rs (search config) recall.rs server.rs (loopback, token, allowlisted paths) mcp.rs (stdio JSON-RPC servers)
crates/term        PTY + terminal model (alacritty_terminal), headless
crates/client      keeps view-model modules; Api::request calls supervisor::Supervisor::handle in-process
crates/app         GPUI; + terminal_view.rs; `workspaceai mcp <memory|artifacts|vault>` subcommand dispatches to supervisor::mcp
```
No `store`, `acp`, `orchestrator` or `memory-mcp` crates: persistence is today's JSON files (`jsonfile.ts` is 32 lines), ACP is gone, orchestrator is N/A, memory-mcp is ~300 lines of stdio JSON-RPC (`initialize`, `tools/list`, `tools/call`) hand-written in `mcp.rs` (rmcp would drag in tokio for three tools).

Route port order (TS test file = contract; each Rust test sits in `crates/supervisor/tests/<same name>.rs`, calls `handle()` in a temp `HARNESS_HOME`, keychain `memory` backend; tests needing `opencode` skip with a printed reason when it is not on PATH):
1. state/settings/workspaces/activate/ws patch/snapshot, policy, audit → `core`, `security`
2. engine (start/stop/restart/mode/version/logs), models, providers, secrets, mcp, opencode-json → `engine`, `providers`, `shutdown`, `core` (managed loopback)
3. projects, agents/layers, facts, memory-changed, recall, export, archived, files → `core`, `memory`, `recall`, `files`, `formats`
4. sessions (chats routes minus send/command/stop/permission), scan/import, attach → `sessions`, `core` (R-10, R-16)
5. agent/* callbacks, artifacts, shared links, design systems, pipeline, opendesign → `artifacts`, `pipeline`, `vendors`
6. subscriptions/signin, computers, routines + hooks + approve, scheduler → `computers`, `scheduler`, `recovery` (routine part)
7. vaults, web search, gitlab, leanctx, dev server, preview/open-external, onboarding/* → `vault`, `web`, `gitlab`, `watch-tree`
N/A with reason: `acp`, `permissions` (terminal sessions; headless-ask polling gets one new test instead), `graph*`, `maps`, `graphify`, `browser`, `graphrun`, `watch-supervisor` (no separate process), `bpmn`, `rules`.

## (e) In-process vs separate process

**In-process.** `Supervisor` is constructed in `app::main` before the window; GPUI views keep using `client::Api`, whose `request` now calls `sup.handle(...)` on a worker thread (same `Result<Value, ApiError>`), so no view code changes when the backend switches. `UiEvent`s go through `sup.on(Sender<UiEvent>)` into the existing `Msg` pump, replacing `events::stream`.
A loopback HTTP listener stays for external callers only: MCP servers (`/api/ws/:id/agent/*`, `/memory-changed`, `/facts`, `/snapshot`, `/artifacts/:aid/content`, vault agent routes), routine webhooks `/api/hooks/:id/:rid`, shared links `/api/shared/*`. Per-launch token, loopback bind, allowlist of path prefixes; everything else 404.
Quit: `sup.shutdown()` stops opencode servers, OD daemons, dev servers, PTYs (SIGHUP group, 3 s, SIGKILL). Crash safety: child processes get `PR_SET_PDEATHSIG` on Linux; macOS relies on the idle stop + stdin-closed pattern for opencode.
Why not a separate process: one binary, no ready-line handshake, no serialization tax, and the only other clients (MCP servers) already need just a handful of routes.

## (f) Batches

Profiles: **I** = implementer (sonnet), **A** = architect (opus). Each batch: ~10–20 tool-heavy steps.

### B1 (I) shared types and helpers
- [ ] Port `shared/src/{types,util,defaults,facts,opencode-config,claude-design-system,markdown(sheet)}.ts` to `crates/shared`.
- [ ] Port tests `sheet`, `markdown`, `claude-design-system`; mark `scroll`, `session-reads` N/A (UI-only) or port if pure.
- **Accept:** `cargo test -p workspaceai-shared` green; a real `~/.harness/ws/*/state.json` and `workspace.json` round-trip through serde without losing fields (test with a fixture copied from a demo workspace).

### B2 (A) supervisor skeleton, keychain, policy
- [ ] `crates/supervisor`: paths, jsonfile (atomic write), keychain (`keyring`, same service names; `HARNESS_KEYCHAIN=memory`), policy (Ed25519, `canonical()` must match JS `JSON.stringify` for numbers), audit, Router + `handle()`, id validation (reject empty / `.` / `..` segments; known debt from PORT-STATUS), `DELETE /facts` requires `confirm`.
- [ ] Routes group 1 + tests from `core`/`security` that need no opencode.
- **Accept:** tests green; a policy signed by the TS `signPolicy` (fixture) verifies in Rust and vice versa.

### B3 (I) engine management
- [ ] `detect`, `opencode.rs` (spawn `serve` with XDG env + password, `/api/info` health, logs, idle stop, remote mode with `remoteUrlAllowed`), version pin, models, providers, secrets, mcp, opencode-json. Routes group 2.
- **Accept:** `engine`, `providers`, `shutdown` ported and green with opencode on PATH; a second workspace's server has a different XDG dir and port.

### B4 (I) projects, instructions, memory
- [ ] Routes group 3; recall; export `.tar.gz` (`tar` + `flate2`), never includes secrets.
- **Accept:** `core` (projects, layers, delete subtree, export), `memory`, `recall`, `files`, `formats` green.

### B5 (A) session model and agent adapters (no UI)
- [ ] `Session` record + migration from `Chat` on load (drop fields listed in (c)).
- [ ] `agents.rs`: `launch_spec(session) -> {argv, env, cwd, files_to_write}`; per-session MCP json for claude; create/resume/fork per table (c); `GET /api/ws/:id/chats/:cid/launch` returns the spec.
- [ ] Routes group 4, transcript read (opencode server, claude jsonl), import (opencode + claude copy-in).
- [ ] Headless runner for routines/pipeline: `ask_and_wait` = opencode `prompt` + `experimental/session/:id/wait`, polling `permission` every 2 s and raising `UiEvent::Approval` for asks.
- **Accept:** `sessions` green; unit tests: claude spec never references `~/.claude`, env contains the workspace `CLAUDE_CONFIG_DIR`, `--mcp-config` file has memory+artifacts servers with `HARNESS_CHAT`; opencode spec carries `--server` and the password in env, not argv.

### B6 (A) `crates/term`
- [ ] `TermSession` over `alacritty_terminal 0.26` (spawn, write, resize, snapshot, selection, scroll, events), key encoder, bracketed paste.
- **Accept:** headless tests: `printf '\e[31mhi'` → cell "h" red; resize to 40×10 reaches the child (`stty size` prints `10 40`); exit status arrives; encoder table test (Ctrl+C = 0x03, Alt+b = ESC b, Up in app-cursor mode = ESC O A); selection over a wrapped line copies without the wrap newline.

### B7 (A) GPUI terminal view
- [ ] `terminal_view.rs`: element paint (runs, backgrounds, cursor), focus, key input with app-shortcut passthrough, IME input handler, mouse selection, copy/paste, wheel scroll, mouse reporting, resize, link hover/click, bell → status.
- [ ] Debug entry: `workspaceai --term <cmd>` opens a window with only a terminal (owner test harness).
- **Accept:** builds; `--smoke` still passes; owner checklist (g) handed over. No headless assertion possible for paint.

### B8 (I) switch the app: in-process backend + terminal sessions
- [ ] `client::Api` → in-process `handle`; events via channel; delete `client/supervisor.rs`, `events.rs`, ureq.
- [ ] Open/create session → fetch launch spec → `TermSession` in the main pane; keep sessions alive across switches.
- [ ] Remove chat UI per table (a), `client/command.rs`, `client/models.rs` (session parts), `shared/commands.rs` + its matrix rows → N/A (feature removed). Home composer → initial prompt. Drag/drop → paste path. Full-access toggle → relaunch with flag.
- **Accept:** `--smoke` runs against the in-process supervisor with no Node supervisor started (`pgrep -f supervisor/src/main.ts` empty); `cargo test` green; diff removes more lines than it adds in `crates/app`.

### B9 (A) loopback server, MCP servers, artifacts
- [ ] `server.rs` (tiny_http, token, allowlist), `mcp.rs` (memory_search/read/write, artifact_guide/create/update/read/list, vault_*), app subcommand `workspaceai mcp <kind>`; opencode.json + claude mcp json point at `current_exe()`.
- [ ] Artifacts store, versions, exports, share links, agent/* routes.
- **Accept:** `artifacts` green; an MCP stdio test drives `initialize → tools/list → tools/call memory_write` against the binary and sees a `suggested` fact; requests without token → 401; non-allowlisted path → 404; bind address is 127.0.0.1.

### B10 (I) Open Design + pipeline
- [ ] `opendesign.rs` (install/update, daemon per workspace via `node resources/od-host.mjs`, logs, resync), design systems, `pipeline.rs` routes on the headless runner.
- **Accept:** `pipeline`, `vendors` green (pipeline E2E skips without OD/opencode, printing why); `od-host.mjs` runs under plain `node` with no TS loader.

### B11 (I) subscriptions, computers, routines, approvals
- [ ] `subscriptions`/`signin` (detect, copy-in login, browser sign-in with code fallback), computers (pairing, WoL), routines + hooks + approve, scheduler thread (croner via `shared::schedule`), Approvals view fed by headless asks + routine approvals.
- **Accept:** `computers`, `scheduler`, routine part of `recovery` green; a routine run creates a session record that opens in a terminal with resume.

### B12 (I) remaining routes
- [ ] Vaults, web search config + preview (also fixes the onboarding Web gap), gitlab, leanctx, dev server, preview check, open-external, onboarding/* (detect, scan, test-remote/provider/web, finish, demo).
- **Accept:** `vault`, `web`, `gitlab`, `watch-tree` green; every route in `routes.ts`, `screens.ts`, `pipeline.ts`, `vault.ts`, `extensions.ts` is either served by `handle()` or listed N/A in the matrix (script: grep TS route strings, diff against Rust router table).

### B13 (A) parity sweep → owner gate
- [ ] Test matrix has no `pending` rows. Run the app on the owner's real `~/.harness` copy (read-only copy in a temp `HARNESS_HOME`): workspaces, projects, facts, artifacts, routines all load.
- [ ] Write the owner checklist (g) into PORT-STATUS.
- **Accept:** owner confirms in chat. **No deletion before that message.**

### B14 (I) deletion — only after the owner's confirmation
- [ ] First copy `apps/desktop/src-tauri/icons/{32x32,128x128,128x128@2x,icon.icns,icon.png}` to `crates/app/assets/icons` (B15 needs them).
- [ ] `git rm -r apps/ui apps/desktop packages/{supervisor,shared,orchestrator,memory-mcp}` except `od-host` (moved in B10); root `package.json`/lockfile/vitest config removed or reduced to nothing; workspace `exclude` cleaned; `default-members` revisited; DESIGN.md + spec updated for terminals; LICENSES.md: drop React/Vite/Tauri/cmdk/bpmn/chrono-node/cron-parser/zod/MCP SDK/napi keyring rows, add alacritty_terminal, keyring, tiny_http, ed25519-dalek, wry, gpui.
- **Accept:** `cargo test`, `cargo build -p workspaceai-app --release`, `--smoke` green; `git grep -n "packages/supervisor\|apps/ui"` returns only history docs.

### B15 (I) packaging + CI
- [ ] `cargo-packager` (one config for `.deb`, AppImage, `.dmg`; maintained by the Tauri/CrabNebula team, no Tauri runtime) in `crates/app/Cargo.toml` metadata; icons from `crates/app/assets/icons` (copied in B14); `resources/od-host.mjs` bundled; .deb depends `libwebkit2gtk-4.1-0, libgtk-3-0, libxkbcommon-x11-0`.
- [ ] `.github/workflows/ci.yml`: ubuntu-24.04 (apt dev libs, `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `cargo build -p workspaceai-app`), macos-14 (`cargo build -p workspaceai-app`, tests). `opencode`-dependent tests skip in CI. `release.yml` on tag: build + package both OSes, upload artifacts. Rust cache (`Swatinem/rust-cache`). macOS unsigned in v1 (notarization needs the owner's Apple ID; ledger it).
- **Accept:** `cargo packager --release` makes a .deb and AppImage locally; the .deb installs in a clean `ubuntu:24.04` container and `workspaceai --smoke` (headless) exits 0 there with opencode absent (reports "opencode not found", no panic). CI yaml validated with `actionlint`.

## (g) Risks and what cannot be verified headlessly

Risks:
- **Agent CLI flags drift.** `claude --session-id` + `--fork-session` combo, grok resume/MCP config, opencode `--server` auth via env: verify each on the owner's machine in B5; adapters keep a one-line fallback (open fresh) instead of failing.
- **opencode attach shares the server's MCP config** → artifact attribution per session is heuristic for opencode (c). Fix later with an opencode per-session MCP env if it appears.
- **Isolation**: a claude/grok session must never see `~/.claude`/`~/.grok`; covered by B5 unit tests, but a CLI that ignores `CLAUDE_CONFIG_DIR` for some file (e.g. `~/.claude.json`) would leak. Check on screen with `strace -f -e openat` once (B5, owner machine).
- **Policy canonical JSON** byte-for-byte vs JS: float formatting; test with fixtures (B2).
- **Terminal correctness** (wide chars, emoji width, combining marks, ligature-free mono font fallback) is only partly testable via grid snapshots.
- **Wayland**: terminal is pure GPUI (fine); web view still X11/XWayland only (unchanged debt).
- **macOS**: PTY, IME, Cmd shortcuts and the web view are compiled in CI only; nobody runs them here.
- **Disk**: dev machine ~93% full; `alacritty_terminal` is small, but release + packaging builds double target size. `cargo clean -p` between batches if needed.
- **Data**: the app reads the live `~/.harness`; B5's `Chat → Session` load-migration must be lossless and write only on change. Backup `state.json` → `state.json.bak-tsport` once on first Rust write.

Owner must check by hand (cannot be verified headlessly):
- Terminal paint, cursor, colors, font, HiDPI scale, resize reflow, scrollback wheel, selection highlight, copy/paste both ways, IME (e.g. Polish dead keys / compose, CJK), link Ctrl+click, mouse in TUIs (opencode, claude), alt-screen apps (`vim`, `htop`).
- Each agent: new, resume after app restart, fork, rename, the agent's own `/model` picker, its permission prompts, sign-in inside the terminal.
- Status dot + "needs you" on bell; sessions keep running while viewing another.
- Routine run → open as terminal and continue; Approvals for a headless ask.
- Packaged .deb/AppImage launch from the desktop menu; .dmg on a Mac.

## (h) Packaging + CI

See B15. Summary: `cargo-packager` for `.deb` + AppImage + `.dmg`; sidecars (opencode, Node for Open Design) detected, not bundled (spec decision); CI on GitHub Actions, Linux full test + build, macOS build + test, release workflow on tags; signing/notarization deferred to the owner.
