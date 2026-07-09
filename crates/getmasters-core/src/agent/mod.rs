//! The shared agent loop.
//!
//! `run_turn` is the single core path behind both the daemon's WebSocket and the CLI
//! (ADR-0001). Phase 1a makes it a **multi-turn tool loop**: stream the model, and whenever
//! it requests a tool, gate the call through Permission & Audit, dispatch it via the
//! [`ToolExecutor`] seam, feed the result back, and re-call the model — until it stops or a
//! bound is hit. Every side-effecting call passes the gate before execution (docs/06).
//!
//! Operational bounds live in [`RunLimits`]: provider retries with backoff, per-tool timeouts,
//! an approval-prompt timeout, a transcript char budget (oldest-first trimming), a tool-result
//! size cap, and a graceful final no-tools round when the iteration cap is reached.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{Stream, StreamExt};
use getmasters_proto::SideEffect;

use crate::extensions::{ExtensionManager, ToolExecutor};
use crate::permission::{
    policy, ApprovalRegistry, ApprovalRequest, Approver, Authorized, AutoApprover, ChannelApprover,
    GrantSet, PermissionGate,
};
use crate::prompt::PromptAssembler;
use crate::provider::{
    ChatMessage, ChatRequest, ContentBlock, Provider, ProviderError, Role, StreamChunk, TokenUsage,
};
use crate::store::Store;

/// Buffered events between the turn task and its consumer; a slow consumer applies
/// backpressure instead of growing memory without bound.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Operational bounds for a single run. Every field has a sane default; the daemon resolves
/// overrides from `GETMASTERS_*` env vars via [`RunLimits::from_env`].
#[derive(Clone, Debug)]
pub struct RunLimits {
    /// `max_tokens` sent to the provider per model call.
    pub max_tokens: u32,
    /// Tool-calling rounds within a single turn; the final round runs with **no tools** so the
    /// model must answer in text instead of the turn erroring out.
    pub max_tool_iterations: usize,
    /// Bound on a single tool execution — a hung tool (e.g. a wedged external MCP connector)
    /// becomes an error result instead of wedging the turn.
    pub tool_timeout: Duration,
    /// Bound on an unanswered approval prompt (timeout → deny).
    pub approval_timeout: Duration,
    /// Transcript size budget (in chars) sent to the model; oldest messages are dropped first.
    pub transcript_char_budget: usize,
    /// Cap on a single tool result fed back into the transcript.
    pub tool_result_char_cap: usize,
    /// Retries for retryable provider failures (429/5xx/transport), with exponential backoff.
    pub provider_retries: u32,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_tokens: 4096,
            max_tool_iterations: 8,
            tool_timeout: Duration::from_secs(120),
            approval_timeout: crate::permission::DEFAULT_APPROVAL_TIMEOUT,
            transcript_char_budget: 200_000,
            tool_result_char_cap: 16_000,
            provider_retries: 2,
        }
    }
}

impl RunLimits {
    /// Resolve limits from `GETMASTERS_*` env vars, defaulting anything unset or unparsable:
    /// `GETMASTERS_MAX_TOKENS`, `GETMASTERS_MAX_TOOL_ITERATIONS`, `GETMASTERS_TOOL_TIMEOUT_SECS`,
    /// `GETMASTERS_APPROVAL_TIMEOUT_SECS`, `GETMASTERS_TRANSCRIPT_CHAR_BUDGET`,
    /// `GETMASTERS_TOOL_RESULT_CHAR_CAP`, `GETMASTERS_PROVIDER_RETRIES`.
    pub fn from_env() -> Self {
        fn get<T: std::str::FromStr>(key: &str) -> Option<T> {
            std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
        }
        let d = Self::default();
        Self {
            max_tokens: get("GETMASTERS_MAX_TOKENS").unwrap_or(d.max_tokens),
            max_tool_iterations: get("GETMASTERS_MAX_TOOL_ITERATIONS")
                .unwrap_or(d.max_tool_iterations),
            tool_timeout: get("GETMASTERS_TOOL_TIMEOUT_SECS")
                .map(Duration::from_secs)
                .unwrap_or(d.tool_timeout),
            approval_timeout: get("GETMASTERS_APPROVAL_TIMEOUT_SECS")
                .map(Duration::from_secs)
                .unwrap_or(d.approval_timeout),
            transcript_char_budget: get("GETMASTERS_TRANSCRIPT_CHAR_BUDGET")
                .unwrap_or(d.transcript_char_budget),
            tool_result_char_cap: get("GETMASTERS_TOOL_RESULT_CHAR_CAP")
                .unwrap_or(d.tool_result_char_cap),
            provider_retries: get("GETMASTERS_PROVIDER_RETRIES").unwrap_or(d.provider_retries),
        }
    }
}

/// Events emitted by [`AgentService::run_turn`].
#[derive(Clone, Debug)]
pub enum AgentEvent {
    /// Incremental assistant text.
    Delta(String),
    /// The model requested a tool call (before gating/dispatch).
    ToolCallStarted {
        id: String,
        name: String,
        summary: String,
    },
    /// A side-effecting call needs the user's approval.
    ApprovalRequest(ApprovalRequest),
    /// A tool finished (or was denied); `summary` is a short result.
    ToolResult {
        id: String,
        summary: String,
        is_error: bool,
    },
    /// The assistant turn was persisted under `message_id`.
    Complete { message_id: String },
    /// A terminal error ended the turn.
    Error(String),
}

/// Structured error for one turn. The `AgentEvent::Error` boundary flattens it to a string,
/// but the internals keep the provider taxonomy so retry logic can act on it.
#[derive(Debug)]
enum TurnError {
    Provider(ProviderError),
    Store(String),
    Loop(String),
}

