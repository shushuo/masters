# 01 — Product Requirements (PRD)

## 1. Problem statement

People who study and do personal knowledge work accumulate a sprawl of local files (PDFs, slides, notes,
drafts, downloads). Existing chat assistants are not *grounded* in those materials, don't *act* on the
filesystem, and forget everything between sessions. Cloud agents that do act raise privacy concerns and tie the
user to one provider. Masters closes this gap: a **local, grounded, acting** agent focused on study and personal
work, with the user in control.

## 2. Personas & top user journeys

### A. The learner
| Journey | Outcome |
|---|---|
| Ingest a course folder of PDFs | Materials indexed and searchable |
| Ask questions about the material | Grounded answers **with citations** to the source files/pages |
| "Make flashcards from chapter 3" | A deck generated and saved to the project |
| "Quiz me" | Spaced-repetition review session driven by SM-2 scheduling |
| "I have an exam in 10 days" | A day-by-day study plan that prioritizes weak areas |

### B. The knowledge worker
| Journey | Outcome |
|---|---|
| "Sort and dedupe my Downloads" | Files renamed/grouped/deduped (after approval) |
| "Merge these 3 reports into a brief" | A synthesized draft document created in the project |
| "Extract the key terms from this contract" | Structured summary returned and optionally saved |
| "Every Monday 9am, summarize my /Inbox folder" | A scheduled recurring routine producing a weekly digest |

## 3. Functional requirements

Priority: **P0** = MVP, **P1** = v1, **P2** = later. (Maps to [08 — Roadmap](./08-roadmap.md).)

### 3.1 Agent & conversation
- **FR-1 (P0)** Multi-turn agent loop: user goal → plan → tool calls → observe → respond, iterating until done
  or stopped.
- **FR-2 (P0)** Streaming responses with visible tool-call steps ("thinking/acting" transcript).
- **FR-3 (P0)** Stop/interrupt a running task at any point.
- **FR-4 (P1)** Per-session and per-project system instructions.
- **FR-30 (P2)** Spawn isolated **parallel subagents** for embarrassingly-parallel sub-tasks (e.g. ingesting
  many documents, scanning several folders); subagent tool calls pass through the same approval/audit path and
  results merge back into the parent run ([ADR-0008](./adr/0008-agent-isolation-parallelism.md)).
- **FR-38 (P3)** **Master Team orchestration**: a brief is routed to selected masters that run as **gated
  parallel and/or sequential subagents** (division of labor, staged workflows), with results merged back into
  the run; every master subagent's tool calls pass through the same approval/audit path
  ([ADR-0010](./adr/0010-master-team-orchestration.md), [ADR-0008](./adr/0008-agent-isolation-parallelism.md)).

### 3.2 Files & workspace
- **FR-5 (P0)** Grant the agent access to specific folders; all file tools are scoped to grants.
- **FR-6 (P0)** File tools: read, list, search, create, edit, move, rename, delete (each write gated by
  approval).
- **FR-7 (P1)** **Projects**: a persistent workspace and **context container** = one or more folders +
  instructions + memory + linked sources + skills + masters/master-team + MCP connectors, **auto-injected into
  each session** with project-scoped items ranked above global, resumable across app restarts
  ([ADR-0011](./adr/0011-project-context-container.md)).
- **FR-42 (P3)** **Project templates**: create a Project from a portable template that pre-seeds its bundle
  (instructions + recommended masters/team + skill set + connectors) ([ADR-0011](./adr/0011-project-context-container.md)).
- **FR-8 (P1)** Diff preview before applying any file edit; one-click revert of the last agent action.

### 3.3 Knowledge / RAG
- **FR-9 (P1)** Ingest PDFs, DOCX, Markdown, plain text, and slides into a per-project index.
- **FR-10 (P1)** Grounded Q&A: retrieval-augmented answers that **cite** source file + location.
- **FR-11 (P1)** Incremental re-index on file changes; show index freshness/status.
- **FR-12 (P2)** Cross-project / global knowledge search.

### 3.4 Study tools
- **FR-13 (P2)** Generate flashcards (Q/A or cloze) from selected materials.
- **FR-14 (P2)** Spaced-repetition review sessions (SM-2 scheduling).
- **FR-15 (P2)** Generate an adaptive study plan toward a deadline.

