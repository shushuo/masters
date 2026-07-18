//! Request/response DTOs for the HTTP surface.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Liveness/readiness probe payload (`GET /health`). Unauthenticated.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthDto {
    /// Always `"ok"` when the daemon is serving.
    pub status: String,
    /// The effective provider name (`"anthropic"`/`"openai"`, or `"unconfigured"` when no usable
    /// provider is set — the daemon still serves so the UI can configure one).
    pub provider: String,
    /// Whether a usable LLM provider is configured. `false` → the desktop opens the setup wizard.
    pub configured: bool,
    /// Daemon crate version.
    pub version: String,
}

/// Body for `POST /sessions`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct CreateSessionRequest {
    /// Optional owning project; `None` for an ad-hoc session.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Optional human-readable title.
    #[serde(default)]
    pub title: Option<String>,
}

/// A conversation thread.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionDto {
    pub id: String,
    pub project_id: Option<String>,
    pub title: Option<String>,
    /// The master team this session is a group chat for, if any (Phase 4c; null = ordinary session).
    #[serde(default)]
    pub team_slug: Option<String>,
    /// Epoch milliseconds.
    pub created_at: i64,
    /// Epoch milliseconds.
    pub updated_at: i64,
}

/// One turn within a session.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct MessageDto {
    pub id: String,
    pub session_id: String,
    /// `"user" | "assistant" | "tool"` (provider protocol role).
    pub role: String,
    /// Who authored the turn: `"user"` or a master slug (Phase 4c group chat). Defaults from `role`.
    #[serde(default)]
    pub author: String,
    /// JSON array of addressed master slugs / `["@all"]` for a group message (null = not addressed).
    #[serde(default)]
    pub addressed_to: Option<String>,
    /// Plain text in Phase 0; structured content blocks arrive later.
    pub content: String,
    /// Provider-reported token usage for this turn (input + output), when available.
    #[serde(default)]
    pub token_usage: Option<i64>,
    /// Epoch milliseconds.
    pub created_at: i64,
}

/// One audit-log entry: a gated tool call with its permission outcome (docs/06). Read-only;
/// surfaced per session so the user can see what was auto-allowed, approved, or denied.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct AuditEntryDto {
    pub id: String,
    pub tool: String,
    /// Redacted JSON of the tool arguments (secrets masked at write time), or null.
    pub args: Option<String>,
    /// `"auto"` | `"approved"` | `"denied"`.
    pub decision: String,
    /// Human-readable outcome (e.g. "created a.txt", "denied by user"), or null.
    pub result_summary: Option<String>,
    /// Epoch milliseconds when the call was recorded.
    pub created_at: i64,
}

/// One session event: an append-only record of run activity beyond the message transcript —
/// tool calls/results, approval requests + decisions, completion/errors. The durable event log
/// the managed-agents posture builds on (turn resume/wake later).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct EventDto {
    pub id: String,
    pub session_id: String,
    /// `tool_call` | `tool_result` | `approval_requested` | `approval_decided` | `complete` | `error`.
    pub kind: String,
    /// JSON detail for the event (redacted where applicable), or null.
    pub payload: Option<String>,
    /// Epoch milliseconds.
    pub created_at: i64,
}

/// Body for the non-streaming `POST /sessions/{id}/messages`.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SendMessageRequest {
    pub content: String,
}

/// A project (context container, ADR-0011).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub instructions: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Body for `POST /projects`.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateProjectRequest {
    pub name: String,
    #[serde(default)]
    pub instructions: Option<String>,
}

/// Body for `POST /projects/{id}/grants`.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct AddGrantRequest {
    pub path: String,
    /// `"read"` | `"read_write"`.
    pub access: String,
}

/// Body for `PUT /projects/{id}/instructions`.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SetInstructionsRequest {
    pub instructions: String,
}

/// One durable memory item (a `##` section of `MEMORY.md`/`USER.md`), read-only for the UI.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct MemoryDto {
    pub title: String,
    pub body: String,
    /// `"fact"` (MEMORY.md) | `"user"` (USER.md).
    pub scope: String,
    /// Backing file name.
    pub source: String,
}