impl std::fmt::Display for TurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TurnError::Provider(e) => write!(f, "provider: {e}"),
            TurnError::Store(m) => write!(f, "{m}"),
            TurnError::Loop(m) => write!(f, "{m}"),
        }
    }
}

/// Orchestrates the store, a provider, tools, and the permission gate to run conversational turns.
#[derive(Clone)]
pub struct AgentService {
    store: Store,
    provider: Arc<dyn Provider>,
    model: String,
    extensions: Arc<dyn ToolExecutor>,
    grants: Arc<GrantSet>,
    approval: Option<Arc<ApprovalRegistry>>,
    /// `Some` = only these tools are advertised/allowed (Blank Slate); `None` = all.
    enabled_tools: Option<Arc<HashSet<String>>>,
    /// Blank Slate posture: re-prompt every side-effect (no standing permissions).
    blank_slate: bool,
    /// The project data dir (file-backed Memory + Skills); `Some` under a project (ADR-0011).
    project_dir: Option<std::path::PathBuf>,
    /// The acting master's persona, injected into the system prompt when running as a master
    /// (ADR-0010/0013); `None` for ordinary turns.
    persona: Option<String>,
    /// The author this run's messages are attributed to (a master slug) in group chat (ADR-0012);
    /// `None` for ordinary turns (messages attributed by role). When set, the transcript is rendered
    /// speaker-labelled from this author's perspective.
    author: Option<String>,
    /// Group-chat roster `(slug, name)` injected into the prompt so a master knows its teammates
    /// and can hand off with `@slug` mentions (Phase 4f); empty outside group chat.
    participants: Vec<(String, String)>,
    /// Operational bounds for the run.
    limits: RunLimits,
}

impl AgentService {
    /// A no-tools agent (the plain chat path).
    pub fn new(store: Store, provider: Arc<dyn Provider>, model: impl Into<String>) -> Self {
        Self {
            store,
            provider,
            model: model.into(),
            extensions: Arc::new(ExtensionManager::empty()),
            grants: Arc::new(GrantSet::empty()),
            approval: None,
            enabled_tools: None,
            blank_slate: false,
            project_dir: None,
            persona: None,
            author: None,
            participants: Vec::new(),
            limits: RunLimits::default(),
        }
    }

