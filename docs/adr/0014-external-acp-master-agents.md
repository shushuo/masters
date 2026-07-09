# ADR-0014 — External ACP master agents (coding-harness backend)

**Status:** Accepted · **Decision:** D14

## Context
Masters ([ADR-0010](./0010-master-team-orchestration.md), [ADR-0013](./0013-per-master-model.md)) are
*persona-over-Skill* role descriptors that always run on Masters's **internal** agent loop
(`AgentService` → a `Provider` → built-in/MCP tools, all gated). But users increasingly have powerful
pre-installed **coding agents** on their machine — Claude Code, Codex, OpenCode, Gemini CLI — each
shipping its *own* agent loop, tools, and model. The [Agent Client Protocol (ACP)](https://agentclientprotocol.com/)
(an open standard: JSON-RPC 2.0 over stdio; `initialize` → `session/new` → `session/prompt` with
streaming `session/update` notifications and `fs/*` + `session/request_permission` callbacks) lets any
client drive any of these CLIs uniformly. There was no way to bring such a harness into a Masters
project, and it cannot be modeled as an MCP connector ([ADR-0005](./0005-mcp-sdk.md)): a connector is a
*tool* Masters's loop calls, whereas an ACP harness is a *whole agent loop* Masters drives.

## Decision
1. **A new master backend, not a new entity.** A master gains a `backend` discriminator —
   `internal` (default) or `acp` — plus an `acp` launch config (command/args/env). It stays the
   file-backed `masters/<slug>.md` ([ADR-0007](./0007-layered-memory-prompt.md)) so an ACP master is
   addressable by the router, mentions, teams, group chat, and **portable bundles** ([ADR-0010])
   with zero changes to those paths. Scope is **coding harnesses only**; general assistants are out.
2. **Real ACP over stdio.** Masters plays the ACP **client** via the official `agent-client-protocol`
   crate (server-only dep; the lean core stays protocol-free). It spawns the harness, runs the
   handshake, sends one prompt, and maps the agent's streaming `session/update` text onto Masters's
   existing `AgentEvent` stream — so single-run, team-run, and group-chat (sync + streaming) consume
   an ACP master exactly like an internal one through a single `run_master_stream` seam.
3. **Callbacks routed through the gate.** The harness runs its own tool loop, so its side effects are
   ACP callbacks: `fs/read_text_file`, `fs/write_text_file`, and `session/request_permission` are each
   routed through Masters's Permission & Audit gate ([ADR-0008](./0008-agent-isolation-parallelism.md),
   [06 §1](../06-security-privacy.md)) **before** being honored — resolved against the project's folder
   grants and written to the audit log. The **grant boundary is the security line**: a write outside a
   granted folder is denied even under headless auto-approval.
4. **Environment posture: inherit + configured, not stripped.** Unlike MCP connectors (narrow tools,
   fully `env_clear`-ed — [ADR-0008]), an ACP coding harness is a *user-installed, trusted local agent*
   that needs a real environment (PATH/HOME/`npx`) to run. It therefore **inherits the daemon
   environment plus the master's configured `acp_env`**, and the gate governs its file/permission side
   effects rather than the environment being the boundary. (Tools the harness runs *internally* without
   surfacing a callback are outside the gate by construction — documented, and `new_session.mcp_servers`
   is empty in this slice.)
5. **Auto-detection for one-click registration.** A static known-harness registry probes `PATH` for the
   supported coding CLIs (`GET /acp/harnesses`) so the desktop can prefill a launch config; detection
   never spawns the agent and never auto-creates a master (the user names and registers it).

This **extends** ADR-0010/0013 (a third dispatch backend for a master) and reuses ADR-0008's gating;
it does not reverse them. Masters still owns the human-in-the-loop boundary for every side effect.

## Alternatives considered
- **Model the harness as a `Provider`** — the `Provider` trait is chat/stream/embed where Masters owns
  the tool loop; an ACP agent inverts that (it owns the loop), so it doesn't fit. Rejected.
- **A separate `project_acp_agents` table** (connector-style) — would drop on bundle export and force a
  parallel load/merge path through the router, mentions, teams, and group chat. Rejected for the
  file-backed `backend` field, which rides every existing master path for free.
- **Full `env_clear` stripping (connector parity)** — truest to ADR-0008's letter, but breaks real
  harnesses (they need PATH/`npx`/HOME) and the crate's spawner inherits env anyway. Rejected in favor
  of inherit-plus-configured, with the gate as the side-effect boundary.
- **Generic stdin/stdout subprocess wrapper** — simpler but not interoperable with the real ACP
  ecosystem and gives no structured permission/fs callbacks to gate. Rejected.

## Consequences
- (+) Users run their existing coding agents inside Masters projects, in teams and group chat, with file
  edits and permission requests **gated + audited** like any built-in tool; portable bundles carry ACP
  masters for free; adding a harness is one registry entry.
- (−) A wider trust surface (a full external agent vs. a narrow tool) governed at the grant boundary,
  not the environment; the harness's *internal* un-callback'd actions are outside the gate; the ACP
  crate is young (API churn risk — isolated to `acp/driver.rs`/`acp/client.rs`).
- **Deferred:** remote ACP transports (HTTP/WebSocket) + OAuth `authenticate`; the full callback surface
  (terminals, plans/modes, slash commands); the harness's own MCP servers + gating its internal toolset;
  interactive (non-headless) approval for ACP single-runs; resumable ACP sessions. Lands as roadmap
  **Phase 4i** ([08](../08-roadmap.md)).

## Amendment (hardening pass)

Three refinements landed after 4i, tightening the gate boundary this ADR defines:

1. **`session/request_permission` is now genuinely gated.** The original slice classified the whole
   request as one opaque Write and, under headless auto-approval, allowed it by selecting the *first*
   offered option (which may be an "always allow"). Now a **located** tool call (the request carries
   `locations`) is checked **per path** against the folder grants — `files.read` for read kinds,
   `files.create` for edit/delete/move — so an out-of-grant operation is **denied + audited** even
   headless; un-located calls (execute/fetch/other) authorize as `acp.<kind>` (Write-classified,
   audited). The reply picks the option matching the verdict: allow → *allow-once* (never a silent
   "always" escalation), deny → *reject-once*.
2. **Scope of the guarantee, stated precisely:** the grant boundary covers operations the harness
   routes through ACP callbacks (`fs/read_text_file`, `fs/write_text_file`,
   `session/request_permission`). A harness that executes its own tools *without* asking (e.g. a
   permissive permission mode) acts outside the gate — constraining that surface belongs to the
   harness's own configuration (e.g. Claude Code's permission mode / allowed-tools flags in the
   master's `acp_args`), and OS-level sandboxing remains future work.
3. **Operational bounds + visibility:** an ACP run is bounded by `GETMASTERS_ACP_TIMEOUT_SECS`
   (default 600s; a wedged harness becomes a turn error), the harness's `session/update` tool
   activity maps onto `ToolCallStarted`/`ToolResult` events (Phase 4g visibility now covers ACP
   masters) and the durable session event log, and group answer turns hand the harness the
   **speaker-labelled transcript** (closing the "full transcript replay" deferral above).
