# Harness — Design Document
2026-10-28 · @Greg
## Summary
The harness is a desktop app modelled on Claude's desktop app. It does no agent work itself: **opencode** runs the agent and **Open Design** makes and manages the artifacts. The harness adds what neither has: isolated workspaces (personal vs organization), a project → chat hierarchy, layered instructions, memory, and a loop builder.
| Layer | Owner | Responsibility |
| Shell and UX | Harness | Workspaces, projects, chats, settings, themes, loops, memory |
| Agent runtime | opencode | Sessions, tools, model providers, MCP, agents, file edits |
| Artifacts | Open Design | Brief → direction → design system → artifact, preview, critique, export, handoff |

**Goals**
- Switch workspaces at any moment, each with its own chats, memory, connectors, MCP servers and model providers.
- Let an organization workspace use only the providers its admin allows, including custom on-prem endpoints.
- Create, edit, version and export documents, decks, prototypes, sheets and media as first-class artifacts.
- Build repeatable agent loops as graphs and run them on demand or on a schedule.
**Non-goals for v1**
- Forking opencode or Open Design; both are run unmodified and upgraded on their own schedules.
- Real-time multi-user editing inside the harness (team sharing goes through artifact links).
- A hosted cloud service; everything runs on the user's machine, with org policy pulled from the organization.
## Requirements
Every requirement given for this project, written so it can be checked. Covered in points to the section of this document that specifies it and to the place in the prototype on the design canvas.
### Platform
| ID | Requirement | How to check | Covered in |
| R-01 | A desktop interface modelled on Claude's desktop app: workspace rail, sidebar with projects and chats, chat view, artifact side panel, settings. | All five surfaces exist and work together in one window. | UI map · Prototype main window |
| R-02 | Universal: not tied to one model vendor; any opencode provider, local model or MCP server can be used. | A workspace runs with Anthropic, Ollama and a custom OpenAI-compatible endpoint. | opencode as the harness |
| R-03 | opencode is the harness: every session, tool call, model request, MCP call and agent runs through `opencode serve`, used unmodified. | No agent work happens outside opencode; upgrading opencode needs no harness fork. | opencode as the harness |
| R-04 | Open Design is the artifacts manager, used unmodified through its daemon and MCP tools. | Every artifact is created, versioned and exported by Open Design. | Open Design as the artifacts manager |

### Workspaces
| ID | Requirement | How to check | Covered in |
| R-05 | The user can switch workspaces at any moment (rail, ⌘K, ⌘1–9) without restarting. | Switching mid-session keeps the other workspace's chats running. | Workspaces and isolation · rail, ⌘K |
| R-06 | Each workspace has its own chats and history, memory, connectors, MCP servers, model providers, secrets, config and data folders, and its own opencode server. | Nothing made in one workspace (chat, fact, key, connector) is visible from another. | Workspaces and isolation |
| R-07 | Personal and organization workspaces are distinct: an organization workspace can use different model providers from personal, chosen by its admin; personal keys cannot be used there. | The org workspace offers only its allowed providers; locked rows say why. | Workspaces and isolation · Settings › Model providers |
| R-08 | An organization workspace is joined later through SSO from + in the rail, not during first-run setup. | Onboarding has no work step; + offers Organization (SSO). | Onboarding · New workspace |

### Projects, chats and artifacts
| ID | Requirement | How to check | Covered in |
| R-09 | Hierarchy is workspace → project → chat. | Every chat shows its project path. | Projects and nesting |
| R-10 | A chat is never stranded: it always belongs to a project. Quick chats go to the workspace's General project; a chat can be moved but not detached; a project with chats cannot be deleted until they are moved or archived with it. | No state in the app shows a chat without a project. | Projects and nesting · Home project picker, chat menu |
| R-11 | Projects support sub-projects that nest like folders, to any depth. | A sub-project can be added under any project from the tree. | Projects and nesting · tree (+) |
| R-12 | Custom instructions at workspace, project and sub-project level (AGENTS.md); lower levels inherit from parents and the narrowest layer wins; inheritance can be turned off per project. | A chat in a sub-project starts with all inherited layers, shown on the Instructions tab. | Memory, instructions and loops · Project › Instructions |
| R-13 | Artifacts belong to projects too: an artifact is placed in the project or sub-project of the chat it came from, appears under that chat in the tree, moves with the chat, and rolls up into parent projects' Artifacts tab. Artifacts made from the library must pick a project. | A doc made in a sub-project's chat appears inside that sub-project. | Artifacts belong to projects too · tree, Project › Artifacts |
| R-14 | Artifacts include documents, presentations, prototypes, sheets and media, with versions, export and sharing. | Each type can be made from a chat and from the library, opened in the panel, versioned and exported. | Open Design as the artifacts manager · artifact panel |
| R-15 | Artifact creation follows Open Design's pipeline: brief → questions → direction → design system → generate → critique → handoff, with DESIGN.md design systems, templates, sandboxed preview, tweaks and exports. | New artifact runs every stage and can hand off to a project chat. | Open Design as the artifacts manager · New artifact, Generating |

### opencode integration
| ID | Requirement | How to check | Covered in |
| R-16 | Chats are opencode sessions, one to one; the session id is visible and fork, rename and delete map to the session API. | The chat header shows `ses_…`; a fork creates a new session. | Sessions are chats · chat header and menu |
| R-17 | opencode can be started by the harness for each workspace or connected to an already running server (URL, username, password, connection test). | Both modes in onboarding and Settings › Engine; connecting requires a successful test. | Starting or connecting opencode · Onboarding, Engine |
| R-18 | Existing opencode sessions can be imported at first run; their folders become projects and nested folders become sub-projects; any folder or session can be skipped. | Imported chats land in the right projects with their history. | Sessions are chats · Onboarding › Sessions |
| R-19 | Sessions stay in sync with the opencode TUI both ways, and any chat can be opened in the TUI. | A session started in the TUI appears in harness; "Open in opencode TUI" gives the attach command. | Sessions are chats |
| R-20 | Custom model providers are configurable per workspace and written to that workspace's opencode.json; keys are referenced, never stored in the file; connections can be tested; providers already set up in opencode can be reused. | Add custom provider writes a valid `provider` block and passes the test. | opencode as the harness · Settings › Model providers |
| R-21 | Web search is available as an option per workspace: off, opencode built-in (Exa), an MCP search provider (Tavily, Brave, Firecrawl) or self-hosted SearXNG, plus page reading (webfetch) and ask-before-search; each chat can turn it off; an organization can lock the choice; answers list the pages used. | Onboarding › Web search and Settings › Web search set it; the 🌐 Web chip turns it off per chat. | Web search · Onboarding, Settings, composer |