/// One saved skill. `tags`/`steps` carry the full definition for the cloud catalog + import; the
/// read-only list endpoints leave them empty (both `#[serde(default)]`, backward-compatible).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SkillDto {
    pub slug: String,
    pub name: String,
    pub summary: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// The Markdown body (the steps).
    #[serde(default)]
    pub steps: String,
}

/// The cloud-hosted catalog of public **system** masters + skills the app syncs down.
/// `version` is an opaque token (the cloud's max `updated_at`) the app stores to skip unchanged syncs.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct CatalogDto {
    pub version: String,
    #[serde(default)]
    pub masters: Vec<MasterDto>,
    #[serde(default)]
    pub skills: Vec<SkillDto>,
}

/// Result/state of a catalog sync: the last-synced version + time and how many entries are installed.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct CatalogStatusDto {
    /// The last successfully-synced catalog version, or null if never synced.
    pub version: Option<String>,
    /// ISO/epoch timestamp of the last successful sync, or null.
    pub synced_at: Option<String>,
    /// Count of installed global masters.
    pub masters: u32,
    /// Count of installed global skills.
    pub skills: u32,
}

/// One flashcard deck with its review counts, read-only for the UI (Phase 3a, FR-13/14).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct DeckDto {
    pub name: String,
    /// Total cards in the deck.
    pub cards: i64,
    /// Cards currently due for review (`due_at <= now`).
    pub due: i64,
}

/// A project's active adaptive study plan, read-only for the UI (Phase 3b, FR-15).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct StudyPlanDto {
    pub title: String,
    /// Epoch milliseconds of the target deadline.
    pub deadline_at: i64,
    /// The agent-authored day-by-day plan (markdown).
    pub body: String,
}

/// One recipe parameter (substituted as `{{key}}` into the prompt) (Phase 3c, FR-16).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RecipeParamDto {
    pub key: String,
    #[serde(default)]
    pub description: String,
    /// Default value used when the run supplies no value for this key.
    #[serde(default)]
    pub default: Option<String>,
}

/// A recipe: a human-authored, parameterized automation whose `prompt` seeds the agent loop
/// (docs/04 §4). This is also the on-disk YAML schema (`recipes/<name>.yaml`).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RecipeDto {
    /// Stable identifier (slugified); also the YAML file name.
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parameters: Vec<RecipeParamDto>,
    /// The seed prompt; `{{key}}` placeholders are substituted at run time.
    pub prompt: String,
    /// MCP servers this recipe expects to be enabled (advisory).
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// A recipe's metadata for listing (no prompt/params).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RecipeSummaryDto {
    pub name: String,
    pub title: String,
    pub description: String,
}

/// Body for `POST /projects/{id}/recipes/{name}/run` — values for the recipe's parameters.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct RunRecipeRequest {
    #[serde(default)]
    pub params: std::collections::HashMap<String, String>,
}

/// Result of a recipe run: the session it ran in + the final assistant message.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RecipeRunResult {
    pub session_id: String,
    pub message: MessageDto,
}

/// A Master: a persona-over-Skill role descriptor (Phase 4a, FR-39/46; ADR-0010/0013).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct MasterDto {
    /// Canonical slug (filesystem-safe; derived from the name).
    #[serde(default)]
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    /// System-prompt fragment: voice, masterise, stance.
    pub persona: String,
    /// Provider-qualified model (`anthropic:claude-…`, `openai:…`, `ollama:…`); persona-fixed.
    #[serde(default)]
    pub default_model: String,
    /// Skills this master may use (stored + surfaced; enforcement deferred).
    #[serde(default)]
    pub allowed_skills: Vec<String>,
    /// Least-privilege tool allow-list (namespaced); empty = all the project's tools.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Expected deliverable shape.
    #[serde(default)]
    pub output_contract: String,
    /// `builtin` | `learned` | `imported`.
    #[serde(default)]
    pub origin: String,
    /// Extended persona instructions (Markdown body).
    #[serde(default)]
    pub body: String,
    /// Execution backend (Phase 4i, ADR-0014): `"internal"` (default) or `"acp"` (external coding CLI).
    #[serde(default = "default_internal")]
    pub backend: String,
    /// ACP only: the executable to spawn (e.g. `claude-code-acp`).
    #[serde(default)]
    pub acp_command: String,
    /// ACP only: command-line arguments.
    #[serde(default)]
    pub acp_args: Vec<String>,
    /// ACP only: environment passed to the agent process. `[key, value]` pairs.
    #[serde(default)]
    pub acp_env: Vec<[String; 2]>,
}