### 3.5 Automation
- **FR-16 (P2)** **Recipes**: declarative YAML workflows (parameterized, repeatable multi-step tasks).
- **FR-17 (P2)** **Scheduler**: run a recipe/prompt once at a time or on a recurring cron schedule.
- **FR-27 (P2)** **Outbound delivery** of routine output: local OS notifications (on-device) and an opt-in
  email digest. Delivery is one-way; email is a `send` side-effect — approval-gated and audited
  ([ADR-0009](./adr/0009-outbound-delivery-surfaces.md)).

### 3.6 Extensions & providers
- **FR-18 (P0)** Pluggable LLM providers behind one interface; Claude is the default.
- **FR-19 (P1)** Enable/disable built-in MCP servers from the UI.
- **FR-20 (P2)** Add external MCP servers (command + args + env) via settings.

### 3.7 Permissions, security, transparency
- **FR-21 (P0)** Per-action approval prompts for write/delete/network/send actions, with "allow once / allow
  for this folder / always allow this tool" options.
- **FR-22 (P0)** Audit log of every tool call (inputs, result, timestamp), viewable and exportable.
- **FR-23 (P1)** Secrets (API keys) stored in the OS keychain, never in plaintext config.
- **FR-28 (P0)** **Blank Slate / least-privilege mode**: a session or profile can boot with a minimal default
  tool set and no standing permissions, granting capability as needed ([ADR-0008](./adr/0008-agent-isolation-parallelism.md)).
- **FR-29 (P2)** **External MCP isolation**: external servers run with credential stripping (inherit only env
  Masters injects; secrets resolved from keychain at spawn) and OS-level sandboxing where available; their
  declared tools and side-effect classes are shown before enabling.

### 3.8 Settings & lifecycle
- **FR-24 (P0)** Configure provider, model, and API key.
- **FR-25 (P0)** Desktop app auto-starts and supervises the local `getmastersd` daemon; clean shutdown.
- **FR-26 (P1)** Optional CLI that drives the same core for power users/scripting.

### 3.9 Skills (procedural memory)
- **FR-31 (P1)** **Capture skills**: after a successful complex task, the agent can record a reusable **Skill**
  (procedure + pitfalls + verification steps) as an editable Markdown file, scoped to a project or global
  ([ADR-0006](./adr/0006-skills-procedural-memory.md)).
- **FR-32 (P1)** **Improve skills**: refine an existing skill when a better approach is found.
- **FR-33 (P1)** **Recall skills**: surface relevant skills into context at task start.
- **FR-34 (P2)** **Manage & share skills**: browse/enable/disable, edit, and import/export portable skill files;
  optionally promote a mature skill into a deterministic Recipe (FR-16).

### 3.10 Memory & personalization
- **FR-35 (P1)** **Layered, file-backed memory**: durable memory is typed (facts vs preferences vs procedural)
  and stored as user-editable files (`MEMORY.md`, `USER.md`) that are the source of truth, indexed in the DB
  for recall ([ADR-0007](./adr/0007-layered-memory-prompt.md)).
- **FR-36 (P1)** **Memory curation ("nudge")**: the agent proposes what is worth persisting or forgetting,
  preventing memory bloat; consequential writes still surface for approval.
- **FR-37 (P1)** **Modular prompt assembly**: each turn's system prompt is composed from editable sources
  (defaults + project `INSTRUCTIONS.md` + memory layers + recalled skills + RAG context), so customization is
  editing files, not code (assembly baseline lands in P0).

### 3.11 Masters & Master Teams
- **FR-39 (P3)** **Define/edit Masters**: a master is a **persona over a Skill** — a thin role descriptor
  (persona, allowed skills + tools, default model, output contract) stored as editable Markdown (`masters/*.md`),
  scoped to a project or global ([ADR-0010](./adr/0010-master-team-orchestration.md)).
- **FR-40 (P3)** **Master router**: auto-select the relevant master(s)/team for a brief, with **manual
  override** always available ([ADR-0010](./adr/0010-master-team-orchestration.md)).