    /// Set the project data dir so the loop can auto-inject file-backed Memory + Skills (ADR-0007).
    pub fn with_project_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.project_dir = Some(dir);
        self
    }

    /// Run this agent as a master: inject `persona` into the system prompt (ADR-0010/0013).
    pub fn with_persona(mut self, persona: impl Into<String>) -> Self {
        self.persona = Some(persona.into());
        self
    }

    /// Attribute this run's persisted messages to `author` (a master slug) and render the transcript
    /// speaker-labelled from its perspective — used for group-chat answer turns (ADR-0012).
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Tell this run who else is in the group chat (`(slug, name)` per teammate) so the prompt
    /// can list them and the master can hand off via `@slug` mentions (Phase 4f).
    pub fn with_participants(mut self, participants: Vec<(String, String)>) -> Self {
        self.participants = participants;
        self
    }

    /// Override the run's operational bounds (the daemon passes [`RunLimits::from_env`]).
    pub fn with_limits(mut self, limits: RunLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Swap the provider + model for this agent (per-master model dispatch, ADR-0013). The
    /// `(provider, model)` pair is resolved via [`crate::provider::resolve_provider`].
    pub fn with_model_provider(
        mut self,
        provider: Arc<dyn Provider>,
        model: impl Into<String>,
    ) -> Self {
        self.provider = provider;
        self.model = model.into();
        self
    }

    /// Boot in Blank Slate / least-privilege mode (ADR-0008): start with no tools enabled and
    /// no standing permissions; capability is granted incrementally via [`Self::with_enabled_tools`].
    pub fn blank_slate(mut self, on: bool) -> Self {
        self.blank_slate = on;
        if on && self.enabled_tools.is_none() {
            self.enabled_tools = Some(Arc::new(HashSet::new()));
        }
        self
    }

    /// Restrict the agent to a specific set of tool names (namespaced, e.g. `files.read`).
    pub fn with_enabled_tools(mut self, tools: HashSet<String>) -> Self {
        self.enabled_tools = Some(Arc::new(tools));
        self
    }

    /// Attach a hosted Extension Manager and the matching folder grants (enables tools).
    pub fn with_extensions(
        mut self,
        extensions: Arc<ExtensionManager>,
        grants: Arc<GrantSet>,
    ) -> Self {
        self.extensions = extensions;
        self.grants = grants;
        self
    }

    /// Attach any [`ToolExecutor`] implementation (the "hands" seam). [`Self::with_extensions`]
    /// is the in-process convenience; tests inject fakes here, and a remote executor is the
    /// documented upgrade path.
    pub fn with_executor(mut self, executor: Arc<dyn ToolExecutor>) -> Self {
        self.extensions = executor;
        self
    }

    /// Attach an approval registry so side-effecting calls prompt the user over the WS.
    /// Without one, the agent auto-approves (CLI/tests).
    pub fn with_approval_registry(mut self, registry: Arc<ApprovalRegistry>) -> Self {
        self.approval = Some(registry);
        self
    }

    /// Clear any approval registry so this agent's runs auto-approve (within grants, still audited).
    /// Used for headless runs with no interactive approver — e.g. a recipe "run now" (Phase 3c).
    pub fn without_approval(mut self) -> Self {
        self.approval = None;
        self
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The approval registry, if approvals are wired (used by the WS handler to resolve
    /// `ApprovalDecision`s against this agent's runs).
    pub fn approval_registry(&self) -> Option<Arc<ApprovalRegistry>> {
        self.approval.clone()
    }

    pub fn provider_name(&self) -> &'static str {
        self.provider.name()
    }

    /// Tool names currently advertised by the hosted extensions (introspection / tests).
    pub fn extension_tool_names(&self) -> Vec<String> {
        self.extensions
            .tool_schemas()
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    /// Call a hosted tool directly, **bypassing the permission gate** — for tests/introspection only.
    /// Production turns always dispatch through the gated loop in `execute`.
    pub async fn call_tool_ungated(
        &self,
        name: &str,
        input: &serde_json::Value,
    ) -> std::result::Result<(String, bool), String> {
        self.extensions.execute(name, input).await
    }

    /// Run one turn for `session_id` given the user's `user_text`. Returns a stream of events.
    pub async fn run_turn(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> impl Stream<Item = AgentEvent> + Send + 'static {
        self.spawn_turn(session_id, Some(user_text.to_string()))
    }

    /// Run an **answer** turn over the session's *existing* transcript — no new user message is
    /// appended (Phase 4c group chat: the user message is already posted; each addressed master
    /// answers the shared transcript). Replies are attributed to this agent's [`Self::with_author`].
    pub fn run_answer_turn(
        &self,
        session_id: &str,
    ) -> impl Stream<Item = AgentEvent> + Send + 'static {
        self.spawn_turn(session_id, None)
    }

    fn spawn_turn(
        &self,
        session_id: &str,
        user_text: Option<String>,
    ) -> impl Stream<Item = AgentEvent> + Send + 'static {
        let store = self.store.clone();
        let provider = self.provider.clone();
        let model = self.model.clone();
        let extensions = self.extensions.clone();
        let grants = self.grants.clone();
        let approval = self.approval.clone();
        let enabled_tools = self.enabled_tools.clone();
        let blank_slate = self.blank_slate;
        let project_dir = self.project_dir.clone();
        let persona = self.persona.clone();
        let author = self.author.clone();
        let participants = self.participants.clone();
        let limits = self.limits.clone();
        let session_id = session_id.to_string();

        let (tx, rx) = tokio::sync::mpsc::channel::<AgentEvent>(EVENT_CHANNEL_CAPACITY);

        tokio::spawn(async move {
            let run = TurnRun {
                store,
                provider,
                model,
                extensions,
                grants,
                approval,
                enabled_tools,
                blank_slate,
                project_dir,
                persona,
                author,
                participants,
                limits,
                session_id,
                tx: tx.clone(),
            };
            if let Err(e) = run.execute(user_text.as_deref()).await {
                run.log_event("error", serde_json::json!({ "message": e.to_string() }));
                let _ = tx.send(AgentEvent::Error(e.to_string())).await;
            }
        });

        tokio_stream_from(rx)
    }

    /// Run a turn to completion and return the persisted assistant message (non-streaming
    /// convenience used by `POST /sessions/{id}/messages`). Uses auto-approval.
    pub async fn complete_turn(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> std::result::Result<getmasters_proto::MessageDto, String> {
        let mut stream = self.run_turn(session_id, user_text).await;
        let mut message_id: Option<String> = None;
        while let Some(event) = stream.next().await {
            match event {
                AgentEvent::Complete { message_id: id } => message_id = Some(id),
                AgentEvent::Error(e) => return Err(e),
                _ => {}
            }
        }
        let id = message_id.ok_or_else(|| "turn ended without completion".to_string())?;
        self.store.get_message(&id).map_err(|e| e.to_string())
    }

    /// Run an answer turn (no user message appended) to completion and return the persisted reply,
    /// attributed to [`Self::with_author`] (Phase 4c group chat).
    pub async fn complete_answer_turn(
        &self,
        session_id: &str,
    ) -> std::result::Result<getmasters_proto::MessageDto, String> {
        let mut stream = self.run_answer_turn(session_id);
        let mut message_id: Option<String> = None;
        while let Some(event) = stream.next().await {
            match event {
                AgentEvent::Complete { message_id: id } => message_id = Some(id),
                AgentEvent::Error(e) => return Err(e),
                _ => {}
            }
        }
        let id = message_id.ok_or_else(|| "turn ended without completion".to_string())?;
        self.store.get_message(&id).map_err(|e| e.to_string())
    }
}

/// One model round's outcome: the streamed text, the requested tool calls, and usage.
struct RoundOutput {
    text: String,
    pending: Vec<(String, String, serde_json::Value)>,
    usage: Option<TokenUsage>,
}

/// Per-run state for the tool loop.
struct TurnRun {
    store: Store,
    provider: Arc<dyn Provider>,
    model: String,
    extensions: Arc<dyn ToolExecutor>,
    grants: Arc<GrantSet>,
    approval: Option<Arc<ApprovalRegistry>>,
    enabled_tools: Option<Arc<HashSet<String>>>,
    blank_slate: bool,
    project_dir: Option<std::path::PathBuf>,
    persona: Option<String>,
    author: Option<String>,
    participants: Vec<(String, String)>,
    limits: RunLimits,
    session_id: String,
    tx: tokio::sync::mpsc::Sender<AgentEvent>,
}

impl TurnRun {
    /// Send an event to the consumer; `false` means the receiver is gone (disconnect / Stop).
    async fn emit(&self, ev: AgentEvent) -> bool {
        self.tx.send(ev).await.is_ok()
    }

    /// Best-effort append to the durable session event log (migration 0019) — a failed write
    /// is logged, never failing the turn.
    fn log_event(&self, kind: &str, payload: serde_json::Value) {
        if let Err(e) = self
            .store
            .append_event(&self.session_id, kind, Some(&payload.to_string()))
        {
            tracing::debug!(error = %e, kind, "failed to append session event");
        }
    }

    /// Build the permission gate for this run (ChannelApprover when an approval registry is
    /// present, else AutoApprover).
    fn gate(&self) -> PermissionGate {
        let approver: Arc<dyn Approver> = match &self.approval {
            Some(registry) => {
                let tx = self.tx.clone();
                let emit = Arc::new(move |req: ApprovalRequest| {
                    // The emit closure is sync; hop onto the runtime for the bounded send.
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let _ = tx.send(AgentEvent::ApprovalRequest(req)).await;
                    });
                });
                Arc::new(ChannelApprover::new(
                    registry.clone(),
                    emit,
                    self.limits.approval_timeout,
                ))
            }
            None => Arc::new(AutoApprover),
        };
        PermissionGate::new(
            self.grants.clone(),
            approver,
            self.store.clone(),
            Some(self.session_id.clone()),
        )
        .least_privilege(self.enabled_tools.clone(), self.blank_slate)
    }

    async fn execute(&self, user_text: Option<&str>) -> Result<(), TurnError> {
        // 1. Persist the user turn — unless this is an answer turn over an existing transcript
        //    (Phase 4c group chat: the user message is already posted).
        if let Some(user_text) = user_text {
            self.store
                .insert_message(&self.session_id, "user", user_text)
                .map_err(|e| TurnError::Store(format!("persist user message: {e}")))?;
        }

        // Advertise only enabled tools (Blank Slate restricts this to the granted subset).
        let tools: Vec<_> = self
            .extensions
            .tool_schemas()
            .iter()
            .filter(|t| match &self.enabled_tools {
                Some(enabled) => enabled.contains(&t.name),
                None => true,
            })
            .cloned()
            .collect();

        // SEAM (ADR-0007): assemble the system prompt from editable sources, auto-injecting the
        // project's file-backed Memory + Skills when the project container is active.
        let system = {
            let project_id = self
                .store
                .get_session(&self.session_id)
                .ok()
                .and_then(|s| s.project_id);
            let instructions = project_id
                .as_deref()
                .and_then(|pid| self.store.project_instructions(pid).ok().flatten());
            let knowledge_present = tools.iter().any(|t| t.name == "knowledge.search");
            let curation_present = tools
                .iter()
                .any(|t| t.name == "memory.remember" || t.name == "skills.create_skill");
            let (memory_block, skill_summaries) = match (&project_id, self.project_dir.is_some()) {
                (Some(pid), true) => (
                    crate::memory::load_memory_context(&self.store, pid),
                    crate::skills::load_skill_summaries(&self.store, pid)
                        .map(|v| v.into_iter().map(|s| (s.name, s.summary)).collect())
                        .unwrap_or_default(),
                ),
                _ => (None, Vec::new()),
            };
            let ctx = crate::prompt::PromptContext {
                persona: self.persona.as_deref(),
                project_instructions: instructions.as_deref(),
                tools_present: !tools.is_empty(),
                knowledge_present,
                curation_present,
                memory_block,
                skill_summaries,
                participants: &self.participants,
            };
            PromptAssembler::assemble(&ctx)
        };

        let gate = self.gate();

        let max_iters = self.limits.max_tool_iterations.max(1);
        for iteration in 0..max_iters {
            if self.tx.is_closed() {
                return Ok(()); // stopped between rounds
            }

            // Graceful cap: the final round advertises no tools, so the model must answer in
            // text and the turn completes instead of erroring out.
            let final_round = iteration + 1 == max_iters;
            let round_tools = if final_round {
                Vec::new()
            } else {
                tools.clone()
            };

            let messages = self.load_transcript()?;
            let mut req = ChatRequest::new(self.model.clone(), messages);
            req.system = system.clone();
            req.tools = round_tools;
            req.max_tokens = self.limits.max_tokens;

            let Some(round) = self.run_model_round(req).await? else {
                return Ok(()); // receiver gone mid-stream (disconnect / Stop)
            };

            // Persist the assistant turn. A text-only turn is stored as plain text (readable
            // transcripts + a human-friendly HTTP `content`); a turn with tool calls is stored
            // as JSON content blocks so the tool_use survives reload. `parse_blocks` reads both.
            let content = if round.pending.is_empty() {
                round.text.clone()
            } else {
                let mut blocks: Vec<ContentBlock> = Vec::new();
                if !round.text.is_empty() {
                    blocks.push(ContentBlock::Text {
                        text: round.text.clone(),
                    });
                }
                for (id, name, input) in &round.pending {
                    blocks.push(ContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    });
                }
                blocks_json(&blocks)
            };
            let assistant_msg = self
                .persist("assistant", &content)
                .map_err(|e| TurnError::Store(format!("persist assistant message: {e}")))?;
            if let Some(usage) = round.usage {
                if let Err(e) = self
                    .store
                    .set_message_token_usage(&assistant_msg.id, usage.total() as i64)
                {
                    tracing::debug!(error = %e, "failed to record token usage");
                }
            }

            if round.pending.is_empty() {
                self.log_event(
                    "complete",
                    serde_json::json!({ "message_id": assistant_msg.id }),
                );
                let _ = self
                    .emit(AgentEvent::Complete {
                        message_id: assistant_msg.id,
                    })
                    .await;
                return Ok(());
            }

            // Gate + dispatch each requested tool, then persist the whole round's results so
            // the transcript never ends on a dangling tool_use — even when stopped mid-round.
            let results = self.dispatch_round(&gate, round.pending).await;
            self.persist("tool", &blocks_json(&results))
                .map_err(|e| TurnError::Store(format!("persist tool result: {e}")))?;
            if self.tx.is_closed() {
                return Ok(()); // stopped mid-round; remaining calls were marked cancelled
            }
            // Loop: re-call the provider with the appended results.
        }

        Err(TurnError::Loop(format!(
            "tool loop exceeded {max_iters} iterations"
        )))
    }

    /// One model call: stream text/tool-use chunks, retrying retryable failures with backoff
    /// (mid-stream failures only retry while nothing has been consumed). Returns `None` when
    /// the event receiver is gone (disconnect / Stop). Partial text from a failed stream is
    /// persisted so it isn't lost.
    async fn run_model_round(&self, req: ChatRequest) -> Result<Option<RoundOutput>, TurnError> {
        let mut attempt: u32 = 0;
        'retry: loop {
            let mut stream = match self.provider.stream(req.clone()).await {
                Ok(s) => s,
                Err(e) => {
                    if e.is_retryable() && attempt < self.limits.provider_retries {
                        attempt += 1;
                        tokio::time::sleep(backoff(attempt)).await;
                        continue 'retry;
                    }
                    return Err(TurnError::Provider(e));
                }
            };

            let mut out = RoundOutput {
                text: String::new(),
                pending: Vec::new(),
                usage: None,
            };
            let mut consumed = false;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(StreamChunk::TextDelta(t)) => {
                        consumed = true;
                        out.text.push_str(&t);
                        if !self.emit(AgentEvent::Delta(t)).await {
                            return Ok(None);
                        }
                    }
                    Ok(StreamChunk::ToolUse { id, name, input }) => {
                        consumed = true;
                        let summary = summarize(&name, &input);
                        self.log_event(
                            "tool_call",
                            serde_json::json!({ "id": id, "tool": name, "summary": summary }),
                        );
                        if !self
                            .emit(AgentEvent::ToolCallStarted {
                                id: id.clone(),
                                name: name.clone(),
                                summary,
                            })
                            .await
                        {
                            return Ok(None);
                        }
                        out.pending.push((id, name, input));
                    }
                    Ok(StreamChunk::Done { usage, .. }) => {
                        out.usage = usage;
                        break;
                    }
                    Err(e) => {
                        if !consumed && e.is_retryable() && attempt < self.limits.provider_retries {
                            attempt += 1;
                            tokio::time::sleep(backoff(attempt)).await;
                            continue 'retry;
                        }
                        // Keep whatever streamed before the failure.
                        if !out.text.is_empty() {
                            let _ = self.persist(
                                "assistant",
                                &format!("{}\n\n…[provider stream interrupted]", out.text),
                            );
                        }
                        return Err(TurnError::Provider(e));
                    }
                }
            }
            return Ok(Some(out));
        }
    }

    /// Dispatch one round's tool calls. An all-read round (every call classifies as
    /// `SideEffect::Read`) executes concurrently — reads auto-allow inside grants, so there is
    /// no approval interaction to serialize. Anything else runs sequentially; once the receiver
    /// is gone (Stop), remaining side-effecting calls are **not executed** and are recorded as
    /// cancelled so the transcript stays well-formed.
    async fn dispatch_round(
        &self,
        gate: &PermissionGate,
        pending: Vec<(String, String, serde_json::Value)>,
    ) -> Vec<ContentBlock> {
        let all_read = pending
            .iter()
            .all(|(_, name, _)| policy::classify(name) == SideEffect::Read);

        let outcomes: Vec<(String, String, bool)> = if all_read && pending.len() > 1 {
            futures::future::join_all(pending.iter().map(|(id, name, input)| {
                let id = id.clone();
                async move {
                    let (summary, is_error) = self.dispatch_tool(gate, name, input).await;
                    (id, summary, is_error)
                }
            }))
            .await
        } else {
            let mut out = Vec::with_capacity(pending.len());
            for (id, name, input) in &pending {
                let (summary, is_error) = if self.tx.is_closed() {
                    ("cancelled by user".to_string(), true)
                } else {
                    self.dispatch_tool(gate, name, input).await
                };
                out.push((id.clone(), summary, is_error));
            }
            out
        };

        let mut results: Vec<ContentBlock> = Vec::with_capacity(outcomes.len());
        for (id, summary, is_error) in outcomes {
            self.log_event(
                "tool_result",
                serde_json::json!({
                    "id": id,
                    "summary": truncate(&summary, 200),
                    "is_error": is_error,
                }),
            );
            let _ = self
                .emit(AgentEvent::ToolResult {
                    id: id.clone(),
                    summary: truncate(&summary, 200),
                    is_error,
                })
                .await;
            results.push(ContentBlock::ToolResult {
                tool_use_id: id,
                content: cap_tool_result(&summary, self.limits.tool_result_char_cap),
                is_error,
            });
        }
        results
    }

    /// Gate + execute one tool call (with the per-tool timeout), returning `(summary, is_error)`.
    async fn dispatch_tool(
        &self,
        gate: &PermissionGate,
        name: &str,
        input: &serde_json::Value,
    ) -> (String, bool) {
        match gate.authorize(name, input).await {
            Authorized::Denied(reason) => (format!("permission denied: {reason}"), true),
            Authorized::Allowed => {
                // Capture the pre-image so this op can be reverted (docs/06).
                let pending_rev = crate::revision::capture(&self.grants, name, input);
                match tokio::time::timeout(
                    self.limits.tool_timeout,
                    self.extensions.execute(name, input),
                )
                .await
                {
                    Ok(Ok((out, err))) => {
                        if !err {
                            if let Some(rev) = pending_rev {
                                crate::revision::commit(&self.store, Some(&self.session_id), rev);
                            }
                        }
                        (out, err)
                    }
                    Ok(Err(e)) => (format!("tool error: {e}"), true),
                    Err(_) => (
                        format!(
                            "tool timed out after {}s",
                            self.limits.tool_timeout.as_secs()
                        ),
                        true,
                    ),
                }
            }
        }
    }

    /// Persist a message for this run, attributed to the run's author when set (group chat); else
    /// by role (ordinary chat). Role stays the provider-protocol role.
    fn persist(
        &self,
        role: &str,
        content: &str,
    ) -> crate::error::Result<getmasters_proto::MessageDto> {
        match &self.author {
            Some(author) => {
                self.store
                    .insert_message_attributed(&self.session_id, author, role, content, None)
            }
            None => self.store.insert_message(&self.session_id, role, content),
        }
    }

    /// Load the session transcript as provider messages, parsing JSON content blocks for
    /// assistant/tool rows (user rows always ride as plain text — never block-parsed, so a user
    /// message that happens to be valid block JSON isn't misread), then trim to the char budget.
    ///
    /// In group chat (`self.author` set), the transcript is rendered from that master's perspective:
    /// its own past turns are `Assistant`; every other author (the user + other masters) rides as
    /// `User` with a `[author]` speaker label so the model can tell participants apart (ADR-0012's
    /// "shared read context"). In ordinary chat the rows are unlabelled.
    fn load_transcript(&self) -> Result<Vec<ChatMessage>, TurnError> {
        let rows = self
            .store
            .list_messages(&self.session_id)
            .map_err(|e| TurnError::Store(format!("load transcript: {e}")))?;
        let msgs = rows
            .into_iter()
            .map(|m| {
                let blocks = if m.role == "user" {
                    vec![ContentBlock::Text {
                        text: m.content.clone(),
                    }]
                } else {
                    parse_blocks(&m.content)
                };
                match &self.author {
                    Some(me) if &m.author == me => ChatMessage {
                        role: Role::Assistant,
                        content: blocks,
                    },
                    Some(_) => ChatMessage {
                        // Other participants (user + other masters) ride as labelled user turns.
                        role: Role::User,
                        content: label_speaker(&m.author, blocks),
                    },
                    None => {
                        let role = match m.role.as_str() {
                            "assistant" => Role::Assistant,
                            _ => Role::User, // "user" and "tool" both ride as user-role to the provider
                        };
                        ChatMessage {
                            role,
                            content: blocks,
                        }
                    }
                }
            })
            .collect();
        Ok(trim_to_budget(msgs, self.limits.transcript_char_budget))
    }
}

