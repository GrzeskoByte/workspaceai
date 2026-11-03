# opencode 2.2.1 server — verified behaviour

Probed on 2026-10-29 with `opencode serve --hostname 127.0.0.1 --port N` and `OPENCODE_SERVER_PASSWORD`.
Full spec: [opencode-openapi.json](opencode-openapi.json) (fetched from `/openapi.json`).

- **Auth**: HTTP basic, user `opencode`, password from `OPENCODE_SERVER_PASSWORD`. Without it every `/api/*` route returns 401.
- **Routes live under `/api`**. Unknown paths fall through to opencode's own web UI (HTML), so health checks must use `/api/info`
  (`{version, pid, urls, paths, capabilities}`).
- **Isolation**: data, config, state and cache follow `XDG_*_HOME`. A workspace server started with its own XDG dirs keeps its own
  `opencode.db`, so sessions never cross workspaces. Config is read from `$XDG_CONFIG_HOME/opencode/opencode.json`.
- **Sessions**: `POST /api/session {title, location:{directory}, model:{id, providerID}}` → `{data: Session.Info}`.
  `GET /api/session?directory=…`, `PATCH /api/session/:id {title, archived}`, `DELETE /api/session/:id`,
  `POST /api/session/:id/fork {before?}`, `POST /api/session/:id/interrupt` (Stop).
- **Prompt**: `POST /api/session/:id/prompt {text}` returns immediately; `POST /api/experimental/session/:id/wait` blocks until idle.
- **Messages**: `GET /api/session/:id/message` → `{data:[…]}` newest first; types `user {text}`, `assistant {content:[{type:'text',text}], tokens, cost}`, `idle {outcome}`, plus agent/model switches, shell, compaction.
- **Events**: `GET /api/event` is SSE, `data: {id, type, created, location, data}`. Seen: `session.created`, `session.execution.started|succeeded`,
  `session.step.started|streamed|ended`, `session.text.started|delta|ended` (`data.delta`), `session.usage.updated`, `session.inbox.*`,
  `project.updated`, `provider.updated`, `model.updated`. Heartbeats are SSE comments.
- **Models**: `GET /api/model` lists models for the location; `GET /api/model/default`. Free `opencode/*` models (for example `opencode/big-pickle`) answer without a key.
- **Providers/MCP/config**: `GET /api/provider`, `GET /api/mcp`, `PUT|DELETE /api/experimental/mcp/:server`, `GET /api/config`, `POST /api/location/reload`.
- **Permissions**: `GET /api/session/:id/permission`, `POST /api/session/:id/permission/:req/reply {decision: once|always|reject}`.
- **Import/export**: `GET /api/experimental/session/:id/export`, `POST /api/experimental/session/import {info, messages, location}`.
- **TUI attach** (v2 CLI): `opencode --server <url> --session <id> <directory>`.
- **ACP**: `opencode acp` starts an Agent Client Protocol server over stdio.