fn default_internal() -> String {
    "internal".to_string()
}

/// A master's listing metadata (no persona/body).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct MasterSummaryDto {
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub default_model: String,
    /// Execution backend (Phase 4i): `"internal"` or `"acp"` — lets the UI badge external agents.
    #[serde(default = "default_internal")]
    pub backend: String,
}

/// A pre-installed ACP coding harness detected on the machine (Phase 4i, ADR-0014). Returned by
/// `GET /acp/harnesses` so the desktop can offer one-click registration of an external master agent.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct AvailableHarnessDto {
    /// Stable id (`claude-code` | `codex` | `opencode` | `gemini`).
    pub id: String,
    /// Human label (e.g. "Claude Code").
    pub display_name: String,
    /// The launch command this harness is detected by.
    pub command: String,
    /// Whether `command` was found on the current `PATH`.
    pub available: bool,
    /// The command to register (may differ from `command`, e.g. an `npx` invocation).
    pub suggested_command: String,
    /// Suggested launch arguments.
    pub suggested_args: Vec<String>,
    /// Where to get the harness if it isn't installed.
    pub homepage: String,
}

/// Body for `POST /projects/{id}/masters/{slug}/run` — the brief to hand the master.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct RunMasterRequest {
    pub brief: String,
}

/// Result of a master run: the session it ran in + the master's final attributed message.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct MasterRunResult {
    pub session_id: String,
    pub message: MessageDto,
}

/// The user-starred default (global) master — the one quick chat uses when none is picked. `slug`
/// is empty when no default is set. Used by both `GET` and `PUT /masters/default`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct DefaultMasterDto {
    #[serde(default)]
    pub slug: String,
}

/// Body for `POST /masters/quickchat` — start an interactive group chat over an ad-hoc set of
/// (global) masters. One slug = a 1:1 chat; several = a multi-master group chat. The system default
/// project supplies the run context; the starred default master (if among `masters`) coordinates.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct QuickChatRequest {
    pub masters: Vec<String>,
}

/// A Master Team: a group of masters + a coordinator (Phase 4b, FR-38/40; ADR-0010).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct TeamDto {
    #[serde(default)]
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    /// Master slug that answers unaddressed briefs.
    #[serde(default)]
    pub coordinator_slug: String,
    /// Member master slugs.
    #[serde(default)]
    pub members: Vec<String>,
}

/// Body for `POST /projects/{id}/teams` — create or overwrite a team.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct CreateTeamRequest {
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub coordinator_slug: String,
    #[serde(default)]
    pub members: Vec<String>,
}

/// A team's listing metadata.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct TeamSummaryDto {
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub member_count: i64,
}

/// One master ranked by the router against a brief.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RankedMasterDto {
    pub slug: String,
    pub name: String,
    pub score: f32,
}

/// Body for `POST /projects/{id}/teams/{slug}/route` — the brief to rank against.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct RouteBriefRequest {
    pub brief: String,
}

/// The router's recommendation: masters ranked + the selected one (executes nothing).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RouteResultDto {
    pub ranked: Vec<RankedMasterDto>,
    /// Slug of the auto-selected master (top match, else the coordinator).
    pub selected_slug: String,
}

/// Body for `POST /projects/{id}/teams/{slug}/run` — a brief, with an optional manual override.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct RunTeamRequest {
    pub brief: String,
    /// Manual override: dispatch to this master slug instead of the routed selection.
    #[serde(default)]
    pub master: Option<String>,
}

/// Result of a team run: which master was chosen + that master's run result.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct TeamRunResult {
    pub selected_slug: String,
    pub result: MasterRunResult,
}

