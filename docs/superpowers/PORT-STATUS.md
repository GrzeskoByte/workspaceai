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
- Chat actions: rename, fork, archive, delete (two-step), full-access toggle

## Next, in order
1. Attach file picker (drag/drop)
2. Chat extras left: move chat to another project, copy message, restore archived chats
3. Settings: general, workspace, providers (+test), subscriptions + sign-in, secrets, MCP servers, engine (start/stop/logs/mode), Open Design panel, memory, policy/audit, skills
4. Onboarding (detect opencode, scan, test remote/provider, templates, finish) and New workspace / join org
5. Command palette (Ctrl+K), keyboard shortcuts, workspace switching by number
6. Home / Create (briefs, pipeline) views
7. Library, Routines (use `schedule` crate), Approvals overview, Memory (facts), Computers, Projects management (create/rename/archive/instructions layers)
8. Artifacts: Open Design embedded web view (X11 shim from the spike; see `spike-webview.md` open items), artifact panel/fullscreen, browser panel
9. Port the TypeScript supervisor/orchestrator/memory-mcp to Rust crates; remove `apps/ui`, `apps/desktop`, TS packages
10. Packaging (Linux, macOS), CI

## Not planned (see design doc)
Graph views, BPMN editor.

## Known gaps / debts
- Markdown input length is not capped in `parse` (security review suggestion).
- Visual/interaction behaviour of the window has only been checked by the owner by hand; no UI tests.
- Disk on the dev machine is ~93% full; GPUI builds are large.
