# GPUI port status

Updated by each loop iteration. Source of truth for "what is left". Plans: `plans/2026-11-05-gpui-app-slice-chat.md`. Design: `specs/2026-11-05-gpui-rust-rewrite-design.md`.

Build/run: `CARGO_PROFILE_DEV_DEBUG=0 cargo run -p workspaceai-app` (needs `libxkbcommon-x11-dev`; `-- --smoke` runs headless against the real supervisor). Tests: `cargo test` (client + shared, no GUI libs).

## Done (native app, TS supervisor as backend)
- Workspace rail, project/chat sidebar, messages, streamed replies, tool rows, send, Stop, New chat
- Permission approval cards (once / always / reject)
- Markdown rendering (headings, emphasis, code, lists, quotes, links; link scheme allowlist)
- Slash commands: suggestions, run via supervisor, info panel (status/usage/context/help), /new
- Model + effort picker (header chip, also opened by /model and /effort)
- `@` file references with a file list popup (project file search)
- Settings shell (gear in the rail): General (name, read-only instructions/model), Memory switches + facts (add, approve, delete, delete all), Engine (status, start/stop/restart); other pages show a placeholder
- Routines (Settings › Routines): list with next run and last run, Run now, Pause/Resume, Delete, and create with a live "understood as…" preview (uses the ported schedule parser)
- Projects (Settings › Projects): tree with chat counts, create, subproject, rename, archive/restore, delete (two-step; General protected)
- Computers (Settings › Computers): status, capabilities, per-workspace access switch (keeps other workspaces' access), Wake, Unlink (two-step). Pair a computer shows the 6-character code and its expiry
- Providers (Settings › Providers): list, key status from the keychain, save/remove key through a masked field (no copy/cut), remove provider (two-step); org-locked pages are read-only. Add a custom provider form (presets, test connection, key to the keychain)
- Agents (Settings › Agents): subscriptions with install/sign-in/connection status, Connect/Disconnect, browser sign-in for Claude Code (waits, link and code fallback, cancel)
- Connectors (Settings › Connectors): MCP servers with on/off, remove (two-step); built-in and org-locked ones protected. Add form (URL or command)
- Library (rail ▤): searchable artifact list, newest first; Markdown shown rendered, CSV/JSON/SVG as text, HTML explained as needing the web view
- Composer Attach: native file dialog or drag/drop; picked files are added to the draft as `@path` references (the web app has no upload, so none here)
- Command palette (Ctrl+K / Cmd+K): actions, workspaces, chats, settings pages; Up/Down/Enter/Esc
- Chat actions: rename, fork, archive, delete (two-step), full-access toggle, move to another project, Copy per message, archived chats list in the sidebar with Restore
- Ctrl/Cmd+1..9 switches workspace (same as the web app)
- Settings, remaining pages (`crates/app/src/pages.rs` over `crates/client/src/workspace.rs`): Web search (mode, provider, SearXNG URL, switches, check, config preview), Workspaces/members (roles, invite; personal shows "Only you"), Vaults, Skills, Artifacts & sharing, Open Design (status, install/update, daemon, logs, engine, switches, resync, design-system picker), Data (keep chats, archived projects restore/delete), Audit (filter, load all), Appearance (saved to app settings; the native window has one look), Engine (connection mode, remote server form, logs, version pin, sync)
- Providers: edit an existing custom provider (name, URL, models, optional new key); free-form secrets (name + masked value, list, remove)
- General: multi-line workspace instructions editor (`TextInput::new_multiline`: Enter = new line, Up/Down/Home/End per line), accent chips

## Next, in order
1. (done)
2. (done)
3. Settings leftovers: workspace export download (needs a binary GET), delete/leave workspace, default project/model pickers, New team/org workspace and SSO join (see 4), Open Design install progress log and health check, lean-ctx panel, multi-line IME underline (marked text is not drawn in the text area), mouse caret in the text area is per line only (no wrapping: long lines scroll off, no soft wrap)
4. Onboarding (detect opencode, scan, test remote/provider, templates, finish) and New workspace / join org
5. (done)
6. Home / Create (briefs, pipeline) views
7. Approvals overview, artifact versions/export/share/delete, computer pairing, Project instructions (AGENTS.md editor, layers) and project model/inherit settings
8. Artifacts: Open Design embedded web view (X11 shim from the spike; see `spike-webview.md` open items), artifact panel/fullscreen, browser panel
9. Port the TypeScript supervisor/orchestrator/memory-mcp to Rust crates; remove `apps/ui`, `apps/desktop`, TS packages
10. Packaging (Linux, macOS), CI

## Not planned (see design doc)
Graph views, BPMN editor.

## Known gaps / debts
- Server side (do in the Rust supervisor port): reject ids that are empty or dot segments when created or loaded, and require an explicit confirm parameter on bulk-destructive routes such as `DELETE /facts`. The client already never sends such ids (`api::seg`).
- Markdown input length is not capped in `parse` (security review suggestion).
- Visual/interaction behaviour of the window has only been checked by the owner by hand; no UI tests.
- Disk on the dev machine is ~93% full; GPUI builds are large.