/// A portable team bundle (Phase 4h; ADR-0010): a team + the full definition of every master it
/// references, self-contained so it can be exported from one project and imported into another.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct TeamBundle {
    /// Bundle format version (currently `1`).
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    /// Master slug that answers unaddressed briefs.
    #[serde(default)]
    pub coordinator_slug: String,
    /// Member master slugs (in team order).
    #[serde(default)]
    pub members: Vec<String>,
    /// The full definition of every master the team references (members + coordinator).
    #[serde(default)]
    pub masters: Vec<MasterDto>,
}

/// Result of importing a [`TeamBundle`] into a project: the recreated team slug + master slugs.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct BundleImportResult {
    pub team_slug: String,
    pub masters: Vec<String>,
}

/// Body for `POST /sessions/{id}/group` — a user message into a group chat (Phase 4c, FR-43).
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct StartGroupSessionRequest {
    /// Optional session title (e.g. the first question, truncated) so the topic list is
    /// recognizable; absent → the `group:<team>` default.
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct GroupPostRequest {
    pub content: String,
    /// Optional per-call cap on mention-driven follow-up rounds (Phase 4f). Clamped into `1..=5`;
    /// absent → the server default (`MAX_GROUP_ROUNDS`).
    #[serde(default)]
    pub max_rounds: Option<u32>,
}

/// One master's failure within a group round (the other masters' replies still return).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct GroupMasterErrorDto {
    /// The failing master's slug.
    pub author: String,
    pub message: String,
}

/// Result of a group post: the masters addressed in the **first** round + every round's attributed
/// replies in order (Phase 4f: a post may run several mention-driven rounds).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct GroupPostResult {
    /// The master slugs the message resolved to in round 0 (mentions, `@all`, or the coordinator).
    pub addressed: Vec<String>,
    /// Each round's attributed replies, posted into the group session, in round-then-addressed order.
    pub replies: Vec<MessageDto>,
    /// Masters that failed (per round, in dispatch order). A partial round still returns the
    /// successful replies instead of failing the whole post.
    #[serde(default)]
    pub errors: Vec<GroupMasterErrorDto>,
}

/// An external MCP server connector for a project (Phase 4d, FR-20; ADR-0005).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ConnectorDto {
    /// Tool-namespace prefix (e.g. `filesystem` → `filesystem.read_file`).
    pub name: String,
    /// The executable to spawn (a stdio MCP server).
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment for the child (the ONLY env it receives — credential stripping). `[key, value]` pairs.
    #[serde(default)]
    pub env: Vec<[String; 2]>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Body for `POST /projects/{id}/connectors` — create or overwrite a connector.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct CreateConnectorRequest {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<[String; 2]>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Body for `PUT /projects/{id}/connectors/{name}` — enable/disable.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct SetConnectorEnabledRequest {
    pub enabled: bool,
}

/// A scheduled automation: fire a recipe once or on a cron expression (Phase 3d, FR-17).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ScheduleDto {
    pub id: String,
    pub recipe_name: String,
    /// `"once"` | `"cron"`.
    pub kind: String,
    pub cron_expr: Option<String>,
    /// Epoch ms of the next fire (null when disabled/done).
    pub next_run_at: Option<i64>,
    pub enabled: bool,
    /// Push an on-device OS notification with the run output (Phase 3e, FR-27).
    pub deliver_notify: bool,
    /// Email the run output to the configured address (opt-in `send`; Phase 3e, FR-27).
    pub deliver_email: bool,
}

/// Body for `POST /projects/{id}/schedules`.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateScheduleRequest {
    pub recipe_name: String,
    /// Parameter overrides passed to the recipe on each run.
    #[serde(default)]
    pub params: std::collections::HashMap<String, String>,
    /// `"once"` | `"cron"`.
    pub kind: String,
    /// Cron expression (5-7 fields) when `kind = "cron"`.
    #[serde(default)]
    pub cron_expr: Option<String>,
    /// Epoch ms to fire at when `kind = "once"`.
    #[serde(default)]
    pub run_at: Option<i64>,
    /// Push an on-device OS notification with the run output (Phase 3e, FR-27).
    #[serde(default)]
    pub deliver_notify: bool,
    /// Email the run output to the configured address (opt-in `send`; Phase 3e, FR-27).
    #[serde(default)]
    pub deliver_email: bool,
}