- **FR-41 (P3)** **Manage & share master/team bundles**: browse/enable/disable, edit, and import/export
  portable master and Master-Team bundles (origin shown; an imported master is *instructions, not trusted
  code*); promote a proven team workflow into a deterministic Recipe (FR-16)
  ([ADR-0010](./adr/0010-master-team-orchestration.md)).
- **FR-43 (P3)** **Master addressing (@-mention)**: in a multi-master session, the user can **@-mention** one
  master, several masters, or **`@all`/`@team`** to answer; an **unaddressed** message is handled by the team's
  **coordinator master** (master router FR-40 available as routing assist). Mentions resolve against the session
  participant list ([ADR-0012](./adr/0012-multi-master-conversation.md)).
- **FR-44 (P3)** **Shared group context**: each master receives the **full session transcript** as conversation
  context, **attributed by author** (user and other masters), assembled per
  [ADR-0007](./adr/0007-layered-memory-prompt.md) with project-scoped items ranked first; long transcripts are
  managed by summarization/RAG recall ([ADR-0012](./adr/0012-multi-master-conversation.md)).
- **FR-45 (P3)** **Master workflow orchestration**: declarative, recipe-style chaining/sequencing of masters
  (linear + simple branching) that post attributed messages into the session, each step's output feeding the
  next; runnable on demand or scheduled; promotable to a Recipe. Masters do not auto-reply to each other except
  via an explicit workflow step or a bounded `max_rounds` cap; **Stop (FR-3) halts the group**
  ([ADR-0012](./adr/0012-multi-master-conversation.md)).
- **FR-46 (P3)** **Per-master model**: each master declares its model in its persona (a provider-qualified
  `default_model`); the orchestrator runs that master on its configured model/provider — **any configured
  provider** (Claude tiers, OpenAI, local Ollama) — so a single session can span multiple models. Persona-fixed
  (no runtime override); falls back to the default provider with a visible notice if unavailable. A local-model
  master keeps its context on-device ([ADR-0013](./adr/0013-per-master-model.md)).

## 4. Non-functional requirements

| ID | Requirement |
|---|---|
| NFR-1 | **Local-first**: no data leaves the device except the model context sent to the chosen provider. |
| NFR-2 | **Cross-platform**: Windows, macOS, Linux. |
| NFR-3 | **Lightweight**: target installed size and idle RAM well below an Electron-heavy baseline (drives the Tauri choice). |
| NFR-4 | **Responsive**: first token < ~2s on a typical broadband + provider; UI never blocks on agent work. |
| NFR-5 | **Resilient**: provider retries/backoff; daemon crash is detected and recovered by the desktop supervisor. |
| NFR-6 | **Private by construction**: embeddings/index/sessions stored locally; secrets in OS keychain. |
| NFR-7 | **Auditable**: every side-effecting action is logged and reversible where feasible. |
| NFR-8 | **Transparent memory**: durable memory, instructions, and skills are stored as human-readable files the user can inspect, edit, diff, and remove. |
| NFR-9 | **Least-privilege by default**: capabilities (tools, folder scopes, standing permissions) are granted incrementally, not assumed. Master subagents inherit the same gating/audit — orchestration never bypasses approval ([ADR-0010](./adr/0010-master-team-orchestration.md)). |

## 5. Success metrics (post-MVP)

- **Task completion rate** — % of started agent tasks that reach an accepted deliverable without abandonment.
- **Grounding quality** — % of RAG answers whose citations the user rates as relevant.
- **Trust** — ratio of approvals granted vs. denied; low "surprise" (unexpected actions) reports.
- **Retention** — weekly active use; number of persistent Projects per user.
- **Automation adoption** — recipes created and scheduled routines still active after 30 days.

## 6. Non-goals (v1)

Team collaboration, hosted backend, coding IDE features, mobile apps, model fine-tuning. No **multi-user master
collaboration, task sharing/handoff, or member approval** (WorkBuddy-style) — Masters and Master Teams are
single-user ([ADR-0010](./adr/0010-master-team-orchestration.md), [ADR-0009](./adr/0009-outbound-delivery-surfaces.md)).
(See [00 — Overview](./00-overview.md#scope-boundaries-non-goals-for-v1).)
