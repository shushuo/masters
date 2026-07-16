# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Current state: Phase 0 + Phase 1 + Phase 2 (2a/2b/2c) + Phase 3a/3b (Study) + 3c (Recipes) + 3d (Scheduler) + 3e (Delivery) + 4a (Masters) + 4b (Teams + router) + 4c (Group chat) + 4d (External MCP) + 4e (Group streaming) + 4f (Multi-round) + 4g (Group tool visibility) + 4h (Portable bundles) + 4i (External ACP master agents) + Desktop UI (design system, full management UI, ACP selector, chat history, audit viewer, theme toggle, group max-rounds) + Masters sidebar (standalone/global masters + system templates + quick chat) + Hardening pass (loop robustness, i18n, event log, ACP gate) + Investing vertical slice 1 (assets+market servers, expert-team pack, ask→track loop, Watch UI) + slice 2 (proactive touch: weekly digest + mover sentinel recipes w/ silent-pass, briefings feed) implemented

**Phase 0 (Foundations)** and **Phase 1a** are in place (see `docs/08-roadmap.md` and
`DEVELOPMENT.md`): a Rust Cargo workspace under `crates/` (`getmasters-proto`, `getmasters-core`,
`getmasters-server`→`getmastersd`, `getmasters-mcp`, `getmasters-cli`), the `getmastersd` axum daemon (loopback
HTTP + WebSocket, per-launch bearer token, OpenAPI), and a Tauri 2 desktop shell under
`ui/desktop/`. Phase 1a added:

- **Tool-calling agent loop** — content-block messages (`Text`/`ToolUse`/`ToolResult`), a
  bounded multi-turn tool loop.
- **Providers** — `Provider` trait with Anthropic, an **OpenAI-compatible** backend
  (configurable `base_url`: OpenAI/Groq/Together/OpenRouter/Ollama) + a provider-qualified
  model resolver (ADR-0013 seam). A deterministic offline `MockProvider` (tool-call trigger)
  exists **only behind the `testing` feature** as the headless test fake — it is **not** a
  production fallback. The daemon **refuses to start** when no usable provider is configured
  (`Config::effective_provider()` → `None`); there is no silent offline mode.
- **Files MCP server** — rmcp `FilesServer` hosted in-process via `tokio::io::duplex`, with
  `getmasters-core::extensions` as the rmcp client host (ADR-0005).
- **Permission & Audit** (`getmasters-core::permission`) — folder grants, the docs/06 default
  policy matrix, an async approver (auto for CLI/tests, WS-channel for the daemon), and
  `audit_log`. The gate runs in Core before every tool dispatch — never bypassed.
- **Modular prompt assembly** baseline (`getmasters-core::prompt`, ADR-0007 seam).

Phase 1b added: **OS-keychain secrets** (`getmasters-core::secrets`, with a memory fallback) +
**settings** (migration 0003, `Config::resolve`, `/settings` endpoints); **revert/undo** of
file ops (migration 0004, `getmasters-core::revision`, `POST /sessions/{id}/revert`); **Blank
Slate** least-privilege mode (`AgentService::blank_slate`); and the desktop **Settings** +
**approval-dialog** UI (front-end bundles; the live Tauri window defers to a real desktop).

Phase 2a added: **Knowledge/RAG** (`getmasters-core::knowledge`, migration 0005) — ingest `.md/.txt`
(pluggable extractor seam) → chunk → embed → index; a `VectorIndex` trait with a brute-force
cosine default and a feature-gated **sqlite-vec `vec0`** backend (ADR-0004); hybrid vector+FTS
retrieval; an rmcp `KnowledgeServer` (ingest/search/status) hosted alongside Files via a
multi-server `ExtensionManager`; **OpenAI embeddings** + a project-level `Embedder`; and
**Projects as context containers** (ADR-0011) — project CRUD + a per-session agent cache that
auto-injects a project's grants/knowledge/instructions, project-scoped ranked above global.

Phase 2b added: **file-backed Memory** (`getmasters-core::memory`, migration 0006) — durable memory in
Markdown the user can edit (`MEMORY.md` facts + `USER.md` profile), the DB as an FTS index; an rmcp
`MemoryServer` (`remember`/`recall`/`forget`) that re-indexes its file on every write (ADR-0007).
**Skills** (`getmasters-core::skills`) — agent-authored procedures as `skills/<slug>.md` + frontmatter,
indexed in the `skills` table; an rmcp `SkillsServer` (`create_skill`/`recall_skill`/`list_skills`,
ADR-0006). Both are hosted per-project alongside Files + Knowledge over a **per-project data dir**
(`{db_parent}/projects/{id}/`, `ExtensionManager::with_project(..., project_dir)`). **Prompt
auto-injection** — `PromptAssembler::assemble` now takes a `PromptContext` that carries the recalled
memory block + skill summaries + a curation nudge (FR-37). Read-only list endpoints
(`/projects/{id}/memories|skills`) + desktop `Memory.tsx`/`Skills.tsx` (bundle-only). Permission
classifier fixed: `recall_skill`/`list_skills`/`recall` are Read; `forget` is a Write curation edit.

Phase 2c added (v1 close-out): **PDF/DOCX ingest** — `PdfExtractor` (pure-Rust `pdf-extract`,
per-page citations) + `DocxExtractor` (`zip` + `quick-xml`), **feature-gated** (`pdf`/`docx`) so the
library default stays lean/headless while the daemon + CLI enable them. **FR-19 per-project
extensions toggle** (migration 0007 `project_extensions`, absent = enabled) — `with_project(...,
enabled)` hosts only chosen built-ins (not-hosting is the enforcement); `GET`/`PUT
/projects/{id}/extensions`. **Knowledge read endpoint** (`GET /projects/{id}/knowledge` =
status + indexed `DocumentDto` paths; ingest stays a gated chat action). **Desktop management UIs** —
a `Projects` picker + `ProjectDetail` (Instructions/Knowledge/Memory/Skills/Study/Extensions tabs); `App.tsx`
nav is `chat | projects | settings`.

Phase 3a added (v2 start): **Study** (`getmasters-core::study`, migration 0008, FR-13/14) — flashcards +
SM-2 spaced repetition. Unlike file-backed Memory/Skills, flashcard **review state** (ease factor,
interval, repetitions, due date, lapses) is **DB-owned** structured data (`decks`/`cards` tables), not
hand-edited Markdown; the agent (LLM) authors cards and persists them through a gated tool. The pure,
clock-free **SM-2 algorithm** lives in `study::sm2` (interval uses the pre-update ease factor, floor
1.3). An rmcp `StudyServer` (`save_flashcards`/`grade_card` = Write, `start_review`/`list_decks` =
Read) is hosted per-project alongside Files/Knowledge/Memory/Skills (added to `IMPLEMENTED_SERVERS` +
the FR-19 toggle). Read endpoint `GET /projects/{id}/decks` (`DeckDto` = name + card/due counts) +
desktop `Study.tsx` tab (bundle-only). No new deps / no feature gate — stays in lean core.