/// Body for `PUT /projects/{id}/schedules/{sid}` — enable/disable a schedule and/or change its
/// delivery flags. Only present fields change.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct SetScheduleRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub deliver_notify: Option<bool>,
    #[serde(default)]
    pub deliver_email: Option<bool>,
}

/// One recorded firing of a schedule (run history).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ScheduledRunDto {
    /// Epoch ms when the run started.
    pub started_at: i64,
    /// `"ok"` | `"error"`.
    pub status: String,
    pub session_id: Option<String>,
    pub summary: Option<String>,
}

/// A built-in MCP server and whether it's enabled for a project (FR-19).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ExtensionDto {
    /// Server name, e.g. `"files"` / `"knowledge"` / `"memory"` / `"skills"`.
    pub name: String,
    /// Whether it's hosted for the project (absent override = enabled).
    pub enabled: bool,
    /// Whether the server is actually implemented yet (placeholders are read-only in the UI).
    pub implemented: bool,
}

/// Body for `PUT /projects/{id}/extensions/{name}`.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SetExtensionRequest {
    pub enabled: bool,
}

/// One indexed knowledge document (read-only for the UI).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct DocumentDto {
    pub path: String,
    pub mime: Option<String>,
    /// Epoch milliseconds.
    pub indexed_at: i64,
}

/// A project's knowledge-index status + its indexed documents.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeStatusDto {
    pub documents: i64,
    pub chunks: i64,
    /// Epoch milliseconds of the most recent ingest, if any.
    pub last_indexed_at: Option<i64>,
    pub paths: Vec<DocumentDto>,
}

/// Result of reverting the last file operation in a session.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RevertResult {
    pub summary: String,
}

/// Current effective settings (secrets are reported as present/absent, never returned).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SettingsDto {
    /// `"anthropic" | "openai"`.
    pub provider: String,
    pub model: String,
    pub openai_base_url: Option<String>,
    /// Whether an Anthropic API key is configured (keychain or env).
    pub anthropic_key_set: bool,
    /// Whether an OpenAI API key is configured (keychain or env).
    pub openai_key_set: bool,
    /// Whether anonymous install telemetry is enabled (on by default; opt-out).
    #[serde(default = "default_true")]
    pub telemetry_enabled: bool,
}

/// The resolved runtime environment (the `hermes config` view analogue). Read-only; surfaces where
/// each effective value comes from and which env-var overrides are active. Secrets are reported as
/// present/absent and env-var overrides as **names only** — never values.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct EnvironmentDto {
    /// Resolved data-home directory (e.g. `~/.getmasters`).
    pub data_home: String,
    /// Resolved SQLite database path inside the data home.
    pub db_path: String,
    /// The provider selected in settings/env (`"anthropic" | "openai"`).
    pub configured_provider: String,
    /// The provider actually usable given the configured credentials, or `"unconfigured"` when
    /// none has a usable key (the daemon refuses to start in that state).
    pub effective_provider: String,
    pub model: String,
    pub openai_base_url: Option<String>,
    pub anthropic_key_set: bool,
    pub openai_key_set: bool,
    /// Where `provider` resolves from: `"settings" | "env" | "default"`.
    pub provider_source: String,
    /// Where `model` resolves from: `"settings" | "env" | "default"`.
    pub model_source: String,
    /// Where `openai_base_url` resolves from: `"settings" | "env" | "default"`.
    pub base_url_source: String,
    /// Names (only) of the Masters/provider env vars currently set in the daemon's environment.
    pub env_overrides: Vec<String>,
}

/// A single diagnostic from the config check (the `hermes config check` analogue).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ConfigCheckItem {
    pub name: String,
    /// `"ok" | "warn" | "error"`.
    pub status: String,
    pub detail: String,
}

/// Result of validating the current configuration, including a live provider test call.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ConfigCheckDto {
    /// True when no check has `status == "error"`.
    pub ok: bool,
    pub effective_provider: String,
    pub checks: Vec<ConfigCheckItem>,
}

/// Partial update of non-secret settings (only present fields change).
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct SettingsUpdate {
    /// Active provider — any catalog id (`"anthropic"`, `"openai"`, `"deepseek"`, …).
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub openai_base_url: Option<String>,
    /// Per-provider base-URL overrides keyed by catalog id; `""` clears an override.
    #[serde(default)]
    pub provider_bases: Option<HashMap<String, String>>,
    /// Enable/disable anonymous install telemetry (opt-out; on by default).
    #[serde(default)]
    pub telemetry_enabled: Option<bool>,
}

