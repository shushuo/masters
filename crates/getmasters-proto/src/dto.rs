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

/// One saved skill (name + summary), read-only for the UI.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SkillDto {
    pub slug: String,
    pub name: String,
    pub summary: String,
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
pub struct GroupPostRequest {
    pub content: String,
    /// Optional per-call cap on mention-driven follow-up rounds (Phase 4f). Clamped into `1..=5`;
    /// absent → the server default (`MAX_GROUP_ROUNDS`).
    #[serde(default)]
    pub max_rounds: Option<u32>,
}

/// Result of a group post: the masters addressed in the **first** round + every round's attributed
/// replies in order (Phase 4f: a post may run several mention-driven rounds).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct GroupPostResult {
    /// The master slugs the message resolved to in round 0 (mentions, `@all`, or the coordinator).
    pub addressed: Vec<String>,
    /// Each round's attributed replies, posted into the group session, in round-then-addressed order.
    pub replies: Vec<MessageDto>,
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
