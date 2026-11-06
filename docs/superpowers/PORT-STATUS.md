# GPUI port status

Updated by each loop iteration. Source of truth for "what is left". Plans: `plans/2026-11-05-gpui-app-slice-chat.md`. Design: `specs/2026-11-05-gpui-rust-rewrite-design.md`.

Build/run: `CARGO_PROFILE_DEV_DEBUG=0 cargo run -p workspaceai-app` (needs `libxkbcommon-x11-dev`; `-- --smoke` runs headless against the real supervisor). Tests: `cargo test` (client + shared, no GUI libs). The embedded web view is the `webview` cargo feature of `crates/app`, on by default; it needs `libwebkit2gtk-4.1-dev` and `libgtk-3-dev` to build (both present on the dev machine). `--no-default-features` builds without it and shows a notice where a page would be.

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
- Library (rail ▤): searchable artifact list, newest first; Markdown shown rendered, CSV/JSON/SVG as text, HTML/deck/prototype pages in the web view
- Composer Attach: native file dialog or drag/drop; picked files are added to the draft as `@path` references (the web app has no upload, so none here)
- Command palette (Ctrl+K / Cmd+K): actions, workspaces, chats, settings pages; Up/Down/Enter/Esc
- Chat actions: rename, fork, archive, delete (two-step), full-access toggle, move to another project, Copy per message, archived chats list in the sidebar with Restore
- Ctrl/Cmd+1..9 switches workspace (same as the web app)
- Settings, remaining pages (`crates/app/src/pages.rs` over `crates/client/src/workspace.rs`): Web search (mode, provider, SearXNG URL, switches, check, config preview), Workspaces/members (roles, invite; personal shows "Only you"), Vaults, Skills, Artifacts & sharing, Open Design (status, install/update, daemon, logs, engine, switches, resync, design-system picker), Data (keep chats, archived projects restore/delete), Audit (filter, load all), Appearance (saved to app settings; the native window has one look), Engine (connection mode, remote server form, logs, version pin, sync)
- Providers: edit an existing custom provider (name, URL, models, optional new key); free-form secrets (name + masked value, list, remove)
- General: multi-line workspace instructions editor (`TextInput::new_multiline`: Enter = new line, Up/Down/Home/End per line), accent chips

- Onboarding (`crates/app/src/onboarding.rs` over `crates/client/src/onboarding.rs`): shown while `settings.onboarded` is false, same ten steps as the web app (Welcome, opencode detect/remote connect with Open Design install card, Subscriptions, Sessions scan with folder/session selection, Workspace, Model with provider test, Web search with test, Memory, first Project with AGENTS.md templates, Look with summary), `finish`, and "Skip setup · use demo data"
- New workspace dialog (rail ＋): Team / Personal / Organization (join with SSO email + issuer); rail tiles tinted per kind (org purple, team cyan, personal accent) with a ring on the active one
- General: default project and default model pickers, Delete workspace / Leave organization (two-step; the only personal workspace is protected); Data: Export downloads the .tar.gz to ~/Downloads (never overwrites)
- Home (rail ⌂, `crates/app/src/flow.rs` over `client/src/home.rs`): greeting, composer with project picker and modes (Chat/Doc/Deck/Prototype/Research; Enter sends to a new chat in the picked project), suggestions, Needs you / Continue / Coming up
- New artifact (rail ✦, `client/src/create.rs`): brief with output gallery and templates, questions, three directions (and "three more"), design system search, generate, critique (jury scores, Apply fixes), tweaks as new versions, export to Downloads, Remember direction and tone, Continue as code, Save to the library. HTML/deck pages show in the web view with a Desktop/Mobile toggle
- Approvals (rail ✓, `!` while something waits; `client/src/approvals.rs`): requests across chats with Allow once / Always allow / Reject / Open chat
- Library artifact actions: versions (view one, Restore), lint line, Export per format, Share (link copied), Delete (two-step)
- Project page (click a project in the sidebar): Chats, Artifacts (rolled up from sub-projects), Instructions (inherit switch, project model picker with where it comes from, inherited layers, AGENTS.md editor, save)
- Esc closes the New workspace dialog
- Embedded web view (`crates/app/src/webview.rs`, feature `webview`): wry built as a child of the GPUI window through the spike's `XlibShim`, GTK pumped from a 16 ms GPUI timer. Placed every frame over a slot element (clipped to its scroll area), hidden when no slot asked for it (other views) or when the palette or the New workspace dialog is open; a click in the GPUI window hands the keyboard back from the page. Two instances: artifacts, and the browser (keeps its history)
- Artifact panel (`crates/app/src/panels.rs` over `client/src/web.rs`): a card under a reply that made an artifact opens it beside the chat (type, format, title, project and origin-chat links, "vN of M", Full screen, Expand/Show chat, Close), with the Library's versions/export/share/delete under the page. Pages (HTML, decks, prototypes, JSON/SVG with an Open Design page) load as a host page with the artifact in `<iframe sandbox="allow-scripts" srcdoc>` (opaque origin, like the web app); Markdown and CSV stay native. Desktop/Mobile toggle (390 px frame); decks keep 16:9. Full screen fills the window (native full screen too), Esc leaves, also from inside the page (relayed over IPC). Library and New artifact show pages the same way
- Browser panel (chat action "Browser", or "▣ Preview localhost:…" under a reply whose text or tool output has a local address): address bar (port / localhost / domain → URL, http(s) only), back, forward, reload, open in the system browser, expand, close; the project's dev server row (command, Run/Stop, Log, server chips; opens the address once the server prints it, polls every 1.5 s while running). Pages asking for a new window open in the system browser