### Memory
| ID | Requirement | How to check | Covered in |
| R-22 | Memory per workspace and per project; the user approves what is remembered and can add or forget facts. | "Remember this" and the Memory tab add and remove facts in the right scope. | Memory, instructions and loops · Memory |
| R-23 | The memory backend is chosen per workspace: Facts, Graphify, Graphify + facts, or a custom memory MCP. Graphify maps chosen folders, can watch for changes, is queried over MCP, shows communities and sends AMBIGUOUS links to a review queue, using only the workspace's allowed providers. | Each workspace can show a different backend; the Memory screen queries and rebuilds the graph. | Memory, instructions and loops · Onboarding › Memory, Memory screen |

### Automation
| ID | Requirement | How to check | Covered in |
| R-24 | Loops and graphs are built by drag and drop from a palette of predefined blocks chosen from graph-engineering research. | A graph can be built without typing code. | Graph engineering with BPMN 2.0 · Graph editor |
| R-25 | Graph modelling follows BPMN 2.0 and is clearly defined: element set, lanes, sequence and data flows, model-check rules, token execution semantics, BPMN XML import and export, harness extension for agent tasks. | The editor rejects a model that breaks a rule; export opens in a standard BPMN tool. | Graph engineering with BPMN 2.0 |
| R-26 | Ready-made agent patterns: prompt chain, router, parallel + vote, orchestrator–workers, evaluator–optimizer, human in the loop. | Each loads as a valid BPMN graph. | Agent patterns as BPMN · Graph editor toolbar |
| R-27 | Schedules and routines: written in plain words and stored as cron with a time zone, or triggered by events; tied to a workspace and project; runs on the cloud, a linked computer or an org runner; approval and notification settings; week view, run history, run now, pause. | A routine created in words shows its parsed schedule and appears on the week view. | Routines and schedules · Routines |
| R-28 | Computer management: pair, rename, wake and unlink computers; set capabilities, allowed apps, per-workspace access and folders, and ask-first actions; see activity and a live view that can be stopped or taken over. | Each control on the Computers screen changes its state. | Computers · Computers |

### Experience and design
| ID | Requirement | How to check | Covered in |
| R-29 | First-run onboarding is fully designed: opencode, sessions, workspace, model, web search, memory, first project with AGENTS.md, and look; each step validates; setup can be skipped to demo data; everything can be changed later in Settings. | Every step can be completed or skipped and finishes in a working app. | UI map · Prototype onboarding |
| R-30 | Visual language from the supplied DESIGN-claude.md (Claude Warm Editorial) is the default; the opencode palette is an alternate theme; all colours are tokens; each workspace has an accent. | Switching theme recolours every screen. | UI map · Claude Warm Editorial design system |
| R-31 | The design is interactive and animated: every element does something and the whole flow works end to end; motion honours Reduce motion. | No dead controls in the prototype. | Prototype |
| R-32 | Every page is designed: onboarding, home, chat, artifact panel and library, project tabs, all settings sections, graph editor, routines, computers, memory, new workspace, command palette. | Each page in the UI map is reachable in the prototype. | UI map |
| R-33 | A design document covers architecture, requirements, every subsystem, the UI map, security, the delivery plan and sources, and is updated with each design change. | This document. | — |

### Security and data
| ID | Requirement | How to check | Covered in |
| R-34 | Secrets live in the OS keychain per workspace; config files only reference them. | No key appears in any opencode.json. | Security and data |
| R-35 | Managed opencode servers bind to loopback with a password; remote servers are reached over HTTPS or an SSH tunnel. | A managed server refuses connections from another machine. | Starting or connecting opencode · Security and data |
| R-36 | Organization workspaces follow the org's signed policy (providers, sharing, retention, logging, residency) and keep an audit log. | Locked settings cannot be changed; actions appear in Settings › Audit log. | Workspaces and isolation · Security and data |

### Open-source components
| ID | Requirement | How to check | Covered in |
| R-37 | The graph editor is built on bpmn-js, with the harness element set, inspector and a moddle extension for `harness:` attributes; the bpmn.io logo stays visible on the canvas, as its licence requires. | XML saved by the editor opens in Camunda Modeler or bpmn.io. | Components we build on · Graph editor |
| R-38 | Graphs are executed by bpmn-engine: state is saved after every activity, a run can be paused, resumed and recovered after a crash, and user tasks wait for a signal. | Quitting the app mid-run and reopening it resumes from the last completed step. | Components we build on · Graph editor › Run |
| R-39 | The harness is an ACP client: each workspace lists the ACP agents it may use, chats and agent tasks can run on them, they still belong to projects, file and terminal access is limited to the project folder, and permission requests appear in the chat. An organization can restrict the list. | A chat on an ACP agent streams replies and asks before writing files. | Components we build on · Settings › Agents (ACP), agent menu |
| R-40 | LiteLLM proxy is used only in its MIT-licensed form (nothing under its `enterprise/` folder) as the organization's model gateway and as a local provider preset. | The org workspace reaches all allowed models through one gateway URL. | Components we build on · Settings › Model providers |
| R-41 | Browser steps in graphs and browsing in chats use Playwright MCP. | A Browser step opens a page and returns what it read. | Components we build on · Graph editor palette, Connectors |
| R-42 | The desktop shell is Tauri 2; opencode, the Open Design daemon and the orchestrator run as sidecars it starts and stops. | Quitting the app stops every sidecar. | Components we build on · Settings › Engine |
| R-43 | Routines written in plain words are parsed with chrono-node, plus a harness layer for repeats, and the interpretation (schedule, next run, time zone, matched words) is shown before saving. | “tomorrow at 9”, “every 2 hours 9–18 weekdays” and “first day of each month at noon” parse correctly. | Components we build on · Routines |
| R-44 | The ⌘K palette is built on cmdk: grouped results, fuzzy match, arrow keys, Enter and Esc; it searches only the current workspace (plus workspace names). | Every action is reachable from the keyboard. | Components we build on · Command palette |
| R-45 | Only open-source components are used, and their licence obligations are recorded and met (for example the bpmn.io logo, and AGPL tools such as SearXNG run as separate, unmodified services). | The components table lists each licence and obligation. | Components we build on |

