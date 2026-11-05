# GPUI App, Slice A (shell + chat) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (owner chose inline). Steps use checkbox syntax.

**Goal:** A native GPUI app that starts the existing TypeScript supervisor, shows workspaces → projects → chats, shows a chat's messages, and sends a message with a streamed reply.

**Architecture:** `crates/client` (no GUI, fully unit-tested) owns the supervisor launcher, the HTTP+token API client, typed state and the SSE event stream. `crates/app` is the GPUI binary and only renders `client` state. The TS supervisor stays the backend for this slice and is ported to Rust in the next plan; the client's `Api` is the seam where the in-process Rust core replaces it.

**Tech Stack:** Rust, `gpui`/`gpui_platform` (rev `654efae4…`), `ureq` 2, `serde`/`serde_json`.

**Spec:** `docs/superpowers/specs/2026-11-05-gpui-rust-rewrite-design.md` (phase 2, vertical slice). Interim deviation, ledgered: the GPUI UI talks to the existing supervisor first; the all-Rust end state is unchanged.

## Global Constraints

- No Tauri, React or webview in this slice. Linux X11 first; macOS must stay possible.
- Supervisor contract (from `apps/desktop/src-tauri/src/lib.rs` and `apps/ui/src/api.ts`): run `node packages/supervisor/src/main.ts` with `HARNESS_PARENT_STDIN=1`, stdin piped (closing it stops sidecars), read stdout for a line `HARNESS_READY {"port":N,"token":"…"}`; every request sends header `x-harness-token`; events at `GET /api/events?token=…` (SSE, one JSON `UiEvent` per `data:` line).
- Routes used: `GET /api/state`, `GET /api/ws/:id/snapshot`, `GET /api/ws/:id/chats/:cid/messages` → `{messages}`, `POST /api/ws/:id/chats/:cid/send`, `POST /api/ws/:id/chats` (new chat), `POST /api/workspaces/:id/activate`.
- GPUI is Apache-2.0: any code copied from Zed examples keeps its attribution header.
- `default-members` excludes `crates/app` so `cargo test` needs no GUI libraries; build the app with `cargo run -p workspaceai-app`.

## Review Focus

- SSE stream drops or the supervisor exits: the app shows a disconnected state and retries, never panics.
- Token or port missing in the ready line, or a non-JSON line: launcher fails with a readable error.
- A message arriving for a chat that is not open must not corrupt the open chat.
- Empty/whitespace composer text must not be sent.
- Very long messages and non-ASCII text render and scroll without panic.

---

### Task 5: `workspaceai-client` crate: ready line + API client

**Files:** Create `crates/client/Cargo.toml`, `src/lib.rs`, `src/api.rs`, `src/supervisor.rs`; Test `crates/client/tests/api.rs`, `tests/supervisor.rs`; Modify root `Cargo.toml` (`default-members`).

**Interfaces — Produces:**
- `supervisor::parse_ready(line: &str) -> Result<Info, String>` where `Info { port: u16, token: String }`; accepts only `HARNESS_READY {json}`.
- `supervisor::Supervisor::start(root: &Path, node: &Path) -> Result<(Info, Supervisor), String>`; `Drop` closes stdin, SIGTERMs, waits ≤8 s then kills (port of `SupervisorProc::stop`).
- `api::Api { base: String, token: String }` with `Api::new(info: &Info)`, `get/post/patch/del(&self, path: &str, body: Option<&serde_json::Value>) -> Result<serde_json::Value, ApiError>`; `ApiError { status: u16, message: String }` (message from the JSON `err` field, else status text).

- [ ] Tests first: `parse_ready` ok / missing prefix / bad json / missing port / missing token; `Api` against a one-shot fake `TcpListener` server asserts the `x-harness-token` header is sent, JSON body is posted, a 200 JSON body is returned, a 400 `{"err":"nope"}` becomes `ApiError{400,"nope"}`, and a non-JSON error body becomes the message text.
- [ ] Run → FAIL. Implement. Run → PASS. Commit `feat(client): supervisor launcher and API client`.

### Task 6: typed state and events

**Files:** Create `crates/client/src/types.rs`, `src/events.rs`; Test `tests/types.rs`, `tests/events.rs`.