/// One provider in the configurable catalog, with its current (non-secret) state.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ProviderStateDto {
    /// Stable catalog id (also the secret-name prefix), e.g. `"deepseek"`.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Transport: `"anthropic" | "openai_compatible"`.
    pub transport: String,
    /// Built-in default base URL (OpenAI-compatible vendors), if any.
    pub default_base: Option<String>,
    /// The user's base-URL override, if set.
    pub base_url: Option<String>,
    /// Docs / API-key page.
    pub docs_url: String,
    /// A local/loopback endpoint that needs no API key (e.g. Ollama).
    pub is_local: bool,
    /// The generic custom OpenAI-compatible slot (user supplies the base URL).
    pub custom: bool,
    /// Whether an API key is configured for this provider (presence only — never the key).
    pub key_set: bool,
}

/// The provider catalog plus which one is the active default.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ProvidersDto {
    /// The active provider id.
    pub active: String,
    pub providers: Vec<ProviderStateDto>,
}

/// Set a secret (API key, SMTP password) — written to the OS keychain, never persisted to the DB.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SecretUpdate {
    /// `"anthropic_api_key" | "openai_api_key" | "smtp_password"`.
    pub name: String,
    pub value: String,
}

/// Outbound-email (SMTP) settings for routine delivery (Phase 3e, FR-27). Off by default; the
/// password lives in the keychain (reported as present/absent, never returned).
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct EmailSettingsDto {
    /// Whether email delivery is turned on (a routine still opts in per-schedule).
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    /// The `From:` address routine emails are sent as.
    pub from: Option<String>,
    /// The destination address routine output is delivered to.
    pub to: Option<String>,
    /// Whether an SMTP password is stored in the keychain.
    pub password_set: bool,
}

/// Partial update of email settings (only present fields change). The password is set separately
/// via `PUT /settings/secret` (`smtp_password`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct EmailSettingsUpdate {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
}

/// One tracked instrument on the asset lifecycle spine (investing vertical, ADR-0016).
/// `state`: `watching` | `holding` | `sold`. The snapshot fields are the first-interest record.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct AssetDto {
    /// Canonical symbol, e.g. `sh600519`.
    pub symbol: String,
    pub name: String,
    pub market: String,
    /// `"stock"` | `"fund"`.
    pub kind: String,
    pub state: String,
    #[serde(default)]
    pub watch_reason: Option<String>,
    /// Epoch ms of first interest.
    pub watched_at: i64,
    #[serde(default)]
    pub snapshot_price: Option<f64>,
    /// `YYYY-MM-DD` the snapshot price is for.
    #[serde(default)]
    pub snapshot_date: Option<String>,
}

/// A market quote with provenance (ADR-0017). `stale` = an old cached value served because a
/// refresh failed — the UI must render this honestly, never hide it.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct QuoteDto {
    pub symbol: String,
    #[serde(default)]
    pub name: Option<String>,
    /// `YYYY-MM-DD` the quote is for.
    pub trade_date: String,
    #[serde(default)]
    pub close: Option<f64>,
    #[serde(default)]
    pub prev_close: Option<f64>,
    #[serde(default)]
    pub change_pct: Option<f64>,
    /// Adapter id, e.g. `eastmoney`.
    pub source: String,
    /// Epoch ms.
    pub fetched_at: i64,
    /// `"unverified"` | `"verified"` | `"disputed"`.
    pub validation: String,
    pub stale: bool,
}

/// One proactive-touch briefing (a delivered scheduled-run output — the 简报流 feed item).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct BriefingDto {
    /// Epoch ms when the run started.
    pub started_at: i64,
    /// The producing recipe's slug (e.g. `weekly-watch-digest`).
    pub recipe_name: String,
    /// The recipe's display title (falls back to the slug when the recipe file is gone).
    pub title: String,
    /// The run session (for audit/trace).
    pub session_id: Option<String>,
    /// The full briefing body (markdown — the run's final assistant message).
    pub body: String,
}