/// Exponential backoff for provider retries: 1s, 2s, 4s, …, capped at 16s.
fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(1000u64.saturating_mul(1 << attempt.saturating_sub(1).min(4)))
}

/// Approximate content size of one provider message (chars of text + serialized tool payloads).
fn message_len(m: &ChatMessage) -> usize {
    m.content
        .iter()
        .map(|b| match b {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolUse { input, .. } => input.to_string().len(),
            ContentBlock::ToolResult { content, .. } => content.len(),
        })
        .sum()
}

/// Trim a transcript to `budget` by dropping the oldest whole messages (never the newest one).
/// A leading orphan tool_result (whose tool_use was dropped) is dropped too, and a synthetic
/// user note marks the truncation so the model knows context is missing.
fn trim_to_budget(mut msgs: Vec<ChatMessage>, budget: usize) -> Vec<ChatMessage> {
    let mut total: usize = msgs.iter().map(message_len).sum();
    if total <= budget {
        return msgs;
    }
    let mut dropped = 0usize;
    while msgs.len() > 1 && total > budget {
        let m = msgs.remove(0);
        total -= message_len(&m);
        dropped += 1;
    }
    while msgs.len() > 1
        && msgs[0]
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
    {
        msgs.remove(0);
        dropped += 1;
    }
    if dropped > 0 {
        msgs.insert(
            0,
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: format!(
                        "[earlier conversation truncated — {dropped} older messages omitted]"
                    ),
                }],
            },
        );
    }
    msgs
}