**Interfaces — Produces:** serde structs (camelCase) `AppState{workspaces: Vec<WorkspaceSummary>}`, `WorkspaceSummary{id,name,kind,initials,accent,note}`, `Snapshot{projects: Vec<Project>, chats: Vec<Chat>, chat_activity: HashMap<String,String>}`, `Project{id,name,parent: Option<String>,archived,is_general: Option<bool>}`, `Chat{id,project,title,archived,updated_at}`, `ChatMessage{id,role,text,tools: Vec<ToolStep>,error: Option<String>,created_at}`, `ToolStep{name,status,detail}`; `UiEvent` enum tagged by `type` with the variants `state, snapshot, chat.delta, chat.tool, chat.done, chat.permission, toast` plus `Other` catch-all for the rest (unknown events must not error); `events::parse_sse_line(&str) -> Option<UiEvent>` (only `data: {json}` lines); `events::stream(api: &Api, on_event: impl FnMut(UiEvent)) -> Result<(), String>` reading `/api/events` line by line.

- [ ] Tests first: parse fixtures copied from real supervisor shapes for each type (missing optional fields tolerated); `parse_sse_line` ignores comments/blank/invalid; unknown event type → `UiEvent::Other`; `stream` against the fake server yields two events then returns Ok on EOF.
- [ ] Run → FAIL. Implement. Run → PASS. Commit `feat(client): typed state and SSE events`.

### Task 7: `workspaceai-app` shell window

**Files:** Create `crates/app/Cargo.toml`, `src/main.rs`, `src/model.rs`; Test `crates/app/tests/model.rs`.

**Interfaces — Produces:** `model::Model` (plain data, no GPUI): `workspaces`, `active_ws`, `snapshot`, `open_chat`, `messages`, `live: HashMap<chat, streaming text>`, `status: Connection{Starting|Connected|Disconnected(String)}`; `Model::apply(&mut self, UiEvent)` (a `chat.delta` appends to the open chat's last assistant message, ignores other chats; `chat.done` clears activity) and `Model::sidebar(&self) -> Vec<SidebarRow>` (non-archived projects nested by `parent`, each with its chats newest first; the General project first).
`main.rs`: `--smoke` flag starts the supervisor, fetches `/api/state` and prints `SMOKE ok: N workspaces`, then exits; without it opens the GPUI window.

- [ ] Tests first (model only): delta appends to open chat; delta for another chat ignored; done clears activity; sidebar nests subprojects, hides archived, orders chats by `updated_at` desc, General first; apply of `snapshot` marks a refresh needed.
- [ ] Run → FAIL. Implement model. Run → PASS.
- [ ] Wire `main.rs`: supervisor thread + event thread push into a channel; GPUI polls it on a 50 ms timer and calls `Model::apply`/refresh. Build with `cargo build -p workspaceai-app`.
- [ ] Verify `cargo run -p workspaceai-app -- --smoke` prints `SMOKE ok`. Commit `feat(app): model and smoke run`.

### Task 8: UI: rail, sidebar, chat, composer

**Files:** Create `crates/app/src/ui/{rail,sidebar,chat,composer,input}.rs`, `ui/mod.rs`; modify `main.rs`.

- [ ] Workspace rail (initials chips, click activates via `POST /api/workspaces/:id/activate` and reloads the snapshot), sidebar (project tree + chats, click opens chat and loads messages), chat view (user/assistant bubbles, tool steps as one-line rows, error text, scrolls, shows streaming text), composer (single/multi-line input adapted from Zed's `crates/gpui/examples/input.rs`, Apache-2.0 header kept; Enter sends, Shift+Enter newline; blank text not sent), "New chat" button (`POST …/chats`), status strip showing `Connection`.
- [ ] Verify: `cargo build -p workspaceai-app`; run the window and send a message to a real chat (owner to confirm visually; the executor can only confirm the build, the smoke run and the model tests).
- [ ] Commit `feat(app): rail, sidebar, chat and composer`.

---

## Self-review

- Spec coverage: phase 2 slice (shell, sidebar, chat, composer) and the owner's choice of vertical slice first. Settings, onboarding, palette, library/routines/approvals/memory/computers/projects screens, artifacts (Open Design view) and the Rust supervisor are separate later plans and are listed as not done.
- Interfaces consistent: `Info`/`Api` (Task 5) used by `events::stream` (Task 6) and `Model` (Task 7).
- Known limit: UI correctness beyond build and model tests needs the owner's eyes; stated in Task 8.
