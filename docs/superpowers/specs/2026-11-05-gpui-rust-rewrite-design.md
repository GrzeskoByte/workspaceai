# WorkspaceAI → native Rust app on GPUI

2026-11-05 · status: draft for approval

## Intent

Replace the Tauri + React + TypeScript stack with one native Rust app. The UI is built on GPUI (https://gpui.rs). The supervisor, orchestrator, memory-mcp and shared packages are ported to Rust crates. The product does not change: opencode still runs the agent, Open Design still makes artifacts (see `docs/DESIGN.md`), and WorkspaceAI is still the shell around them.

Why: the owner wants a single Rust codebase and binary, with no Node runtime of our own and no web UI toolchain.

## Goals

- One Rust workspace producing one desktop binary. No Tauri, no React, no Vite.
- Every supervisor responsibility today is reproduced in Rust: workspaces and projects, opencode sidecar management, subscriptions and sign-in, keychain secrets, memory and recall, scheduler, policy, extensions, computers, browser, artifacts pipeline, Open Design host.
- Core UI in GPUI: shell, sidebar, chat, composer, home, onboarding, settings, command palette, library, routines, approvals, memory, computers, projects.
- Open Design's own UI is shown as an embedded web view, so artifact creation, preview, versions and fullscreen are not rewritten in GPUI.
- Existing TypeScript tests are the behavioural spec. Each ported module gets the equivalent Rust tests.

## Non-goals (for now)

- Graph views are dropped: `Graph`, `MapView`, `MapPreview`, `MemoryGraphView`, `graphPalette`, `runPaint`. Their supervisor backends (`graphify`, `graphs`, `graph-tools`, `maps`) are not ported in v1 (my call: nothing consumes them once the views are gone).
- **BPMN editor is not migrated.** It stays in draft. Future consideration: a GPUI-native or embedded BPMN editor, revisited after phase 4. The `bpmn-js` and `bpmnlint` dependencies, `orchestrator/layout.ts`, `orchestrator/lint.ts` and `shared/bpmnPreview.ts` are not ported.
- Forking opencode or Open Design. Both run unmodified.
- Rebuilding Playwright in Rust.
- Mobile targets. The Tauri Android icons in the repo are not carried over.

## Anti-goals

- A port that looks right but loses behaviour: any supervisor test with no Rust equivalent at the end of its phase counts as a failure.
- A GPUI app that is only a thin wrapper around a web view. Only Open Design surfaces and the browser panel are web views.

## Decisions made with the owner

1. **Scope:** whole shell replaced. One native Rust app.
2. **Backend:** supervisor and the other packages are rewritten in Rust, not kept as a Node sidecar.
3. **Open Design:** embedded as a web view in the GPUI window.
4. **BPMN:** deferred, documented as future work.
5. **Graphs:** removed for now.

## Calls made by the author (not yet approved)

- **Playwright stays external.** The supervisor launches `@playwright/mcp` as a Node subprocess and talks to it over stdio MCP with the `rmcp` crate. "All Rust" means all of our code. Open Design's daemon and opencode are external processes already, so a Node runtime on the machine is a dependency either way. A Rust CDP server (`chromiumoxide`) can replace it later behind the same MCP interface.
- **In-process core.** The supervisor becomes a library crate the GPUI app calls directly, with a typed state-and-events model instead of the HTTP+token API the React UI uses today. A local HTTP/MCP server remains only for what external processes call back into (opencode MCP tools, Open Design host, memory-mcp, artifacts-server).
- **Web view:** `wry`. GPUI has no built-in web view, and embedding one in a GPUI window on Linux (X11 vs Wayland) is the biggest technical risk. It is settled by a spike (phase 0) before anything else is built.
- **Browser panel** is also a web view, not a GPUI rewrite.
- **Orchestrator:** only what is needed to run existing loops headless (`model`, `rules`, `runner`) is ported, in phase 3. Loop editing waits for the BPMN decision.
- **TypeScript is kept in the repo** until phase 3 reaches parity, then removed in one commit.
- **Crates:** `workspaceai-shared`, `-supervisor`, `-orchestrator`, `-memory-mcp`, `-app`. Cargo workspace at the repo root.

- **Delivery order (owner said "build it" without choosing; author's default):** vertical slice first (onboard, workspace, chat, Open Design embed), then the rest toward full parity.
- **Existing data (author's default):** keep today's on-disk formats and keychain service names where that costs nothing, so the Rust app can read existing workspaces. No migration tooling in v1.
- **Look (author's default):** same layout and behaviour as the React UI, GPUI styling, no pixel parity.
- **Spike:** phases 1 and 2 start in parallel with phase 0. If the spike fails, Open Design opens in a separate native window or the system browser, and the options go to the owner first.
- **HTTP+token API is not a contract.** Only routes and MCP endpoints that opencode, Open Design and memory-mcp call back into stay wire-compatible.
- **Platforms:** Linux and macOS at v1; Windows best effort.
- **Sidecars:** opencode, Open Design and Node are detected on the machine, not bundled in v1 (as `detect.ts` does today).
- **Test matrix:** tests covering removed or redesigned surfaces (graphs, HTTP routes) are marked N/A with a reason. No UI tests to port.

## Constraints

- Linux is the development platform; macOS and Windows should stay possible. GPUI supports macOS and Linux first; Windows support is a risk to check in phase 0.
- Secrets stay in the OS keychain (`keyring` crate replaces `@napi-rs/keyring`).
- Personal and organization workspaces stay fully isolated, including their own opencode server, Open Design daemon, memory and secrets (`docs/DESIGN.md`).
- The artifact web view must keep today's sandbox rule: scripts run, no same-origin access to the app.
- The app stops its sidecars on quit, as the Tauri shell does now (`apps/desktop/src-tauri/src/lib.rs`).

## Phases

**0. Spike (throwaway).** A GPUI window embedding a `wry` web view that loads a local page, on this Linux machine under both X11 and Wayland. Output: a go/no-go and the chosen embedding approach. If it fails, the fallback options go back to the owner before phase 1.

**1. Rust workspace and core.** Port `shared` types and utilities, then `supervisor` (server, routes, runtime, opencode and Open Design management, keychain, subscriptions, sign-in, policy, scheduler, recall, extensions, computers, browser, artifacts, pipeline), then `memory-mcp`. Port tests alongside. Done when the ported tests pass and a headless run can drive a chat against opencode.

**2. GPUI shell and core views.** Window, workspace rail, sidebar, chat, composer, home, onboarding, settings, command palette (replacing `cmdk`), Markdown rendering (replacing `Markdown.tsx`), icons and theming. Done when a user can onboard, start a workspace and hold a chat.

**3. Remaining views and orchestrator runner.** Library, routines, approvals, memory, computers, projects, run card and panel, action sheet. Orchestrator runner for existing loops. Remove the TypeScript packages and `apps/ui`, `apps/desktop`.

**4. Embedded surfaces.** Open Design web view (create, artifact panel, fullscreen) and browser panel, wired to workspace state. Packaging and install story.

Phases 2–4 can overlap once phase 1 is stable. The editor for markdown/code (CodeMirror today) needs a GPUI text editing approach, decided in phase 2 (left to the builder).

## Left to the builder

- GPUI crate versions and the component library to use.
- Exact crate layout beyond the five named above.
- Async runtime and the state/event model between core and UI.
- Test strategy details, as long as each ported TS test has a Rust counterpart.
- Packaging format per OS.

## Future (documented, not planned)

- BPMN editor and loop editing.
- Graph and memory-graph views.
- Rust-native browser automation replacing Playwright MCP.