Phase 3b added (FR-15): **adaptive study plans** — a per-project, DB-owned plan (migration 0009,
`study_plans`, one active plan via `UNIQUE(project_id)`, replaced on regenerate). Two more `StudyServer`
tools: `review_stats` (Read) surfaces per-deck weak-area signal (cards/due/lapses/avg ease via
`Store::deck_stats`) so the agent prioritizes weak decks, and `create_study_plan` (Write) persists an
agent-authored day-by-day plan toward a deadline (`deadline_days` from now). Read endpoint `GET
/projects/{id}/study-plan` (`StudyPlanDto` = title + deadline + body, null when none) + the plan shown
under the desktop `Study.tsx` tab. Plan is surfaced via endpoint/UI, not auto-injected into the prompt.

Phase 3c added (FR-16): **Recipes** — human-authored, parameterized automations (docs/04 §4). Like
Skills the YAML file is the source of truth (`recipes/<name>.yaml` under the per-project data dir),
indexed by a `recipes` table (migration 0010) for listing. To keep the **lean core YAML-free**, the
serde model + parsing (`serde_yaml_ng`) + run logic live in **`getmasters-server`** (`recipe.rs`); core
only gains the string-typed `recipes` index methods. A run substitutes `{{param}}` (supplied →
declared default → built-in `{{date}}`) into the `prompt` and feeds it to the existing agent loop:
`POST /projects/{id}/recipes/{name}/run` creates a `recipe:<name>` session and calls
`AgentService::complete_turn` on the project agent with **approvals cleared**
(`AgentService::without_approval`) — headless auto-approval, still grant-bounded + audited. CRUD via
`POST`/`GET /projects/{id}/recipes(/{name})` (`RecipeDto`/`RecipeSummaryDto`) + a desktop `Recipes.tsx`
tab with run-now (bundle-only). Recipes are user-level automations (not an MCP tool); `extensions` is
advisory in 3c. Scheduler (FR-17) + outbound delivery (FR-27) build on this next.