**Changed along the way.** The opencode palette was the first visual direction and is now the alternate theme (R-30). Graphs started as a custom block language and are now BPMN 2.0 (R-25). Joining an organization was an onboarding step and is now done from + in the rail (R-08).
## Architecture
[embed {'id': '.14118', 'h': '77932c61', 'caption': 'architecture · shell, supervisor, two isolated workspaces', 'ref': 'node/1c8b531f-d8d5'}]
The shell talks only to the supervisor and to the active workspace's processes. Inside a workspace, opencode is the hub: Open Design uses it as its agent and exposes artifacts back to it over MCP, the memory service is another MCP server, and the loop runner drives opencode sessions. Model calls leave only through the providers that workspace allows.
## Components we build on
The harness keeps its own code to the shell, the workspace supervisor and the glue. Everything else is an existing open-source project, used unmodified.
| Component | Role in the harness | Licence | Obligations and limits | Requirement |
| opencode | Agent runtime, sessions, providers, MCP | MIT | — | R-03 |
| Open Design | Artifacts manager | see Open questions | Licence still to confirm | R-04 |
| bpmn-js | BPMN modeller in the graph editor | bpmn.io licence (MIT-style) | The bpmn.io logo must stay visible and uncovered | R-37 |
| bpmnlint | Model-check rules for graphs | MIT | — | R-25 |
| bpmn-engine | Executes BPMN graphs | MIT | — | R-38 |
| Agent Client Protocol (ACP) | Connects other coding agents | Open specification | Each agent has its own licence | R-39 |
| LiteLLM proxy | Organization model gateway; local preset | MIT, except its `enterprise/` folder | We use none of the enterprise features | R-40 |
| Playwright MCP | Browser steps and browsing | Apache-2.0 | — | R-41 |
| Tauri 2 | Desktop shell and sidecars | MIT / Apache-2.0 | — | R-42 |
| chrono-node | Plain-words dates and times for routines | MIT | — | R-43 |
| cmdk | ⌘K command palette | MIT | — | R-44 |
| Graphify | Memory backend option | see Open questions | Licence still to confirm | R-23 |
| SearXNG | Self-hosted web search option | AGPL-3.0 | Run as a separate, unmodified service | R-21 |

### Graph editor: bpmn-js
- The editor is a bpmn-js Modeler. Its palette provider offers exactly the harness element set, including the Browser step and the agent task; its context pad gives connect, replace and delete; the harness inspector replaces the stock properties panel.
- A moddle extension defines the `harness:` namespace (`type`, `runner`, `agent`, `model`, `tools`, `prompt`, `askFirst`, `maxLoops`, `mcp`), so these attributes survive `saveXML` and `importXML` and other tools ignore them.
- Lanes are bpmn-js lanes in one participant. Files imported without diagram layout are laid out with bpmn-auto-layout.
- Model-check rules from Graph engineering with BPMN 2.0 are implemented as a bpmnlint plugin, so the same rules run in the editor, before a run and in CI.
- The canvas is styled with the design system's tokens. The bpmn.io logo stays visible in a corner of the canvas.
### Graph runs: bpmn-engine
- The orchestrator (a Node sidecar) loads the saved XML into bpmn-engine and runs it with a harness extension that reads the `harness:` attributes.
- Services map tasks to work: an agent task becomes a prompt in an opencode session or, with `harness:runner="acp:<id>"`, in an ACP agent; a service task calls one MCP tool; a Browser step calls Playwright MCP; a script task runs in the sandbox; send and receive tasks use connectors.
- Conditions on sequence flows are engine expressions over the run variables. Timers and boundary events use the engine's own support.
- A user task raises `activity.wait`; the harness shows it in the chat and the Routines view and calls `signal()` with the answer. A rejection ends the run at that task.
- After every `activity.end` the orchestrator saves `engine.getState()` to `.harness/runs/<run-id>.json`. Pause stops the engine and keeps that state; resume and crash recovery use `recover(state)` followed by `resume()`.
- A dry run is the same engine with every service replaced by sample outputs, so it follows the exact path a real run would.
### Other agents: ACP
- The harness is an ACP client. For a chat or agent task on an ACP agent it starts the agent's command as a subprocess in the project folder and speaks JSON-RPC over stdio: `initialize`, `authenticate` if needed, `session/new` (with the workspace's MCP servers, so memory and connectors stay per workspace), `session/prompt`, `session/cancel` for Stop.
- `session/update` notifications stream message chunks, tool calls and plans into the chat. `session/request_permission` becomes an approval in the chat. The harness offers the client file and terminal capabilities, limited to the project folder.
- Chats on ACP agents still belong to a project. The harness keeps their transcript; reopening uses `session/load` when the agent supports it and otherwise starts a new session with the saved history as context.
- opencode itself stays on `opencode serve`, which exposes config, providers and events that ACP does not; `opencode acp` exists and is used to test parity.
- Agents are listed per workspace in Settings › Agents (ACP), with one default. An organization policy can allow only opencode.
### Organization gateway: LiteLLM proxy
- Only the MIT-licensed proxy is used: one OpenAI-compatible endpoint in front of many backends, with routing, fallbacks, retries and load balancing.
- Nothing under LiteLLM's `enterprise/` folder is used. LiteLLM lists SSO, audit logs, RBAC, virtual keys and budgets as enterprise additions, so SSO stays with the organization's identity provider (for example Keycloak), and auditing stays in the harness audit log.
- The organization runs the proxy; its workspace's opencode.json points at it as an `@ai-sdk/openai-compatible` provider with a key reference. Personal users can run it locally through the LiteLLM proxy preset (`http://localhost:4000/v1`).
### Browser: Playwright MCP
- Added to a workspace's `mcp` block as a local server (`npx @playwright/mcp`), headless by default, with a browser profile folder per workspace so sign-ins never cross workspaces.
- Graphs use it through the Browser step (`harness:type="browser"`); chats can use it like any other MCP tool, subject to the workspace's permissions.
### Desktop shell: Tauri 2
- Tauri 2 provides the window, menus, deep links (for the SSO callback), updates and access to the OS keychain from Rust.
- opencode ships as a sidecar binary; the Open Design daemon and the orchestrator run as Node sidecars. The supervisor starts one set per workspace, and quitting the app stops them all.
- This replaces Open Design's own Electron shell, which the harness does not use.
### Routines: chrono-node
- chrono-node turns phrases such as “tomorrow at 9”, “next Monday at 10” or “in 3 hours” into a date and time. A small harness layer handles repeats (“every”, “weekdays”, “weekends”, “every 2 hours 9–18”, “first day of each month”) and events (“on push to main”, “when an email arrives”).
- The editor shows what it understood — schedule, cron, once or repeating, next run, time zone and the matched words — before saving.
### Command palette: cmdk
- The ⌘K palette is a cmdk command menu: results grouped as Actions, Workspaces, Projects, Chats, Artifacts and Screens; fuzzy matching; ↑↓, Enter and Esc.
- It searches only the current workspace, plus the list of workspace names, so nothing from one workspace shows up while you are in another.
## Workspaces and isolation
A workspace is a sealed unit: its own opencode server, its own Open Design daemon, its own folders and its own secrets. Switching workspaces changes which pair of processes the UI talks to; nothing is copied across.
| What | Where it lives | Shared across workspaces? |
| Config | `~/.harness/ws/<id>/config/` (opencode.json, harness.json, policy.json) | No |
| Chats | opencode sessions in `~/.harness/ws/<id>/data/opencode/` | No |
| Artifacts, design systems, templates | Open Design data dir `~/.harness/ws/<id>/data/open-design/` | No |
| Memory | `~/.harness/ws/<id>/memory/` (workspace) and `<project>/.harness/memory/` (project) | No |
| Secrets | OS keychain, one entry prefix per workspace | No |
| MCP servers and connectors | `opencode.json` `mcp` block of that workspace | No — copying a config brings the definition, never the token |
| Theme and window layout | `~/.harness/app.json` | Yes (app-level only) |