/// Cap a tool result fed back into the transcript, keeping the head and noting what was cut.
fn cap_tool_result(s: &str, cap: usize) -> String {
    let len = s.chars().count();
    if len <= cap {
        return s.to_string();
    }
    let kept: String = s.chars().take(cap).collect();
    let cut = len - cap;
    format!("{kept}\n…[truncated {cut} chars — re-read with a narrower scope if needed]")
}

/// Prefix the first text block with a `[author]` speaker label (group-chat attribution). Non-text
/// leading blocks get a standalone label block.
fn label_speaker(author: &str, mut blocks: Vec<ContentBlock>) -> Vec<ContentBlock> {
    match blocks.first_mut() {
        Some(ContentBlock::Text { text }) => {
            *text = format!("[{author}] {text}");
        }
        _ => blocks.insert(
            0,
            ContentBlock::Text {
                text: format!("[{author}]"),
            },
        ),
    }
    blocks
}

fn parse_blocks(content: &str) -> Vec<ContentBlock> {
    serde_json::from_str::<Vec<ContentBlock>>(content).unwrap_or_else(|_| {
        vec![ContentBlock::Text {
            text: content.to_string(),
        }]
    })
}

fn blocks_json(blocks: &[ContentBlock]) -> String {
    serde_json::to_string(blocks).unwrap_or_else(|_| "[]".to_string())
}