Phase 3d added (FR-17): **Scheduler** — fire a project recipe once at a time or on a recurring cron
expression while the daemon runs (docs/02 §5; an always-on service is Phase 4). DB-owned `schedules` +
`scheduled_runs` (migration 0011, ints/strings only) keep the **lean core cron-free**; the cron math +
the tick loop live in `getmasters-server` (`scheduler.rs`, the **`cron`** + `chrono` deps). The run path is
the **3c recipe run, extracted to `recipe::run_loaded`** and shared by the HTTP "run now" handler and
the scheduler — identically gated/audited. `next_after` normalizes a standard 5-field cron to the
crate's seconds-first form; `run_due(state, now)` fires every due schedule (`enabled AND next_run_at <=
now`), records the outcome, and advances (cron → next occurrence; once → disabled). `scheduler::spawn`
runs a 30s `tokio::interval` loop, wired in `main.rs` (clone `AppState` before `build_app`). CRUD +
history via `POST`/`GET`/`PUT`/`DELETE /projects/{id}/schedules(/{sid})` + `GET .../runs`
(`ScheduleDto`/`ScheduledRunDto`) + a desktop `Routines.tsx` tab (bundle-only). Overdue schedules fire
once on the next tick (at-least-once catch-up).

Phase 3e added (FR-27; ADR-0009): **Outbound delivery** — pushes a routine's output off the agent loop
after a scheduled run, over the two channels ADR-0009 puts in scope: an on-device **OS notification**
and an **opt-in email digest** (user-configured SMTP, **off by default**). Delivery is a *server-level
component the Scheduler invokes after the run* (docs/02 §1 `Sched → Deliver → Perm`), **not** an MCP
tool — recipes run headless (`without_approval`), so an in-loop tool would auto-approve and violate
docs/06's "`send` never silently" rule; instead the opt-in **is** the config + the per-schedule
`deliver_email` flag (the standing per-target approval), and every send is written to the **audit log**.
The lean core stays SMTP-free: **migration 0012** only adds two flags (`deliver_notify`/`deliver_email`)
to `schedules`; the SMTP wire + email config live in `getmasters-server` (`delivery.rs`, the **`lettre`** dep
with rustls). An `EmailTransport` trait (real `LettreTransport` / test `CapturingTransport`, injected via
`AppState`) keeps it headless-testable. `delivery::deliver` is hooked into `scheduler::run_due` after a
successful run: notify → audited `auto` (on-device; the toast is the desktop's Tauri job); email →
resolve `EmailConfig` (settings table + `smtp_password` secret), send a redaction-aware body, audited
`approved` (or `denied`+skipped when unconfigured). Email config via `GET`/`PUT /settings/email`
(`EmailSettingsDto`/`EmailSettingsUpdate`; password through the existing `/settings/secret`); per-schedule
toggles on `ScheduleDto`/`CreateScheduleRequest`/`SetScheduleRequest` + a desktop Settings email section &
`Routines.tsx` notify/email checkboxes (bundle-only).

Phase 4a added (FR-39/46; ADR-0010/0013): **Masters** — *persona-over-Skill* role descriptors. A master
is editable Markdown `masters/<slug>.md` (frontmatter + body) indexed in the `masters` table (migration
0013) **exactly like Skills** — the **lean core** `getmasters-core::masters` module reuses the same hand-rolled
frontmatter parser + `skills::slugify` (no new dep). A master carries a **persona**, a provider-qualified
**`default_model`** (ADR-0013), and a least-privilege **`allowed_tools`** subset (NFR-9), plus
summary/allowed_skills/output_contract/origin; the file is the source of truth, the DB row is listing
metadata. Two small `AgentService` seams realize a single-master run: `with_persona` (injected into the
prompt via a new `PromptContext.persona`, emitted right after BASE) and `with_model_provider` (per-master
dispatch through the existing `provider::resolve_provider`, ADR-0013). `getmasters-server::master::run` mirrors
`recipe::run_loaded`: load the master, open an `master:<slug>` session, run the project agent on the
master's persona + model + (when set) `allowed_tools` via `with_enabled_tools`, `without_approval`
(headless, grant-bounded + audited), returning the attributed message. HTTP CRUD + run via
`POST`/`GET`/`DELETE /projects/{id}/masters(/{slug})` + `POST .../{slug}/run`
(`MasterDto`/`MasterSummaryDto`/`RunMasterRequest`/`MasterRunResult`) + a desktop `Masters.tsx` tab
(define + run, bundle-only).

Phase 4b added (FR-38/40; ADR-0010; docs/09 §3): **Master Teams + the `route_brief` router**. A team is a
named, project-scoped group of masters + a **coordinator** (the master that answers unaddressed briefs),
**DB-owned** structured config like schedules/study-plans — migration 0014 `master_teams` with members as a
JSON-array column (no file I/O, no new dep; portable bundles deferred). The router is a **pure, deterministic
lexical ranker in the lean core** (`masters::router::rank`/`select`) — score the brief's terms against each
member's name/summary (weighted) + persona/skills, top match selected, else the coordinator (the *(no
mention) → coordinator* rule). `getmasters-server::team` does the orchestration the router must not: `route`
(read-only — rank + select, executes nothing) and `run` (route or honor a manual `master` override → dispatch
the one chosen master through the gated 4a `master::run`; **single dispatch**). CRUD + route + run via
`POST`/`GET`/`DELETE /projects/{id}/teams(/{slug})` + `POST .../{slug}/route|run`
(`TeamDto`/`TeamSummaryDto`/`CreateTeamRequest`/`RouteResultDto`/`RankedMasterDto`/`TeamRunResult`) + a desktop
`Teams.tsx` tab (build a team, route/run a brief, bundle-only).

Phase 4c added (FR-43; ADR-0012; docs/09 §4): **Multi-master group chat** — a session bound to a team behaves
as a single-user group chat under *"shared read context, isolated gated execution"*. **Migration 0015** adds
message **authorship** (`author` = `user` or a master slug; `addressed_to` = JSON of addressed slugs) +
`sessions.team_slug` (all nullable; ordinary chat unaffected) — `Store::insert_message` now delegates to
`insert_message_attributed`. Pure **@-mention resolution** lives in the lean core (`masters::mentions::resolve`:
`@a @b` → those masters, `@all`/`@team` → everyone, **no mention → the team coordinator**). Two surgical
turn-loop seams (threaded through `TurnRun` like 4a's `persona`): `AgentService::with_author` (attributes the
run's replies + renders `load_transcript` **speaker-labelled** from that master's perspective) and
`run_answer_turn`/`complete_answer_turn` (run over the *existing* transcript with **no** user-message append).
`getmasters-server::group` orchestrates: `start` binds a session to a team; `post` resolves mentions, persists the
user msg, snapshots the clean transcript, then dispatches the addressed masters **in parallel** — each in its
own **isolated scratch session** seeded with the snapshot (tool scratch never pollutes the group transcript),
on its own persona + model + tools (`without_approval`, gated + audited), posting only its final attributed
reply back. Endpoints `POST /projects/{id}/teams/{slug}/session` + `POST /sessions/{id}/group`
(`GroupPostRequest`/`GroupPostResult`); `MessageDto`/`SessionDto` gain author/addressed_to/team_slug; a desktop
`GroupChat.tsx` panel off the Teams tab (bundle-only). **Deferred** (ADR-0012): multiple rounds + bounded
master↔master turn-taking (one round now), live WS streaming of group turns (synchronous HTTP now), declarative
master workflows / sequential staging, transcript summarization+RAG, portable bundles, embedding-based routing.

Phase 4d added (FR-20; ADR-0005, docs/04 §3): **External MCP servers** — a project can add a third-party
**stdio** MCP server whose tools join the agent, **gated + audited exactly like built-ins**. The hosting seam
was already transport-agnostic: `ExtensionManager::host_external` builds a `tokio::process::Command`
(`env_clear()` + only the connector's configured env — credential stripping, ADR-0008), wraps it in rmcp's
`TokioChildProcess` (the **`transport-child-process`** feature — no new crate), `().serve(child)`, and registers
its tools under the connector `name` prefix via the same path as in-process built-ins. A connector that fails to
spawn is **logged + skipped** (built-ins survive). Safety is automatic: `permission::policy::classify` already
defaults unknown tools to **Write** (gated). **Migration 0016** `project_connectors` (DB-owned: command + args/env
JSON + enabled); `with_project` gained a `connectors` param; `AppState::project_agent` loads the enabled
connectors + `invalidate_project` rebuilds on change. CRUD via `POST`/`GET /projects/{id}/connectors` +
`PUT`/`DELETE .../{name}` (`ConnectorDto`/`CreateConnectorRequest`/`SetConnectorEnabledRequest`) + a desktop
Connectors section in the Extensions tab (bundle-only). A tiny **`mcp-echo`** stdio bin (`getmasters-server`) is the
headless test fixture — the integration test spawns it as a real connector and asserts tool hosting + env
isolation. **Deferred**: remote SSE/HTTP transports + OAuth, per-connector tool allow-lists, the `web` built-in.

Phase 4e added (ADR-0012): **Live WS streaming of group turns** — the 4c group chat was synchronous (the
user waited blind until every master finished). Now each addressed master's reply **streams token-by-token,
attributed**, over the existing session WebSocket, while the synchronous `POST /sessions/{id}/group` stays as
the non-streaming path. The streaming primitive already existed: 4c's `AgentService::run_answer_turn` returns
the same `AgentEvent` stream the single-turn WS path forwards. `group::post` was refactored to share a `setup`
(resolve → persist user msg → snapshot) + `build_master_run` (isolated scratch + persona/model/tools) with the
new `group::stream_post`, which spawns one **forwarder task per addressed master**: each streams its
`(author, AgentEvent)` into a shared `mpsc` channel and, on `Complete`, posts its final reply back into the
group transcript (emitting a `Complete` with the **group** message id). `routes/ws.rs` auto-routes the existing
`Send` command to `stream_group` when the session is **team-bound** (`team_slug`) — no new `ClientCommand`.
New author-tagged `ServerEvent`s: `GroupStart{addressed}` / `MasterDelta{author,text}` /
`MasterComplete{author,message_id}` / `MasterError{author,message}` (non-terminal) / `GroupComplete`; a `Stop`
aborts every in-flight master. Desktop `GroupChat.tsx` now drives the WebSocket (`openStream` gained group
callbacks) with per-author live bubbles (bundle-only). **Deferred** (ADR-0012): tool-call visibility in the
group stream, interactive mid-stream approval (dispatch stays headless), sequential staging / workflows,
transcript summarization+RAG.

Phase 4f added (ADR-0012): **Bounded multi-round group turn-taking** — 4c/4e ran exactly one round; now an
master reply that explicitly `@mentions` another master drives a **follow-up round** so masters answer each
other (e.g. architect → `@copy-writer` → copy-writer replies), without the free-form auto-reply ADR-0012
forbids. Continuation is **mention-driven + hard-capped**: after each round, `mentions::followups` (a thin
self-excluding wrapper over `resolve` with an empty coordinator = explicit-mentions-only, **no** coordinator
fallback) scans the round's replies for explicit `@mentions` of *other* masters → the next round's addressed
set; the loop stops when a round yields no follow-ups **or** hits `MAX_GROUP_ROUNDS` (3; the sync API may
override into `1..=5` via `GroupPostRequest.max_rounds`). Deterministic (no LLM "continue?" meta-call), bounded
(the cap backstops echo/ping-pong), turn-taking (rounds sequential; within a round masters still run in
parallel from a **fresh** snapshot that includes prior rounds' replies). `group.rs` factored the per-round
dispatch into `run_round` + `resolve_followups`; `post` loops it (returning all rounds' replies flat), and
`stream_post` became a **single orchestrator task** running rounds sequentially over a channel of
`GroupStreamEvent::{RoundStart{round,addressed}, Master{round,author,event}}` (the orchestrator + every
in-flight master handle live in a shared `Arc<Mutex<Vec<AbortHandle>>>` so a `Stop` cancels them all). The
streamed `ServerEvent`s gained a `round`: `GroupStart{round,addressed}` / `MasterDelta{round,..}` /
`MasterComplete{round,..}` / `MasterError{round,..}`; `GroupComplete` still fires once at the end. Desktop
`GroupChat.tsx` keys bubbles by `(round, author)` with a per-round divider (bundle-only). **Deferred**
(ADR-0012): tool-call visibility, interactive mid-stream approval, coordinator-decides-next (LLM meta-routing),
declarative/sequential master workflows, transcript summarization+RAG.

Phase 4g added (ADR-0012): **Tool-call visibility in the group stream** — 4e/4f surfaced each master's text +
lifecycle but **dropped tool calls** (a `_ => None` arm in `stream_group` discarded `ToolCallStarted`/
`ToolResult`). Now a group master's tool activity streams **attributed**, like the single-turn chat. The
plumbing already carried it: `group::stream_master` forwards *every* `AgentEvent` into the channel, so the
tool events already reached `stream_group` tagged `(round, author)` — the slice just adds two wire events +
maps them (no change to `group.rs` dispatch, the agent loop, or the gate). New `ServerEvent`s
`MasterToolCall{round,author,id,tool,summary}` / `MasterToolResult{round,author,id,summary,is_error}` mirror
the single-turn `ToolCallStarted`/`ToolResult` with round+author; `ApprovalRequest` stays unmapped (group
dispatch is headless, so it can't occur). Desktop `GroupChat.tsx` renders dim `→ tool` / `← result` lines
under each `(round, author)` bubble (bundle-only). **Deferred** (ADR-0012): interactive mid-stream approval
(dispatch stays headless), streaming full tool args/raw output (summaries only), coordinator-decides-next,
declarative/sequential workflows, transcript summarization+RAG.

Phase 4h added (ADR-0010): **Portable master/team bundles** — export a team + the full definition of every
master it references as a self-contained **JSON `TeamBundle`**, and import a bundle into another project
(recreating the masters as `masters/<slug>.md` files + DB rows, then the team). The bundle is a **proto DTO,
not a YAML file** (no new dep, no file IO, no core change — leaner than Recipes); the desktop saves/loads the
JSON as the portable artifact. `getmasters-server::bundle` is pure orchestration over existing primitives:
`export` = `team::load_team` + `master_store(...).load(slug)` per member (∪ coordinator) + the 4a `to_dto`;
`import` = `from_dto` + `MasterStore::create` per master + `Store::upsert_team`. **Import overwrites** (upsert
— `create`/`upsert_team` already replace a same-slug file/row, so re-import is idempotent). HTTP via `GET
/projects/{id}/teams/{slug}/bundle` (export) + `POST /projects/{id}/bundles` (import)
(`TeamBundle`/`BundleImportResult`) + Teams.tsx Export button (downloads `<slug>.bundle.json`) & an Import
file-picker (bundle-only). **Deferred**: promote-a-team-to-a-Recipe, project templates, partial/remap import,
bundling a team's knowledge/memory/skills/connectors (masters + team structure only).

Phase 4i added (FR-39/46; ADR-0014): **External ACP master agents** — drive a pre-installed,
ACP-compatible **coding** CLI (Claude Code, Codex, OpenCode, Gemini CLI) as a *first-class master*. An
master gains a **`backend`** discriminator (`internal` | `acp`) + an `AcpLaunch` (command/args/env) on the
**file-backed `Master`** (migration 0017 adds an index-only `masters.backend` column; the `.md` file stays
the source of truth) — so an ACP master is addressable by the router, mentions, teams, group chat, and 4h
bundles with **zero changes** to those paths. The ACP wire lives **server-only** (`agent-client-protocol`
crate; lean core stays protocol-free): `getmasters-server::acp` plays the ACP **client** — spawns the harness,
runs `initialize`→`session/new`→`session/prompt`, and maps streaming `session/update` text onto the existing
`AgentEvent` stream. A single **`master::run_master_stream`** seam returns that stream for *either* backend, so
`master::run`, `team.rs`, and `group.rs` (sync + streaming) dispatch an ACP master identically (group.rs's
`build_master_run` became a backend-agnostic `make_scratch` + the seam). **Gate routing is the crux**
(ADR-0008): the harness runs its own tool loop, so its `fs/read_text_file`/`fs/write_text_file`/
`session/request_permission` callbacks each route through `getmasters-core::permission` (`PermissionGate::authorize`
→ folder-grant check + audit) *before* being honored — an out-of-grant write is **denied + audited** even under
headless auto-approval (the grant boundary is the security line). **Env posture differs from connectors**
(ADR-0014): an ACP coding harness inherits the daemon env **plus** its configured `acp_env` (it's a trusted,
user-installed agent that needs PATH/HOME/`npx`), rather than being `env_clear`-stripped. **Auto-detection**:
`GET /acp/harnesses` probes `PATH` for the known coding CLIs (`acp::registry`) so the desktop offers one-click
registration; detection never spawns the agent. `MasterDto`/`MasterSummaryDto` gain `backend`/`acp_*` (all
`#[serde(default)]`, backward-compatible) + a new `AvailableHarnessDto`. Headless test fixture: an **`acp-echo`**
stdio ACP *agent* bin (mirrors `mcp-echo`); the integration test (`tests/acp_masters.rs`) asserts the handshake
round-trip + env passthrough + gate-allowed-vs-denied write (with audit rows) + harness detection. **Deferred**:
remote ACP transports + OAuth, the full callback surface (terminals/plans/modes), the harness's own MCP servers
+ gating its internal toolset, interactive approval for ACP single-runs, resumable sessions, full transcript
replay into the ACP prompt. (The desktop `Masters.tsx` backend-selector UI has since landed in the
Desktop UI layer below.)

