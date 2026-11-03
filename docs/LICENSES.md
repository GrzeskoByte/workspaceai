# Open-source components and licence obligations (R-45)

Harness only uses open-source components. Each one is used unmodified. Versions are the ones installed in this repository on 2026-10-29. Licences were read from each package's own `package.json` / `LICENSE`, or from the project's repository metadata.

| Component | Version | Role in Harness | Licence | Obligation and how it is met |
|---|---|---|---|---|
| opencode | 2.2.1 (system install) | Agent runtime: sessions, providers, MCP, agents | MIT | Keep the copyright notice. Harness runs the user's `opencode` binary unmodified and ships no copy of it. |
| Open Design | 0.25.1 (`~/.harness/vendor/open-design`) | Artifacts manager: lint, versioned files, exports, MCP | Apache-2.0 | Keep the LICENSE and NOTICE, and state any changes. It is installed from the upstream repository, unmodified, and run as a separate daemon. |
| bpmn-js | 18.31.1 | BPMN modeller in the graph editor | bpmn.io licence (MIT-style) | The bpmn.io watermark must stay visible and not overlapped. The inspector sits beside the canvas, not over it. The browser walkthrough confirmed `.bjs-powered-by` is displayed and visible. |
| bpmn-moddle | 10.3.1 | BPMN XML read/write in the model check | MIT | Keep the notice. |
| bpmnlint | 11.15.0 | Model-check rules (plus the `harness/*` rule plugin) | MIT | Keep the notice. |
| bpmn-engine | 26.0.5 | Executes BPMN graphs | MIT | Keep the notice. |
| Agent Client Protocol | spec | Talks to other coding agents over stdio | Apache-2.0 (specification) | Each ACP agent keeps its own licence. Harness starts the user's installed agent binaries and bundles none. |
| LiteLLM proxy | not bundled | Organization gateway; local provider preset | MIT, except its `enterprise/` folder (the repository is marked NOASSERTION because of that folder) | Harness only points an OpenAI-compatible provider at a proxy URL. Nothing from `enterprise/` is used. SSO stays with the organization's identity provider, and auditing stays in the Harness audit log. |
| Playwright MCP (`@playwright/mcp`) | 0.0.83 | Browser steps in graphs; browsing connector in chats | Apache-2.0 | Keep the LICENSE and NOTICE. |
| Tauri 2 (`tauri`, `@tauri-apps/*`) | 2.12.4 | Desktop shell and sidecars | Apache-2.0 OR MIT | Keep the notice. |
| chrono-node | 2.10.3 | Plain-words dates and times for routines | MIT | Keep the notice. |
| cron-parser | 5.10.2 | Next-run calculation for cron schedules | MIT | Keep the notice. |
| cmdk | 1.1.1 | ⌘K palette | MIT | Keep the notice. |
| Model Context Protocol SDK | 1.32.2 | Memory and artifact MCP servers; Playwright MCP client | MIT | Keep the notice. |
| Graphify | external (`graphifyy` Python package) | Memory backend option | Apache-2.0 (Graphify-Labs/graphify) | It is installed by the user and run as its own process. Keep its LICENSE and NOTICE if it is ever redistributed. |
| SearXNG | external service | Self-hosted web search option | AGPL-3.0 | It runs as a separate, unmodified service. Harness only calls its HTTP API through an MCP server, so no AGPL code is linked or distributed. |
| React, react-dom | 19.3.1 | UI | MIT | Keep the notice. |
| Vite | 8.3.5 | UI build | MIT | Build tool only; not shipped. |
| @napi-rs/keyring | 2.1.2 | OS keychain access | MIT | Keep the notice. |
| zod | 4.6.7 | MCP tool input schemas | MIT | Keep the notice. |

Both of the design document's licence questions are now answered. Open Design is Apache-2.0, and Graphify is Apache-2.0.