/// One index in the cloud daily market cross-section (ADR-0017 — the market-wide snapshot the
/// cloud publishes once per trading day).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct MarketIndexDto {
    pub symbol: String,
    pub name: String,
    #[serde(default)]
    pub close: Option<f64>,
    #[serde(default)]
    pub change_pct: Option<f64>,
    pub trade_date: String,
}

/// The human-reviewed weekly bulletin (D13 「本周市场三件事」 — retrospective, market-wide, no
/// individual names). Only the latest published one is served.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct MarketBulletinDto {
    pub slug: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub published_at: Option<String>,
}

/// One 大师一句 quote from the cloud pack (D13 daily heartbeat).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct DailyQuoteDto {
    pub text: String,
    #[serde(default)]
    pub who: String,
}

/// The cloud daily payload (`GET {catalog_base}/api/snapshot/daily`), proxied to the desktop by
/// the daemon (best-effort, briefly cached). Empty when the cloud is unreachable — the desktop
/// then falls back to its local quote pack, so the heartbeat is a nicety, never a dependency.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct DailySnapshotDto {
    #[serde(default)]
    pub snapshot_date: Option<String>,
    #[serde(default)]
    pub indices: Vec<MarketIndexDto>,
    #[serde(default)]
    pub bulletin: Option<MarketBulletinDto>,
    #[serde(default)]
    pub quotes: Vec<DailyQuoteDto>,
}

/// One valued holding in the portfolio overview (nullable everywhere — an unvalued position is
/// reported honestly, never estimated).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct PortfolioPositionDto {
    pub symbol: String,
    pub name: String,
    #[serde(default)]
    pub quantity: Option<f64>,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub close: Option<f64>,
    /// `quantity × close` when both known.
    #[serde(default)]
    pub value: Option<f64>,
    /// Share of the valued total (0..1).
    #[serde(default)]
    pub weight: Option<f64>,
    #[serde(default)]
    pub trade_date: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub stale: bool,
}

/// The deterministic portfolio overview (FinCalc — docs/11 M2).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct PortfolioDto {
    #[serde(default)]
    pub total_value: Option<f64>,
    /// Herfindahl–Hirschman concentration over valued weights.
    #[serde(default)]
    pub hhi: Option<f64>,
    #[serde(default)]
    pub top3_share: Option<f64>,
    pub positions: Vec<PortfolioPositionDto>,
    /// Holdings that could not be valued (missing quantity or quote).
    pub unvalued_count: i64,
}

/// The seeded investing workspace (docs/11): the default project + the standing expert team.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct InvestingWorkspaceDto {
    pub project_id: String,
    pub team_slug: String,
    /// The coordinator master slug (answers unaddressed messages).
    pub coordinator: String,
    /// Member master slugs.
    pub members: Vec<String>,
}

// --- Simulation Investment Lab (模拟投资实验室) ------------------------------

/// A simulation's constraints (Alpha-Arena-style given conditions).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SimConstraintsDto {
    /// Reject short positions. Default: true.
    #[serde(default = "default_true")]
    pub long_only: bool,
    /// Per-symbol cap as a fraction 0..1 (e.g. 0.4 = 40%).
    #[serde(default)]
    pub max_weight: Option<f64>,
    /// Minimum cash weight as a fraction 0..1.
    #[serde(default)]
    pub cash_floor: Option<f64>,
    /// Benchmark symbol for the fixed buy-and-hold comparison line.
    #[serde(default)]
    pub benchmark: Option<String>,
    /// Round-trip turnover fee in basis points (0 = frictionless).
    #[serde(default)]
    pub fee_bps: f64,
}

impl Default for SimConstraintsDto {
    fn default() -> Self {
        Self {
            long_only: true,
            max_weight: None,
            cash_floor: None,
            benchmark: None,
            fee_bps: 0.0,
        }
    }
}