## Next, in order
1. (done)
2. (done)
3. Settings leftovers: Open Design install progress log and health check, lean-ctx panel, multi-line IME underline (marked text is not drawn in the text area), mouse caret in the text area is per line only (no soft wrap: shaping, hit-testing, Up/Down and height all assume one row per line, so wrapping is a rewrite of the multi-line path, not a tweak)
4. (done) Remaining from it: the web-search config preview under the onboarding Web step (needs the `webConfigPreview` port), a keyboard-driven workspace switcher overlay like `design/Switcher.dc.html` (Ctrl+K palette already lists and switches workspaces; Ctrl+1..9 works), the New workspace "Start from" options of `NewWorkspace.dc.html` (the React app has none)
5. (done)
6. (done) Home / Create. Left: Home composer has no model/effort/web chips and no `@` file mentions; Loop mode needs the graph editor (not planned); Desktop/Mobile toggle done in batch 8
7. (done) Approvals, artifact versions/export/share/delete, Project instructions and model/inherit. Left: artifact "Move to project" and "Ask for a change" box, Project tabs Graphs/Map/Vaults, Archive/Delete on the Project page (Settings › Projects has them)
8. (done) Embedded web view, artifact panel/fullscreen, browser panel. Left: the panel's "Ask for a change" box and "Move to project" (see 7); the browser panel has no "can't be shown here" check (not needed: pages load top-level in a real web view, not in a frame); back/forward are never greyed out (they drive the page's own history); Wayland (see below)
9. Port the TypeScript supervisor/orchestrator/memory-mcp to Rust crates; remove `apps/ui`, `apps/desktop`, TS packages
10. Packaging (Linux, macOS), CI

## Not planned (see design doc)
Graph views, BPMN editor.

## Known gaps / debts
- Server side (do in the Rust supervisor port): reject ids that are empty or dot segments when created or loaded, and require an explicit confirm parameter on bulk-destructive routes such as `DELETE /facts`. The client already never sends such ids (`api::seg`).
- Markdown input length is not capped in `parse` (security review suggestion).
- Visual/interaction behaviour of the window has only been checked by the owner by hand; no UI tests.
- Web view, **unverified on screen** (the agent machine cannot look at the window; only builds, unit tests and `--smoke` ran): placement and size of the page over its slot (logical pixels handed to wry; HiDPI scale not checked), resize following the window, hiding when switching views or opening the palette/dialog, keyboard focus moving between GPUI inputs and the page (X11 focus; `focus_parent` on clicks), the deck getting arrow keys in full screen (`focus()` runs before the page is placed that frame, so a click may be needed), and native full screen toggling. A page partly scrolled out of the New artifact view is squeezed to the visible part rather than clipped.
- Web view on Wayland: GPUI then has a Wayland window and wry cannot embed into it, so the slot says so; run under XWayland (unset `WAYLAND_DISPLAY`). Separate-window fallback (spike attempt B) not built. macOS path (`build_as_child` on the AppKit handle) compiles only in theory: never built here.
- Disk on the dev machine is ~93% full; GPUI builds are large.