Build/test with `cargo build --workspace` / `cargo test --workspace` (the latter enables
getmasters-core's `testing` feature — the offline `MockProvider` fake — via the server's dev-dependency
+ Cargo feature unification; the `vec0` backend builds with `--features sqlite-vec`; PDF/DOCX extractor
tests with `--features pdf,docx`; the lean-core gate is `cargo test -p getmasters-core --features
testing`). The original tech-design package under `docs/` (incl. the eighteen
ADRs) remains the authoritative spec.

**Install/first-run (infra):** a GitHub Action (`.github/workflows/desktop-build.yml`) builds unsigned
macOS (arm64) + Windows (x64) installers on manual dispatch / `v*` tags. The daemon roots all on-disk
state in a **data home** — `~/.getmasters` by default (`getmasters.db` + `projects/`), resolved by
`getmasters_server::home` with precedence `GETMASTERS_DB_PATH` > `GETMASTERS_HOME` > `~/.getmasters` and created on
first run — so an installed app never writes to an unpredictable working dir. The desktop shows a
first-run **Onboarding** (`ui/desktop/src/components/Onboarding.tsx`, gated on effective provider =
`mock`) to set provider/key + an optional project & folder grant.

**Install telemetry + auto-update (infra):** the daemon reports a **single anonymous install event**
on first run — `getmasters_server::install::report_install` (spawned non-blocking from `main.rs`)
generates a random per-data-home `install_id` (settings table, no migration), detects the platform
(`os_type()`), and POSTs `{install_id, platform, os, app_version}` to `{base}/api/installs` (default
`https://getmasters.app`, env `GETMASTERS_TELEMETRY_URL`). It's **once per install** (gated on
`install_reported_at`, retried until 2xx) and **opt-out** (`GETMASTERS_NO_TELEMETRY` env or the
`telemetry_enabled` setting, surfaced as a checkbox in Settings → Environment; docs/06). The cloud
(`masters-cloud/apps/web`) receives it at `POST /api/installs` (Prisma + Postgres `InstallEvent`,
upsert by `installId` so retries don't duplicate). **In-app auto-update** uses
`tauri-plugin-updater` + `tauri-plugin-process` (`ui/desktop/src-tauri`, `plugins.updater` →
`https://getmasters.app/api/update/{{target}}/{{arch}}/{{current_version}}`, `createUpdaterArtifacts`);
the renderer checks on startup (`ui/desktop/src/lib/updater.ts`, App.tsx banner + Settings button) and
downloads/verifies/installs a signed build, then relaunches. The cloud serves the Tauri dynamic
manifest from the latest GitHub release (`apps/web/src/lib/update.ts` → `/api/update/...`), and CI
(`desktop-build.yml`) signs updater artifacts via `TAURI_SIGNING_PRIVATE_KEY*` secrets and uploads
`*.app.tar.gz`/`*-setup.exe`/`*.nsis.zip` + `.sig` to the release. The updater **pubkey** in
`tauri.conf.json` is a placeholder until the keypair is generated (`tauri signer generate`).

**Desktop UI layer** (post-bundle-only enhancement): a Notion/Manus-inspired **design system** —
CSS-variable tokens with an OS-following **and** manually-pinned theme (`ui/desktop/src/index.css` +
`src/lib/theme.ts`, a system/light/dark toggle in the `Sidebar`) and dependency-light shared
primitives (`src/components/ui/*`: Button/IconButton/Card/Input/Textarea/Select/Badge). A collapsible
`Sidebar` drives `chat | projects | settings`; every management screen is built on the primitives
(Masters/Teams/GroupChat/Routines/Recipes/Study/Memory/Skills/Connectors/Onboarding). `Masters.tsx`
exposes the 4i **ACP backend selector** with one-click **harness auto-detect** (`GET /acp/harnesses`)
+ `acp_*` fields, and **edit** flows exist for masters/teams. `Chat.tsx` adds **session history**
(`GET /sessions` switcher + new-chat + prior-message load) and a **right-hand audit panel** — the
session's gated tool-call trail via `client.listAudit` → `GET /sessions/{id}/audit`
(`routes::sessions::list_audit` → `Store::audit_entries` → `AuditEntryDto`; decision badge + redacted
args + timestamp). `GroupChat.tsx` adds a **rounds control** (`ClientCommand::Send.max_rounds`
threaded through `group::stream_post`/`run_rounds_streaming` via `clamp_rounds`, matching the sync
`POST /sessions/{id}/group`). `Settings.tsx` adds an **Environment & config-init panel** (the Hermes
`config` / `config check` analogue): `GET /settings/environment` (`EnvironmentDto`) surfaces the resolved
data-home/db path, **configured-vs-effective provider** (exposes the silent mock fallback), and a per-field
**config source** badge (`settings`/`env`/`default`) + env-override chips (names only); `POST /settings/check`
(`ConfigCheckDto`) validates with a **live provider test call** (`build_provider` + a tiny `ChatRequest` ping;
mock skips egress), routing failures distinctly (`ProviderError::Auth` → "check the API key"); and a **Re-run
setup wizard** button re-launches `Onboarding` from Settings via an `App.tsx` `forceOnboarding` flag (keychain
-only — no `.env` file). The desktop stays **bundle-only** — verified via `tsc --noEmit` +
`pnpm build`; the live Tauri window defers to a real desktop. The OpenAPI→TS client stays in
lock-step (`just gen-openapi` regenerates `openapi.json` + `schema.ts`).

**Masters sidebar** (standalone/global masters + quick chat; amends ADR-0011): a first-class
**Masters** nav item (`Sidebar.tsx`/`App.tsx` `View` gains `masters` → `MastersHub.tsx`) to **explore
system masters and create your own without a project**. Masters can now exist **globally**,
independent of any project: a **global master store** (`getmasters-core` migration 0018 `global_masters`
table + `MasterStore::global` rooted at `<data_home>/masters/`, mirroring the project store keyed on
`slug` alone) with project-less endpoints `GET/POST /masters` + `GET/DELETE /masters/{slug}` + a
built-in **template gallery** `GET /masters/templates` (`getmasters-server::master_templates::builtin`,
curated `MasterDto`s, `origin:"builtin"`) + a user-**starred default master** `GET/PUT /masters/default`
(a `settings` row). **Quick chat** (`POST /masters/quickchat`) starts an interactive chat with **one or
many** masters: it `ensure_default_project()`s (a lazily-created system default project as run context,
its id in `settings`), upserts an **ephemeral team** (`quick-<uuid>`, members = selection, coordinator =
starred default if selected else first), and binds a session via the existing `group::start` — so it
**reuses the whole group-chat machinery** (a single master = a one-member team). The one cross-cutting
seam is `master::load_master_any` (load project store → fall back to global), routed into `master::run`
+ `group::setup`, so a global master is addressable by the router/mentions/teams/group chat/bundles
unchanged. `MastersHub.tsx` reuses the exported `MasterForm`/`blankMaster` (ACP-aware editor) and a
session+roster-generalized `GroupChat.tsx` (new optional `openSession`/`members`/`coordinator`/`title`/
`backLabel` props; the Teams tab path is unchanged). Client methods: `listMasterTemplates`,
`{list,get,create,delete}GlobalMaster`, `{get,set}DefaultMaster`, `startQuickChat` (bundle-only).

**Cloud catalog sync (public system masters + skills):** system content is no longer baked into the
binary — the desktop syncs a **cloud-hosted catalog** so it changes without an app release. **Skills
gained the global tier masters got in 0018**: migration 0019 `global_skills`, a `Scope`/`SkillStore::global`
(`skills/mod.rs`, files under `<data_home>/skills/`), `Store::{upsert,list,get,delete}_global_skill` +
project `delete_skill`, `AppState::global_skill_store()`, and endpoints `GET /skills` + `GET/DELETE
/skills/{slug}` (`routes/skills_global.rs`). `getmasters-server::catalog` fetches `GET
{GETMASTERS_CATALOG_URL|getmasters.app}/api/catalog` (`CatalogDto { version, masters: [MasterDto], skills:
[SkillDto] }`; `SkillDto` gained `#[serde(default)]` `tags`/`steps`) and `apply_catalog` installs masters via
`from_dto` → `global_master_store().create()` (tagged `origin:"system"`, **skipping same-slug user masters**)
and skills via `global_skill_store().create_skill()`; it's **version-gated** (settings `catalog_version`) and
runs best-effort on startup (opt-out `GETMASTERS_NO_CATALOG_SYNC`) + `POST /catalog/sync` / `GET
/catalog/status`. Desktop `MastersHub.tsx` gains a **Sync from cloud** button + a **System Skills** tab
(`syncCatalog`/`getCatalogStatus`/`listGlobalSkills`/`deleteGlobalSkill`). The cloud
(`masters-cloud/apps/web`) serves it from Prisma `SystemMaster`/`SystemSkill` tables (seeded via
`prisma/seed.ts` from the former builtin gallery) at `GET /api/catalog` (`src/lib/catalog.ts`); the baked-in
`GET /masters/templates` remains as an offline fallback.

**Hardening pass** (post-sidebar; amends ADR-0014, extends ADR-0007/0008 seams): a cross-cutting
robustness + i18n + foundations batch. **Core loop** — `RunLimits` (env-overridable `GETMASTERS_*`:
max_tokens / tool iterations / tool + approval timeouts / transcript char budget / tool-result cap /
provider retries) threaded through `TurnRun`; Stop/disconnect now halts further side-effecting dispatch
(remaining calls recorded as cancelled — the transcript never ends on a dangling tool_use); approval
prompts time out to Deny (`ChannelApprover` + `ApprovalRegistry::cancel`); retryable provider failures
(429/5xx/transport — `ProviderError::{RateLimited,HttpStatus}` + `is_retryable`) retry with backoff and
partial streamed text is persisted; per-tool timeouts; all-read rounds execute concurrently; the
iteration cap ends with a final **no-tools** round (text answer, not an error); transcripts trim
oldest-first to the char budget and tool results are capped; token usage (`StreamChunk::Done.usage`)
persists to `messages.token_usage` (`MessageDto.token_usage`); Anthropic requests cache-mark the
system + tool prefix; the event channel is bounded. **ToolExecutor trait** (`extensions`) — the loop's
execution seam (`tool_schemas` + `execute`); `ExtensionManager` is the in-process impl,
`with_executor` injects fakes/remote executors (the cloud "hands" upgrade path). **Session event log**
(migration **0020** `events`) — append-only tool_call/tool_result/approval_requested/approval_decided/
complete/error rows from the loop + gate; `GET /sessions/{id}/events` (`EventDto`) — the
managed-agents "session = durable event log" slice (resume/wake later). **Unicode/CJK** — `slugify`
keeps Unicode letters; `masters::router::terms` emits CJK bigrams; mentions add a positional
`@slug`/`@name` substring pass — Chinese briefs/names route and address correctly. **Group chat** —
`PromptContext.participants` injects the teammate roster (so 4f mention-driven follow-ups can actually
trigger; ACP masters get it in the prompt text); scratch sessions are deleted after dispatch (+ a
startup GC for `group:%:%` orphans); the sync post returns partial results + per-master
`GroupPostResult.errors` instead of failing the round. **ACP hardening** (ADR-0014 amendment) —
`session/request_permission` grant-checks **located** calls per path (denied + audited out-of-grant,
even headless) and answers allow-once/reject-once (never blind first-option); group answer turns hand
the harness the speaker-labelled transcript; `session/update` tool calls map onto
ToolCallStarted/ToolResult + the event log (4g visibility for ACP); runs are bounded by
`GETMASTERS_ACP_TIMEOUT_SECS` (default 600s).

**Investing vertical slice 1** (docs/11 MVP start; ADR-0015/0016/0017 implemented, 0018 deferred):
the ask→track closed loop on the unchanged foundation. **Core** — migrations **0021** (`assets`
lifecycle spine `watching→holding→sold` + first-interest snapshot; `positions`/`txns` schema'd for
V1) and **0022** (`price_cache` with provenance: source/fetched_at/validation, global not
project-scoped); two new Study-precedent built-ins: `getmasters-core::assets` (`AssetsServer` —
`track_asset` Write = D8 silent-but-revocable under headless dispatch, `untrack_asset`
lifecycle-guarded, `list_assets` Read) and `getmasters-core::market` (`MarketDataServer` —
`get_quote`/`search_symbol` Read; one shared `MarketData::quote` cache-or-fetch path, 60min TTL,
stale fallback flagged, explicit error over fabricated numbers). The upstream fetch is a
**`MarketFetcher` trait injected from the server** (EmailTransport seam — core stays HTTP-free per
ADR-0015's lean-core reading, see its implementation note); `FixtureFetcher`/`FailingFetcher` behind
`testing`. `classify()` Read-arm gains `get_quote|search_symbol|list_assets` (narrow ruling,
documented). **Server** — `market_fetch.rs` Eastmoney push2 adapter (pure parsers, canned-JSON
tests, zero network in CI; Tencent = documented second-source slot); `AppState.market` +
`with_market_fetcher`; `assets`/`market` in `ALL_BUILTIN_SERVERS`+`IMPLEMENTED_SERVERS` (FR-19
free); `master_templates::investing()` — the 4-master pack (chief/analyst/risk/coach, stable ASCII
slugs via the new `MasterStore::create_with_slug` seam, Chinese personas embedding the shared
compliance block + research-card contract + D8 track mandate); `investing::ensure_workspace`
(idempotent lazy seed: default project → global masters w/ catalog semantics → standing `investing`
team → compliance instructions only-when-empty) behind `POST /investing/workspace`. **HTTP** —
`GET /projects/{id}/assets`, `DELETE .../assets/{symbol}` (204/404/409 lifecycle guard),
`GET .../quotes?symbols=` (batch-capped, per-symbol degrade) — same DeckDto flow, openapi+schema.ts
regenerated in lock-step. **Desktop** — `Watch.tsx` (asset cards: watch reason, snapshot line,
honest quote w/ ▲▼ + data-as-of + stale ⚠, untrack, empty-state bait questions, embedded GroupChat
vs the standing team, fixed compliance footer); `src/lib/i18n.ts` (tiny typed dict, zh-first, new
surfaces only); CJK font fallbacks on `--font-sans`; `--color-gain`/`--color-loss` tokens (CN
red=gain, both theme blocks). Integration tests (`tests/investing.rs`, all offline) cover workspace
idempotence + user-slug protection, the assets roundtrip, quote provenance + cache accounting, and
the **D8 closed loop** (a group master calls `assets.track_asset` headlessly through the gate).
**Slice 2 (proactive touch)** added the second touchpoint on the same machinery: two
seeded touch recipes (`investing::touch_recipes` — `weekly-watch-digest` Sun 12:00 UTC and
`watch-mover-sentinel` weekdays 07:30 UTC post-close Beijing, cron day-of-week uses names since
the `cron` crate rejects `0`), seeded **only-when-absent** (user-edited recipes/schedules are
never overwritten, unlike system masters) with `deliver_notify` on; the **silent-pass contract**
(`investing::NO_ALERT` + `is_silent` — a run with nothing to say is recorded but not delivered
and hidden from the feed: 超阈值才说话); `Store::list_project_runs` (runs⋈schedules) + `GET
/projects/{id}/briefings` (`BriefingDto`, full body = the run session's final assistant message;
ok + non-silent only) + a desktop `Briefings.tsx` feed (📰 nav, markdown cards, 就此提问 →
embedded expert GroupChat). **Deferred within the vertical:** the earnings sentinel (needs a
cninfo disclosure data face: adapter + filings/calendar cache), the cloud cross-section snapshot
+ weekly bulletin + daily master-quote pack, unread state on briefings, JournalServer,
FinCalcServer + portfolio unlock, redaction mode, dual-source validation, screenshot ingestion
(ADR-0018).

**Deferred to Phase 3 (later slices) and Phase 4:** the per-session **audit-log viewer** (`GET
/sessions/{id}/audit`) and **group `max_rounds` over the WS stream** have since landed in the Desktop
UI layer above; still-deferred **group-chat extensions** beyond the 4c/4e/4f/4g slices
(interactive mid-stream approval, coordinator-decides-next routing,
sequential staging, declarative master workflows, transcript summarization+RAG),
`route_brief` as a hosted MCP tool, promote-a-team-to-a-Recipe, project templates,
agent-authored "learned" masters, embedding-based routing — the single-master primitive landed in 4a, the team
+ router in 4b, @-mention group chat (addressing + attributed transcript + parallel-snapshot dispatch) in 4c,
**live WS streaming of group turns** in 4e, **bounded multi-round turn-taking** (mention-driven, hard-capped)
in 4f, **attributed tool-call visibility in the group stream** in 4g, and **portable master/team bundles**
(JSON export/import) in 4h, and **external ACP master agents** (drive a pre-installed coding CLI as a
gated master backend) in 4i; **external MCP servers** landed in 4d
(stdio connectors) with remote SSE/HTTP transports + OAuth still deferred (both for MCP connectors and ACP
masters); OCR for image PDFs; vector recall
for memory/skills; an
always-on background scheduler/delivery service (FR-27 messaging gateways + inbound are ADR-0009 non-goals).
The agent loop and prompt assembler mark the seams. When implementing further, follow the roadmap
and the ADRs rather than improvising stack choices — the eighteen foundational decisions are already
settled (see below; 0015–0018 are the design-only investing-vertical decisions, not yet implemented).

> **Note on `src-tauri`:** `ui/desktop/src-tauri` is deliberately excluded from the Cargo
> workspace so `cargo build --workspace` stays green in headless CI (Tauri needs webkit + a
> display). Build the desktop app only on a real desktop.

## What the project is

**Masters** is a planned local-first, single-user agentic desktop app for **personal study and work** — a
"Claude Cowork–like" agent that acts on the user's local files with human-in-the-loop approval, built on an
architecture adapted from [Goose](https://github.com/block/goose) (structure),
[Hermes Agent](https://github.com/NousResearch/hermes-agent) (the learning loop — Skills + file-backed memory,
ADRs 0006–0009), and [WorkBuddy](https://www.workbuddy.cn/) (Projects-as-context-container + Master Teams,
single-user slice — ADRs 0010–0011). Read `README.md` and `docs/00-overview.md` for the full framing.

## Documentation map (read these before designing or coding anything)

The docs are numbered and meant to be read in order; later docs depend on earlier ones.

- `docs/00-overview.md` — vision, personas, Cowork/Goose comparison
- `docs/01-product-requirements.md` — PRD; feature IDs `FR-*` / `NFR-*` are referenced throughout the roadmap
- `docs/02-architecture.md` — components, the Rust workspace layout, the agent loop, process lifecycle
- `docs/03-tech-stack.md` — technology choices + rationale
- `docs/04-extensions-mcp.md` — MCP extension model, built-in servers, recipe format
- `docs/05-data-storage-rag.md` — SQLite schema (illustrative DDL) + RAG pipeline
- `docs/06-security-privacy.md` — permission model, sandbox, audit, privacy boundary
- `docs/07-ux-flows.md` — screens and flows
- `docs/08-roadmap.md` — phased plan (Phase 0 → 4) and how features map to phases
- `docs/09-projects-masters.md` — Project as a context container + Masters (persona-over-Skill) + Master Teams
- `docs/10-ui-design.md` — desktop UI design system as implemented (tokens/layout/components/interaction) + Manus/Claude-Cowork benchmark + prioritized enhancement plan
- `docs/11-investment-agent.md` — the investing-vertical product positioning & design (《大师》 pivot; 13 settled product decisions D1–D13; design-only, not yet implemented)
- `docs/adr/0001..0018` — the binding decisions; treat these as authoritative (0006–0009 are the Hermes-informed refinements; 0010–0012 are the WorkBuddy-informed Project/Master-Team additions, 0012 = the multi-master group-chat communication model; 0013 = per-master model selection; 0014 = external ACP master agents; 0015–0018 = the investing-vertical decisions from docs/11 — domain packs, asset lifecycle, market-data supply, provider vision)

Diagrams are **mermaid** embedded in markdown.

## Settled architectural decisions (do not contradict without an ADR update)

| ADR | Decision |
|-----|----------|
| 0001 | Backend = **Rust** Cargo workspace: `getmasters-core` (lib) → `getmasters-server`/`getmastersd` (daemon, axum) + optional `getmasters-cli`; `getmasters-mcp` (built-in servers); `getmasters-proto` (shared DTOs) |
| 0002 | Desktop shell = **Tauri 2** + React + TypeScript + Vite + Tailwind/shadcn (not Electron) |
| 0003 | LLM = **Claude-first**, all access behind a `Provider` trait (`chat`/`stream`/`embed`); other providers added later |
| 0004 | RAG vector store = **SQLite + `sqlite-vec`** in the same `getmasters.db`; LanceDB is the documented upgrade path |
| 0005 | Extensions = **MCP via the official Rust SDK (`rmcp`)** for both hosting external servers and implementing built-ins |
| 0006 | **Skills** = self-improving procedural memory (agent-authored, gated, portable); bridges to Recipes |
| 0007 | Memory = **layered & file-backed** (`MEMORY.md`/`USER.md` as truth, DB as index) + **modular prompt assembly** |
| 0008 | Agent = **least-privilege (Blank Slate) + isolation** (MCP sandbox, credential stripping) + parallel subagents; **no** remote/serverless execution |
| 0009 | Delivery = **outbound-only** OS notifications + opt-in email (`send`-gated); **no** messaging-platform gateway |
| 0010 | **Master Team** = masters as *persona-over-Skill* + a master router; orchestration via **gated** parallel/sequential subagents; portable bundles, bridges to Recipes; **no** multi-user collaboration/handoff/inbound |
| 0011 | **Project = context container** bundling instructions/grants/knowledge/memory/skills/masters/connectors, auto-injected into each session with **project-scoped ranked above global**; project templates |
| 0012 | **Multi-master conversation** = single-user group chat; @-mention addressing (one/many/`@all`, unaddressed → coordinator master); shared author-attributed transcript (assembled per 0007); bounded turn-taking (no ad-hoc master↔master auto-reply); declarative master workflows; **"shared read context, isolated gated execution"** |
| 0013 | **Per-master model** = each master runs on its own **provider-qualified** `default_model` (any configured provider — Claude tiers/OpenAI/Ollama) via the `Provider` trait; **persona-fixed** (no runtime override); **per-master privacy boundary** (local-model masters stay on-device); fallback to default provider |
| 0014 | **External ACP master agents** = a coding-harness *backend* on the file-backed `Master` (`backend: acp`); Masters is the **ACP client** driving a pre-installed CLI (Claude Code/Codex/OpenCode/Gemini) over stdio; its **fs/permission callbacks route through the gate** (ADR-0008); inherits env + configured `acp_env` (trusted local agent, not env-stripped like MCP connectors); coding harnesses only |
| 0015 | **Vertical domain packs** (investing pivot, docs/11) = verticalization is **four layers on the unchanged foundation**: domain data/compute as built-in gated MCP servers (Study precedent) + content packs via catalog sync + vertical UI; core crates stay domain-free; compliance lives in content + fixed UI; the pattern is the template for future verticals |
| 0016 | **Asset lifecycle storage** = one `assets` spine (`watching→holding→sold`) behind a gated `AssetsServer` — watchlist and ledger are states, not features; **silent-but-revocable** tracking with point-in-time snapshots; holdings + risk profile accumulate **progressively from conversation** (no upfront demands); details never leave device (redaction mode for cloud contexts); hypothetical returns only in coached quarterly reviews |
| 0017 | **Market data supply** = hybrid: per-asset **EOD** data fetched **client-direct** from public channels (disclosure-first sourcing; adapters are catalog-hot-updated content; dual-source cross-validation with provenance, disputed → explicit ⚠ degrade) + market-wide cross-sections as one daily **cloud static-JSON snapshot** on CDN (carries the human-reviewed weekly bulletin) + cloud proxy as fallback only; **no realtime/intraday, no commercial redistribution licensing at this stage** |
| 0018 | **Provider vision** (deferred to P2) = image input enters through the `Provider` trait (capability-flagged; local-VLM route preserves the ADR-0013 privacy boundary) for **screenshot holdings extraction**: proposal → user-confirmed diff preview → gated AssetsServer writes; screenshots never persisted; **no chart reading** (compliance boundary) |

Cross-cutting invariants the design depends on:
- Every side-effecting tool call routes through **Permission & Audit** before execution — adding a tool must
  never bypass approval/audit gating (`docs/06`, `docs/04` §5).
- The desktop owns the `getmastersd` lifecycle; the daemon binds **loopback only** with a per-launch auth token.
- The desktop's TypeScript client is **generated from the daemon's OpenAPI** description — keep them in lock-step.

## Tooling

Per `docs/03-tech-stack.md` / `docs/08-roadmap.md`, the toolchain is **Cargo + Just + pnpm** (mirroring Goose's
setup: `cargo build`, task runners covering OpenAPI generation and a `run-ui` target, `cargo test`, `cargo fmt`,
`cargo clippy`; `pnpm` for the Tauri/React app). Both a `Justfile` (for `just` users) and a `Makefile` (the
`make`-based equivalent) are wired up; every recipe maps to a raw `cargo`/`pnpm` command. Notable targets:
`build`, `test`, `fmt`, `clippy`, `check`, `gen-openapi`, `run-ui` (full Tauri app — needs a desktop), and
**`make dev`** (`scripts/dev-web.sh` — the headless web path: the *desktop app* served as a plain web page,
daemon + Vite wired together, for WSL/browser dev). The **marketing landing page** lives in the separate
**`masters-cloud`** repository (Next.js + Tailwind), not in this repo. See `DEVELOPMENT.md` for the full
workflow.

## Repo conventions

- **Commit identity:** this repo's local git config is set to `shushuo-dev <shushuo.dev@gmail.com>` so GitHub
  attributes commits to the **shushuo-dev** account (that email is verified there). Do not change the author
  email — committing under a different verified email re-attributes the commit to another account.
- `main` is the default branch and the remote (`origin`, SSH) tracks it directly.