/// Short human-readable summary of a tool call for events + approval prompts: the target
/// path/recipient when present, plus a preview of any `content` being written so the user can
/// see *what* they're approving.
fn summarize(name: &str, input: &serde_json::Value) -> String {
    let mut s = match input
        .get("path")
        .or_else(|| input.get("to"))
        .and_then(|v| v.as_str())
    {
        Some(p) => format!("{name} {p}"),
        None => name.to_string(),
    };
    if let Some(content) = input.get("content").and_then(|v| v.as_str()) {
        let preview: String = content.chars().take(120).collect();
        let ellipsis = if content.chars().count() > 120 {
            "…"
        } else {
            ""
        };
        s.push_str(&format!(" — \"{preview}{ellipsis}\""));
    }
    s
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}…")
    }
}

/// Adapt an `mpsc::Receiver` into a `Stream` without pulling in `tokio-stream`.
fn tokio_stream_from(
    mut rx: tokio::sync::mpsc::Receiver<AgentEvent>,
) -> impl Stream<Item = AgentEvent> + Send + 'static {
    futures::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use super::*;
    use crate::provider::{MockProvider, ToolSchema};
    use async_trait::async_trait;
    use futures::stream::{self, BoxStream};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    #[tokio::test]
    async fn run_turn_streams_and_persists() {
        let store = Store::open_in_memory().unwrap();
        let session = store.create_session(None, None).unwrap();
        let agent = AgentService::new(store.clone(), Arc::new(MockProvider::new()), "mock");

        let events: Vec<AgentEvent> = agent.run_turn(&session.id, "hello").await.collect().await;

        let text: String = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::Delta(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "echo: hello");
        assert!(matches!(events.last(), Some(AgentEvent::Complete { .. })));

        let msgs = store.list_messages(&session.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
    }

    /// A fake executor: one `probe.run` tool, optional per-call delay, records calls.
    struct FakeExec {
        delay: Option<Duration>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl ToolExecutor for FakeExec {
        fn tool_schemas(&self) -> Vec<ToolSchema> {
            vec![ToolSchema {
                name: "probe.run".into(),
                description: "probe".into(),
                input_schema: json!({"type": "object"}),
            }]
        }

        async fn execute(&self, name: &str, _input: &Value) -> Result<(String, bool), String> {
            self.calls.lock().unwrap().push(name.to_string());
            if let Some(d) = self.delay {
                tokio::time::sleep(d).await;
            }
            Ok(("probe ok".into(), false))
        }
    }

    /// A scripted provider: fails the first `fail_first` stream() calls with a retryable error;
    /// then requests `probe.run` while tools are advertised, and answers in text once the tools
    /// disappear (the final no-tools round) or a tool result is present.
    struct ScriptedProvider {
        fail_first: AtomicU32,
    }

    impl ScriptedProvider {
        fn new(fail_first: u32) -> Self {
            Self {
                fail_first: AtomicU32::new(fail_first),
            }
        }
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        fn name(&self) -> &'static str {
            "scripted"
        }

        async fn chat(&self, _req: ChatRequest) -> Result<String, ProviderError> {
            Ok(String::new())
        }

        async fn stream(
            &self,
            req: ChatRequest,
        ) -> Result<BoxStream<'static, Result<StreamChunk, ProviderError>>, ProviderError> {
            if self
                .fail_first
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
            {
                return Err(ProviderError::RateLimited);
            }
            let chunks: Vec<Result<StreamChunk, ProviderError>> = if req.tools.is_empty() {
                vec![
                    Ok(StreamChunk::TextDelta("final answer".into())),
                    Ok(StreamChunk::Done {
                        stop_reason: Some("end_turn".into()),
                        usage: Some(TokenUsage {
                            input_tokens: 3,
                            output_tokens: 5,
                        }),
                    }),
                ]
            } else {
                vec![
                    Ok(StreamChunk::ToolUse {
                        id: "t1".into(),
                        name: "probe.run".into(),
                        input: json!({}),
                    }),
                    Ok(StreamChunk::Done {
                        stop_reason: Some("tool_use".into()),
                        usage: None,
                    }),
                ]
            };
            Ok(stream::iter(chunks).boxed())
        }

        async fn embed(&self, _input: Vec<String>) -> Result<Vec<Vec<f32>>, ProviderError> {
            Ok(vec![])
        }
    }

    fn probe_agent(store: &Store, provider: Arc<dyn Provider>, limits: RunLimits) -> AgentService {
        let calls = Arc::new(Mutex::new(Vec::new()));
        AgentService::new(store.clone(), provider, "scripted")
            .with_executor(Arc::new(FakeExec { delay: None, calls }))
            .with_limits(limits)
    }

    #[tokio::test]
    async fn retryable_provider_failure_is_retried() {
        let store = Store::open_in_memory().unwrap();
        let session = store.create_session(None, None).unwrap();
        // Fails once, then succeeds — with 2 retries allowed the turn completes.
        let agent = probe_agent(
            &store,
            Arc::new(ScriptedProvider::new(1)),
            RunLimits {
                max_tool_iterations: 3,
                provider_retries: 2,
                ..RunLimits::default()
            },
        );
        let events: Vec<AgentEvent> = agent.run_turn(&session.id, "go").await.collect().await;
        assert!(
            matches!(events.last(), Some(AgentEvent::Complete { .. })),
            "expected completion, got {events:?}"
        );
    }

    #[tokio::test]
    async fn turn_appends_durable_events() {
        let store = Store::open_in_memory().unwrap();
        let session = store.create_session(None, None).unwrap();
        let agent = probe_agent(
            &store,
            Arc::new(ScriptedProvider::new(0)),
            RunLimits {
                max_tool_iterations: 2,
                ..RunLimits::default()
            },
        );
        let _: Vec<AgentEvent> = agent.run_turn(&session.id, "go").await.collect().await;

        let kinds: Vec<String> = store
            .list_events(&session.id)
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert!(kinds.contains(&"tool_call".to_string()), "{kinds:?}");
        assert!(kinds.contains(&"tool_result".to_string()), "{kinds:?}");
        assert_eq!(kinds.last().map(String::as_str), Some("complete"));
    }

    #[tokio::test]
    async fn non_retryable_budget_exhaustion_errors() {
        let store = Store::open_in_memory().unwrap();
        let session = store.create_session(None, None).unwrap();
        // Fails 3 times but only 1 retry allowed → the turn errors.
        let agent = probe_agent(
            &store,
            Arc::new(ScriptedProvider::new(3)),
            RunLimits {
                provider_retries: 1,
                ..RunLimits::default()
            },
        );
        let events: Vec<AgentEvent> = agent.run_turn(&session.id, "go").await.collect().await;
        assert!(matches!(events.last(), Some(AgentEvent::Error(_))));
    }

    #[tokio::test]
    async fn iteration_cap_ends_with_text_answer() {
        let store = Store::open_in_memory().unwrap();
        let session = store.create_session(None, None).unwrap();
        // The provider requests a tool every round it has tools; cap at 2 rounds → round 2 runs
        // without tools and must produce the final text answer (a Complete, not an Error).
        let agent = probe_agent(
            &store,
            Arc::new(ScriptedProvider::new(0)),
            RunLimits {
                max_tool_iterations: 2,
                ..RunLimits::default()
            },
        );
        let events: Vec<AgentEvent> = agent.run_turn(&session.id, "go").await.collect().await;
        assert!(
            matches!(events.last(), Some(AgentEvent::Complete { .. })),
            "expected graceful completion, got {events:?}"
        );
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::Delta(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "final answer");
        // Usage from the final round was recorded on the assistant message.
        let msgs = store.list_messages(&session.id).unwrap();
        let last = msgs.last().unwrap();
        assert_eq!(last.token_usage, Some(8));
    }

    #[tokio::test]
    async fn slow_tool_times_out_as_error_result() {
        let store = Store::open_in_memory().unwrap();
        let session = store.create_session(None, None).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let agent = AgentService::new(store.clone(), Arc::new(ScriptedProvider::new(0)), "s")
            .with_executor(Arc::new(FakeExec {
                delay: Some(Duration::from_millis(200)),
                calls: calls.clone(),
            }))
            .with_limits(RunLimits {
                tool_timeout: Duration::from_millis(20),
                max_tool_iterations: 2,
                ..RunLimits::default()
            });
        let events: Vec<AgentEvent> = agent.run_turn(&session.id, "go").await.collect().await;
        let timed_out = events.iter().any(|e| {
            matches!(e, AgentEvent::ToolResult { summary, is_error: true, .. }
                if summary.contains("timed out"))
        });
        assert!(timed_out, "expected a timeout tool result: {events:?}");
        // The turn still completes via the final no-tools round.
        assert!(matches!(events.last(), Some(AgentEvent::Complete { .. })));
    }

    #[test]
    fn trim_drops_oldest_and_marks_truncation() {
        let msg = |role: Role, text: &str| ChatMessage {
            role,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        };
        let msgs = vec![
            msg(Role::User, &"a".repeat(100)),
            msg(Role::Assistant, &"b".repeat(100)),
            msg(Role::User, &"c".repeat(100)),
        ];
        let trimmed = trim_to_budget(msgs, 150);
        // Oldest dropped; a truncation note leads; the newest survives.
        assert!(matches!(
            &trimmed[0].content[0],
            ContentBlock::Text { text } if text.contains("truncated")
        ));
        assert!(matches!(
            &trimmed.last().unwrap().content[0],
            ContentBlock::Text { text } if text.starts_with('c')
        ));
    }

    #[test]
    fn trim_never_leads_with_orphan_tool_result() {
        let msgs = vec![
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "x".repeat(100),
                }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "r".repeat(50),
                    is_error: false,
                }],
            },
            ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "y".repeat(60),
                }],
            },
        ];
        let trimmed = trim_to_budget(msgs, 120);
        // The orphaned tool_result right after the dropped prefix is dropped too.
        assert!(!trimmed
            .iter()
            .flat_map(|m| m.content.iter())
            .any(|b| matches!(b, ContentBlock::ToolResult { .. })));
    }

    #[test]
    fn tool_results_are_capped() {
        let capped = cap_tool_result(&"z".repeat(50), 10);
        assert!(capped.starts_with(&"z".repeat(10)));
        assert!(capped.contains("truncated 40 chars"));
        assert_eq!(cap_tool_result("short", 10), "short");
    }

    #[test]
    fn summarize_previews_written_content() {
        let s = summarize(
            "files.create",
            &json!({"path": "/tmp/a.txt", "content": "hello world"}),
        );
        assert!(s.contains("files.create /tmp/a.txt"));
        assert!(s.contains("hello world"));
    }

    #[test]
    fn user_rows_are_never_block_parsed() {
        // A user message that happens to be valid block JSON must ride as plain text.
        let store = Store::open_in_memory().unwrap();
        let session = store.create_session(None, None).unwrap();
        let tricky = r#"[{"type":"text","text":"not blocks"}]"#;
        store.insert_message(&session.id, "user", tricky).unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let run = TurnRun {
            store: store.clone(),
            provider: Arc::new(MockProvider::new()),
            model: "mock".into(),
            extensions: Arc::new(ExtensionManager::empty()),
            grants: Arc::new(GrantSet::empty()),
            approval: None,
            enabled_tools: None,
            blank_slate: false,
            project_dir: None,
            persona: None,
            author: None,
            participants: Vec::new(),
            limits: RunLimits::default(),
            session_id: session.id.clone(),
            tx,
        };
        let msgs = run.load_transcript().unwrap();
        assert_eq!(
            msgs[0].content,
            vec![ContentBlock::Text {
                text: tricky.to_string()
            }]
        );
    }
}