**Personal vs organization**
- **Personal**: the user owns everything. Any opencode provider, own API keys, local models, any MCP server.
- **Organization**: joined through SSO (OIDC; e.g. MyAccessID via the org's Keycloak). The org publishes a signed `policy.json`: allowed providers and models, blocked features (personal keys, external sharing), data residency, retention and logging. The harness enforces it by writing the workspace's opencode.json from policy and refusing edits outside it.
- **Team**: a shared space the user manages, without an org policy.
Each workspace runs as its own OS process group with its own environment, so one workspace's environment variables (API keys, proxy settings) never reach another's tools.
## Projects and nesting
Every chat, loop, routine and artifact belongs to exactly one project; there are no loose chats. Each workspace has a **General** project that quick chats land in, and the composer always shows which project a new chat goes to.
Projects nest like folders. A project maps to a folder on disk; a sub-project is a sub-folder promoted to a project, and it can hold its own sub-projects.
| A sub-project… | Behaviour |
| Instructions | Inherits every parent's AGENTS.md, then adds its own; the narrowest wins on conflict |
| Model and connectors | Inherits the parent's defaults unless it overrides them |
| Memory | Gets its own scope inside the workspace's memory; searches widen to parents on request |
| Chats and loops | Listed under it in the sidebar tree; the parent's view can include descendants |
| Moving | Drag it onto another project; its folder moves with it |
| Archiving | Hides it and everything inside; nothing is deleted |

In opencode terms a chat runs with its project's folder as the working directory, so opencode reads that folder's AGENTS.md; the harness prepends the parent layers.
### Artifacts belong to projects too
An artifact is never loose either. It is stored with two links: **project** (where it lives) and **origin chat** (the opencode session that made it, if any).
- **Placement follows the chat.** An artifact made in a chat goes to that chat's project, at any depth. A doc made in a chat in Castform › Simulation engine is a child of the Simulation engine sub-project, not of Castform.
- **Shown under its chat.** In the sidebar tree the artifact appears directly beneath the chat that made it (↳), and the chat row shows how many artifacts it produced. Artifacts started from the library (no chat) ask for a project and sit directly under it.
- **Moves with the chat.** Moving a chat to another project moves its artifacts with it; the toast says how many moved. Moving an artifact on its own is also allowed and keeps the origin-chat link.
- **Rolls up.** A project's Artifacts tab lists its own artifacts and those of its sub-projects, each labelled with its sub-project path and "from 💬 chat". The library can filter by project, including sub-projects.
- **Provenance in the viewer.** The artifact panel header links to the project path and to the origin chat, so the conversation that produced a version is one click away. Revisions made from a different chat keep the original project and add that chat to the version history.
- **Inheritance.** Generation uses the instructions, memory and design system of the artifact's project, so an artifact in a sub-project follows the sub-project's AGENTS.md and overrides.
- **Storage.** Open Design keeps the files in the project folder (`.harness/artifacts/<id>/`) with a manifest holding `project`, `originSession` (`ses_…`) and versions, so moving a project folder carries its artifacts along.
## opencode as the harness
Every chat, tool call and model request goes through `opencode serve`, one instance per workspace bound to `127.0.0.1` on its own port and protected with `OPENCODE_SERVER_PASSWORD`. The harness is a client of its HTTP API, the same way opencode's own TUI is.
| Harness concept | opencode mechanism |
| Chat | A session: create, list, fork, abort, revert, summarize; messages via `/session/:id/message` or `/prompt_async` |
| Live streaming, tool steps, permissions | `GET /event` SSE stream; permission requests answered through the session API |
| Model picker | `GET /config/providers` (providers and default models) and `GET /provider` |
| Custom provider | `provider` block in the workspace's opencode.json, written by the harness, then `PATCH /config` |
| Project | The folder opencode runs in; `AGENTS.md` at its root holds the project instructions |
| Connectors / MCP | The `mcp` block of opencode.json, per workspace |
| Loop agent node | An opencode agent (model, prompt, tools); exportable as an agent file |
| Health | `/global/health` for status and version; `/doc` for the OpenAPI spec the client is generated from |

**Custom providers.** The Add custom provider screen writes exactly what opencode reads:
```
{
  "provider": {
    "ai-factory": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "AI Factory inference",
      "options": { "baseURL": "https://[INFERENCE_ENDPOINT]/v1", "apiKey": "{env:AI_FACTORY_TOKEN}" },
      "models": { "llama-4-maverick": { "name": "Llama 4 Maverick", "limit": { "context": 131072, "output": 8192 } } }
    }
  }
}
```
Keys never enter the file: `apiKey` holds an `{env:…}` or `{file:…}` reference, and the harness injects the value from the keychain into that workspace's process environment. `@ai-sdk/openai` covers `/v1/responses` endpoints; any other AI SDK package can be named in `npm`.
**Lifecycle.** The workspace supervisor starts the server on first use, health-checks it, restarts it on crash with backoff, and stops it after a configurable idle time. Upgrading opencode is a version pin per workspace, so an org can hold a version while personal moves ahead.
### Starting or connecting opencode
A chat in harness is an opencode session, so each workspace chooses where its sessions live:
| Mode | What harness does | When to use it |
| Managed (default) | Starts `opencode serve` for the workspace on 127.0.0.1 with a random `OPENCODE_SERVER_PASSWORD`, using the opencode on this computer or a bundled build | Most people; one server per workspace keeps workspaces isolated |
| Connect to a server | Talks to an `opencode serve` you already run, local or remote, with basic auth (username, default `opencode`, and password from the keychain); health-checked through `/global/health` | You already use opencode, run it on a dev box or GPU host, or want the TUI and harness on the same sessions |

Remote servers must be reached over HTTPS or an SSH tunnel. Switching mode in Settings › Engine restarts the workspace's connection; chats stay because they belong to the server.
### Sessions are chats
- **One to one.** Every chat maps to one session id (`ses_…`), shown in the chat header. Fork, rename, delete and revert call the session API directly.
- **Import on first run.** Onboarding lists the sessions opencode already has (from the connected server, or from this computer's opencode data in managed mode). Each session's working folder becomes a project and nested folders become sub-projects, so imported chats are never stranded. The user can skip any folder or session, or start fresh.
- **Two-way sync.** With sync on (default), sessions started in the opencode TUI appear in the right project in harness, and chats started in harness appear in the TUI. Updates arrive over the `/event` stream.
- **Open in the TUI.** Any chat can be continued in the terminal with `opencode attach <server-url> --session <id>`; the chat menu copies the command.
- **Providers come along.** When a server already has providers configured, onboarding offers From opencode as the model choice and reuses them instead of asking for keys again.
### Web search
opencode has two web tools: `webfetch` reads one URL and is on by default; `websearch` finds pages through Exa and is turned on with `OPENCODE_ENABLE_EXA=true` (it is on automatically for OpenCode Zen models). Each workspace picks one search backend, chosen in onboarding and in Settings › Web search:
| Choice | How it is wired | Notes |
| Off | `permission.websearch: deny` | `webfetch` can stay on for links the user pastes |
| Built-in (Exa) | `OPENCODE_ENABLE_EXA=true` in the workspace server's environment | No key; queries go to Exa |
| Search provider | A local MCP server in the `mcp` block (Tavily, Brave or Firecrawl), key injected from the keychain as an environment variable | Better control over results and costs |
| Self-hosted SearXNG | An MCP server pointed at the user's or org's SearXNG URL | Queries stay on that network; the default for organization workspaces |

- Permissions are per workspace: allow, or ask before each search or fetch (opencode's `ask` permission).
- A **🌐 Web** toggle in the composer turns search off for a single chat; the reply then says it answered without the web.
- Answers that used the web list the pages read under the reply, and the tool steps show each search and fetch.
- An organization can lock the choice (for example SearXNG only) through its policy; the toggle still lets a user turn search off for a chat.
## Open Design as the artifacts manager
Open Design owns every artifact: how it's made, previewed, critiqued, versioned and exported. The harness embeds its pipeline as the New artifact and Generating screens and points it at the workspace's opencode as its agent. Open Design already lists OpenCode among its supported agent CLIs, so no adapter beyond configuration is needed.
**Pipeline** (Open Design's, kept as is): brief → questions → direction → design system → artifact → critique → handoff → memory.
| Open Design piece | How the harness uses it |
| Local daemon (Express + SQLite on 127.0.0.1) | One per workspace, data dir inside the workspace folder; the harness proxies its API to the UI |
| Output modes (`od.mode`): prototype, deck, live artifact, image, video, design system | The artifact types in the library, next to the harness's own Doc and Sheet types |
| Design systems (`DESIGN.md` + tokens.css, manifest.json) | Per-workspace library; the Claude Warm Editorial system ships as the default; org systems stay in the org workspace |
| Design templates (by mode and scenario) | The template gallery on the New artifact screen |
| Skills (`SKILL.md`) | Merged with opencode skills in the workspace's Skills settings |
| Sandboxed iframe preview, tweaks manifest | Generating screen preview and Tweaks tab; live artifacts re-render without reload |
| Critique template (five-part self-review) | Critique tab, with Apply fixes |
| `/api/artifacts/save` and `/api/artifacts/lint` | Save becomes a new version; lint runs before Ready |
| Exports: single-file HTML, PDF, PPTX, ZIP, Markdown, MP4 | Export tab and the artifact's Export menu |
| Handoff to a coding agent | "Continue as code" opens a chat in the project with the files, run by opencode |
| MCP stdio server | Registered in the workspace's opencode.json, so any chat can list, open or update artifacts |

**Two ways an artifact is born.** From the New artifact screen (the full pipeline), or from a chat: when an opencode session asks for an artifact, it calls Open Design's MCP tools, and the result appears as an artifact card in the chat and in the library.
**Versions.** Every save is a version. The artifact view shows the history; restoring makes a new version rather than rewriting history.
Open Design's licence and API stability need checking before shipping (see Open questions).
## Memory, instructions and loops
**Instructions stack from broad to narrow; the narrower layer wins.**
- Workspace instructions — set in workspace settings, stored in harness.json, sent as a system prefix.
- Project instructions — the project's `AGENTS.md`, edited in place on the Project screen, read natively by opencode.
- Chat instructions — set from the composer's menu, stored on the session.
**Memory** is a harness service, one store per workspace with a project scope inside it, served to opencode as an MCP server (`memory.search`, `memory.read`, `memory.write`). Chats read memory on demand instead of stuffing it into every prompt. New memories arrive as suggestions the user approves on the Project › Memory tab; nothing crosses workspaces. Open Design's pipeline ends by offering to save design decisions (direction, accent, tone) to project memory.
**Memory backends are chosen per workspace.** Whatever the backend, opencode sees it as one MCP server.
| Backend | What it keeps | Good for |
| Facts (default) | Short statements the user approves, as Markdown files | Preferences, decisions, people |
| [Graphify](https://github.com/safishamsi/graphify) | A knowledge graph of the project folders: code parsed by tree-sitter (no model calls), docs and images read by an LLM pass, clustered into communities; stored as `graphify-out/graph.json` | "How does this fit together" questions in code-heavy projects |
| Graphify + facts | Both, behind one MCP server | Recommended for software projects |
| Custom MCP | Any memory MCP server the user runs | Teams with their own memory service |

Graphify specifics:
- Install per workspace (`graphifyy` package); `graphify opencode install` registers it with that workspace's opencode, and `python -m graphify.serve` exposes `query_graph`, `get_node`, `get_neighbors` and `shortest_path` over MCP.
- Updates: watch mode for code, git hooks on commit and branch switch, and a manual rebuild for docs; a SHA-256 cache means only changed files are reprocessed.
- The docs pass uses a model from the workspace's allowed providers, so an org workspace never sends documents outside its policy.
- Links Graphify marks AMBIGUOUS go to a review queue on the Memory screen; the user keeps or drops each one.
**Loops** are BPMN 2.0 processes saved in the project (`.harness/graphs/<name>.bpmn`); the element set, lanes, rules and execution are defined in Graph engineering with BPMN 2.0 below. Each run has its own opencode session, so the run is one reviewable transcript, and agent tasks can be exported as opencode agent files so the same step runs from the opencode TUI.
## Graph engineering with BPMN 2.0
Every loop and graph is a BPMN 2.0 process. Users drag elements from a palette into swimlanes and connect them; the harness stores the model as standard BPMN 2.0 XML and runs it with token semantics. Using an existing standard means one notation for people and agents, a precise meaning for each shape, and files that open in any BPMN tool (Camunda Modeler, bpmn.io). Agent patterns from Anthropic, LangGraph, AWS Step Functions and Dify are expressed as BPMN fragments, not as a custom block language.
### Pool and lanes
A graph is one pool (the process) with three lanes. The lane says who performs a step, which decides approvals and audit.
| Lane | Who acts | Typical elements |
| You | The user, through the harness UI | User tasks (approve, answer, edit), message start events |
| Agent · opencode | An opencode session in the project folder | Agent tasks, business rule tasks (judge, classify), sub-processes |
| Harness | The orchestrator itself, with no model call | Service, script, send and receive tasks, timers, data stores |

### Element set
| BPMN element | Notation | Meaning in the harness |
| Start event: none | Thin circle | Run by hand or from a chat |
| Start event: timer | Thin circle with clock | Scheduled by a routine (cron + time zone) |
| Start event: message | Thin circle with envelope | File change, git event, webhook or incoming message |
| Start event: conditional | Thin circle with lines | Starts when a memory or data condition becomes true |
| Intermediate timer (catch) | Double circle with clock | Wait for a duration or until a time |
| Intermediate message (catch / throw) | Double circle with envelope | Wait for, or send, a message |
| End event: none / error / terminate | Thick circle; with bolt; filled | Finish; finish with an error; stop every branch |
| Agent task (harness extension) | Rounded box, spark icon | One opencode prompt with the chosen agent, model and tools |
| User task | Rounded box, person icon | Approval, a question or an edit; pauses the token |
| Service task | Rounded box, gear icon | MCP tool, HTTP call, connector or computer action |
| Script task | Rounded box, script icon | Shell command or code in a sandbox |
| Business rule task | Rounded box, table icon | Judge or classify against a rubric; returns structured output |
| Send / receive task | Rounded box, envelope | Notify or wait for a reply on a channel |
| Call activity | Thick-bordered box | Runs another saved graph |
| Sub-process | Box with + marker | Embedded group of steps with its own start and end |
| Exclusive gateway | Diamond with × | Exactly one outgoing flow whose condition holds, else the default |
| Parallel gateway | Diamond with + | Split into all branches; as a join, wait for all |
| Inclusive gateway | Diamond with ○ | Every branch whose condition holds; join waits for those started |
| Event-based gateway | Diamond with pentagon | First of several events wins (reply or timeout) |
| Loop marker | ↻ on a task | Repeat while a condition holds, with a required maximum |
| Multi-instance |  |  |
| Boundary timer / error | Small circle on a task edge | Time-out or failure path out of a task |
| Data object / data store | Page / cylinder | Run variables; workspace memory or project files |
| Sequence flow | Solid arrow, optional condition, slash = default | Order of execution |
| Data association | Dotted arrow | What a task reads or writes |

### Agent patterns as BPMN
- **Prompt chain**: agent tasks in sequence, with an exclusive gateway as the quality gate and an error end event when it fails.
- **Router**: business rule task (classify) → exclusive gateway with one condition per route and a default flow.
- **Parallel + vote**: parallel gateway split → several agent tasks on different models → parallel join → business rule task (vote).
- **Orchestrator–workers**: agent task (plan) writes a task list → parallel multi-instance agent task over it → script task (merge).
- **Evaluator–optimizer**: agent task (draft) → business rule task (score) → exclusive gateway: pass → end; otherwise flow back to the draft. The back-flow must carry a loop limit.
- **Human in the loop**: a user task in the You lane with a boundary timer (remind or escalate) and an event-based gateway for reply versus timeout.
### Execution semantics
- A run places a token on the start event; the orchestrator moves tokens along sequence flows and executes each element as the token arrives.
- Gateways follow BPMN rules: exclusive takes the first true condition in order or the default; parallel joins wait for a token on every incoming flow; inclusive joins wait for the branches that were started.
- Run variables are the process data. Tasks read and write them through data associations, and the state is checkpointed after every element, so a failed run resumes from the last completed element.
- User tasks and any task marked ask-first pause the token until someone acts. Routines can pre-approve named tasks, never the whole graph.
- Boundary timers and errors interrupt the task and send the token down their own flow. An error with no boundary handler ends the run with an error.
- A dry run moves tokens with sample outputs and calls nothing, so the user can watch the path the run would take.
### Model check (blocks saving or running)
- Exactly one start event, at least one end event, and every element reachable from the start.
- Every non-end element has an outgoing flow; every event is connected.
- Each exclusive or inclusive gateway split has a condition on every outgoing flow or one default flow.
- Parallel splits and joins are balanced; a parallel join after an exclusive split is flagged (it would deadlock).
- Every cycle passes through a loop limit (a loop marker maximum or a counter condition on the gateway).
- Warnings: a task with several incoming flows and no gateway (implicit merge), a user task outside the You lane, an agent task with no model.
### Storage, import and export
- Graphs are saved as BPMN 2.0 XML in the project (`.harness/graphs/<name>.bpmn`) with the diagram interchange section, so the layout is kept.
- Harness-specific settings live in a `harness:` extension namespace: `harness:type="agent"` on a task, plus agent, model, tools, prompt, ask-first and loop maximum. Other BPMN tools ignore these and still open the file.
- Import accepts any BPMN 2.0 file; unsupported elements (for example compensation or choreography) are shown greyed and block running until replaced.
- An agent task can be exported as an opencode agent file, so the same step can run from the opencode TUI.
## Routines and schedules
A routine is a saved prompt or graph plus a trigger. It shows on a week view with past runs and upcoming ones, and each has a run history.
| Setting | Options |
| When | Schedule in words (stored as cron with a time zone), file change, git event, message arrives, webhook |
| What | A prompt, a graph, or an artifact refresh |
| Where | A workspace and a project (required) |
| Runs on | Cloud runner (no local files), a linked computer, or an org runner |
| Approval | Ask before acting (default), or pre-approved per block |
| If the computer is asleep | Run when it wakes, wake it (if allowed), or skip |
| When done | Notify by push, email or chat |

Routines can be typed in plain words; the harness shows how it understood them (schedule, cron, output, project) before saving.
## Computers
The harness can act on computers the user links through the desktop app. Each computer is configured once, and each workspace separately decides whether it may use it.
- **Per computer**: what the agent may do — read and edit files, run commands, use the built-in browser, computer use (screen control in allowed apps), and wake for routines.
- **Per workspace**: which computers it may use and which folders on each, read-only or read-write. An org can provide managed runners and forbid personal devices.
- **Always ask first**: deleting files, sending messages, purchases, installing software, system settings, opening new apps.
- **Live view**: while computer use runs, the user sees the screen, the next action in words, and can stop or take over at any moment.
- **Pairing**: a short code entered in the desktop app on the other computer; a new computer gets no workspace until the user allows one.
## UI map
Every screen is on the [Workspace Harness design canvas](https://claude.ai/artifact/HNqqnUV8DiJy6LWQ4r57ms); the visual language is the [Claude Warm Editorial design system](https://claude.ai/artifact/W8px5omr6VTs7L5WBvyrm9), with the opencode dark palette as an alternate theme.
| Screen | What it does | Backed by | Status |
| Prototype (one window, shared state) | The whole flow from onboarding to every screen below; press Play on the canvas | All | Designed, every control works |
| Onboarding | Start opencode or connect to a running server, import earlier opencode sessions (folders become projects), workspace, model (or reuse opencode's providers), web search, memory backend, first project with AGENTS.md, theme; or skip to demo data. Work workspaces are added later from + in the rail | Supervisor, opencode | Designed, interactive |
| Home | New chat (always in a project), mode chips, needs-you, continue, coming up | All | Designed, interactive |
| Main window | Workspace rail, nested project tree, streaming chat, artifact side panel, theme toggle | opencode sessions, Open Design MCP | Designed, interactive |
| Command palette (⌘K) | Jump to workspaces, projects, chats, artifacts and actions | Supervisor | Designed, interactive |
| New workspace | Personal, Organization (SSO) or Team; isolation checklist | Supervisor, SSO | Designed, interactive |
| Project | Chats, sub-projects, inherited instructions, memory, files, loops, settings | AGENTS.md, memory service | Designed, interactive |
| Settings | All 15 sections (including Agents (ACP), Web search and Engine) for either workspace, org-locked rows marked | All | Designed, interactive |
| Model providers · Add custom provider | Allowed providers, routing; form, test and live opencode.json preview | opencode config | Designed, interactive |
| Connectors & MCP | MCP servers, memory and history settings | opencode `mcp` | Designed, interactive |
| Memory | Backend choice per workspace, Graphify graph, query, communities, facts, review queue | Facts / Graphify MCP | Designed, interactive |
| Graph editor (BPMN 2.0) | Palette of BPMN events, tasks, gateways and data; lanes; inspector; model check; token dry run; XML export | Orchestrator | Designed, interactive |
| Routines | Routine list, week view, run history, plain-words creation and editing | Scheduler | Designed, interactive |
| Computers | Linked machines, capabilities, workspace access, live view, pairing | Device bridge | Designed, interactive |
| New artifact · Generating · Artifact view | Open Design pipeline, preview, critique, versions, export, handoff | Open Design | Designed, interactive |

## Security and data
- **Secrets**: API keys and tokens live in the OS keychain under a per-workspace prefix. Config files only ever hold `{env:…}` or `{file:…}` references.
- **Local servers**: opencode and Open Design bind to 127.0.0.1 only. Each opencode server has a random password per launch, held by the supervisor; Open Design's preview routes stay loopback-only.
- **Preview sandbox**: artifact previews run in a sandboxed iframe with no network and no access to the harness origin. Published share links serve a static export, never the live daemon.
- **Org policy**: signed by the org, fetched at sign-in and on a timer. It can lock providers, models, MCP servers, external sharing, retention and logging. A policy the harness can't verify keeps the last good one and blocks sending.
- **Data residency**: an org workspace can only talk to the endpoints its policy names; the supervisor sets a per-process proxy allow-list to enforce it.
- **Audit**: the org workspace keeps a local append-only log of sign-ins, provider calls (model, tokens, no content unless policy says so), tool runs, exports and shares, uploaded to the org on its schedule.
- **Permissions**: opencode's permission requests (edits, bash, network) surface as approval prompts; loops can pre-approve per node, never globally.
## Delivery plan, risks and open questions
**Phases** (no dates set yet)
- **Shell + one workspace**: desktop shell, supervisor, one opencode server, projects (with General), chats, model picker, custom providers. Exit: daily use on a personal workspace.
- **Workspaces**: multiple workspaces, switcher, per-workspace secrets and MCP, themes. Exit: personal and org running side by side with no shared state.
- **Artifacts**: Open Design daemon per workspace, New artifact and Generating flows, library, versions, export, Open Design MCP in chats. Exit: a deck and a prototype made end to end.
- **Projects, memory, instructions**: nested projects, inherited AGENTS.md, facts memory, Graphify backend. Exit: a chat in a sub-project starts with the right context and can query the graph.
- **Org**: SSO join, signed policy, provider lock, audit log, residency allow-list. Exit: an org admin can lock providers and see the audit trail.
- **Automation**: BPMN 2.0 graph editor with model check and XML import/export, token-based orchestrator, approvals, routines, computers and computer use. Exit: a routine runs a BPMN graph on a linked computer and stops for approval.
| Risk | Effect | Mitigation |
| opencode or Open Design APIs change | Harness breaks on upgrade | Pin versions per workspace; generate clients from their OpenAPI specs; contract tests in CI |
| Two daemons per workspace use too much memory | Slow machine with many workspaces | Start on demand, stop when idle, cap running workspaces |
| Open Design licence doesn't allow embedding | Can't ship it | Check before phase 3; fall back to running it as a separate install the harness connects to |
| Org policy bypass through a local MCP server | Data leaves allowed endpoints | Proxy allow-list per workspace process; policy can lock the `mcp` block |

**Open questions**
- Open Design's licence and whether its daemon API is meant for third-party clients
- Graphify's licence, and the cost of its docs pass on large folders
- Desktop shell: decided — Tauri 2, with opencode, Open Design and the orchestrator as sidecars (Open Design's Electron shell is not used)
- Where org policy is published and how it is signed (CAIM/Keycloak client, or a static signed file)
- Should Team workspaces sync artifacts, or only share links
- Exact path and format for exporting loop agents as opencode agent files
- How a cloud runner gets project files without a linked computer (git clone, sync, or none)
- Whether the organization needs per-user budgets; in LiteLLM those are enterprise features, so they would need another open-source way
- Which ACP agents to offer by default, and the exact command each one is started with
## Sources
- [opencode — Providers](https://opencode.ai/docs/providers/) · [opencode — Server](https://opencode.ai/docs/server/)
- [nexu-io/open-design](https://github.com/nexu-io/open-design) · [Open Design — open-source alternative to Claude Design](https://open-design.ai/blog/open-source-alternative-to-claude-design/)
- [Anthropic — Building effective agents](https://www.anthropic.com/research/building-effective-agents)
- [AWS Step Functions — workflow states](https://docs.aws.amazon.com/step-functions/latest/dg/workflow-states.md)
- [LangGraph: stateful graphs and durable execution](https://learn.traeai.com/t/ai-engineering/phases/14-agent-engineering/13-langgraph-stateful-graphs.html)
- [Dify — question classifier](https://legacy-docs.dify.ai/guides/workflow/node/question-classifier) · [Dify — parameter extractor](https://legacy-docs.dify.ai/guides/workflow/node/parameter-extractor)
- [Graphify guide (Analytics Vidhya)](https://www.analyticsvidhya.com/blog/2026/04/graphify-guide/) · [Graphify installation guide](https://codeconductor.ai/blog/graphify-installation-guide/)
- [opencode attach (CLI)](https://www.mintlify.com/anomalyco/opencode/cli/attach) · [Web search & fetch in opencode](https://learnopencode.com/en/5-advanced/23-web-search)
- [OMG BPMN 2.0.2 specification](https://www.omg.org/spec/BPMN/2.0.2/)
- [bpmn.io licence](https://scancode-licensedb.aboutcode.org/bpmn-io.html) · [bpmn-engine](https://npmjs.com/package/bpmn-engine) · [bpmn-engine API](https://raw.githubusercontent.com/paed01/bpmn-engine/HEAD//docs/API.md)
- [Agent Client Protocol overview](https://agentclientprotocol.com/protocol/overview) · [opencode ACP](https://opencode.ai/docs/acp)
- [LiteLLM licence](https://github.com/BerriAI/litellm/blob/main/LICENSE) · [LiteLLM enterprise features](https://www.litellm.ai/enterprise)
- [Tauri 2 sidecars](https://v2.tauri.app/develop/sidecar)