/// A simulation ("模拟盘"): masters compete under fixed conditions. Forward-in-time paper trading —
/// virtual portfolios mark to live EOD prices as the real market moves between rounds.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SimulationDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub scenario: Option<String>,
    /// Canonical symbols the masters may allocate across.
    pub universe: Vec<String>,
    pub starting_cash: f64,
    #[serde(default)]
    pub constraints: SimConstraintsDto,
    /// `"active"` | `"paused"` | `"ended"` | `"running"`.
    pub state: String,
    pub round_no: i64,
    pub created_at: i64,
    /// Participants with their latest leaderboard standing.
    #[serde(default)]
    pub participants: Vec<SimLeaderboardRowDto>,
    /// The cron expression of the auto-round schedule, when one is set.
    #[serde(default)]
    pub schedule_cron: Option<String>,
}

/// Body for `POST /projects/{id}/simulations`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct CreateSimulationRequest {
    pub name: String,
    #[serde(default)]
    pub scenario: Option<String>,
    /// Symbols (canonical or common forms) the masters may allocate across.
    pub universe: Vec<String>,
    pub starting_cash: f64,
    #[serde(default)]
    pub constraints: SimConstraintsDto,
    /// Participant master slugs (global or project masters). A benchmark line is added
    /// automatically when `constraints.benchmark` is set.
    pub participants: Vec<String>,
}

/// One participant's standing on the leaderboard (latest valuation).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SimLeaderboardRowDto {
    /// Master slug, or `"__benchmark__"` for the fixed buy-and-hold line.
    pub master_slug: String,
    /// Latest post-round NAV (null when nothing could be valued yet).
    #[serde(default)]
    pub nav: Option<f64>,
    pub cash: f64,
    /// Cumulative return vs. starting cash (0.1 = +10%).
    #[serde(default)]
    pub return_pct: Option<f64>,
    /// Excess return over the benchmark line (this row's return − the benchmark's), when a
    /// benchmark participant exists. `None` for the benchmark itself and when there is none.
    #[serde(default)]
    pub alpha: Option<f64>,
    /// Cumulative-return series across rounds (oldest first) — the equity sparkline.
    #[serde(default)]
    pub equity: Vec<f64>,
    #[serde(default)]
    pub unvalued_count: i64,
}

/// One master's decision in a round (targets + captured reasoning).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SimDecisionDto {
    pub master_slug: String,
    /// Target weights (percent of NAV) the engine applied; empty when held/unparsed.
    #[serde(default)]
    pub targets: std::collections::HashMap<String, f64>,
    #[serde(default)]
    pub summary: Option<String>,
    /// The master's full reasoning reply (RETuning-style framework → evidence → decision).
    #[serde(default)]
    pub reasoning: Option<String>,
    /// The run session (for audit/trace).
    #[serde(default)]
    pub session_id: Option<String>,
    /// False when the decision block was unparseable → the master held this round.
    pub parsed: bool,
    /// Post-round NAV for this participant.
    #[serde(default)]
    pub nav: Option<f64>,
    #[serde(default)]
    pub return_pct: Option<f64>,
    /// Token usage of the run (cost signal).
    #[serde(default)]
    pub tokens: Option<i64>,
}

/// One decision round with every participant's decision.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SimRoundDto {
    pub round_no: i64,
    #[serde(default)]
    pub quote_date: Option<String>,
    pub status: String,
    pub run_at: i64,
    #[serde(default)]
    pub decisions: Vec<SimDecisionDto>,
}

/// Result of running one round (`POST .../rounds`).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SimRoundResultDto {
    pub round_no: i64,
    #[serde(default)]
    pub quote_date: Option<String>,
    pub leaderboard: Vec<SimLeaderboardRowDto>,
    pub decisions: Vec<SimDecisionDto>,
}

/// Body for `PUT /projects/{id}/simulations/{sid}/schedule` — set or clear the auto-round cron.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct SetSimScheduleRequest {
    /// Cron expression (5-7 fields) to run a round on; null/empty clears the schedule.
    #[serde(default)]
    pub cron_expr: Option<String>,
    /// Push an on-device OS notification with the round digest.
    #[serde(default)]
    pub deliver_notify: bool,
    /// Email the round digest (opt-in `send`).
    #[serde(default)]
    pub deliver_email: bool,
}

/// Uniform error envelope returned on any 4xx/5xx.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ErrorDto {
    pub error: String,
}

impl ErrorDto {
    pub fn new(msg: impl Into<String>) -> Self {
        Self { error: msg.into() }
    }
}
