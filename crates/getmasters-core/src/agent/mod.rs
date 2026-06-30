//! The shared agent loop.
//!
//! `run_turn` is the single core path behind both the daemon's WebSocket and the CLI
//! (ADR-0001). Phase 1a makes it a **multi-turn tool loop**: stream the model, and whenever
//! it requests a tool, gate the call through Permission & Audit, dispatch it via the
//! Extension Manager, feed the result back, and re-call the model — until it stops or a
//! bound is hit. Every side-effecting call passes the gate before execution (docs/06).

use std::collections::HashSet;
use std::sync::Arc;

use futures::stream::{Stream, StreamExt};

use crate::extensions::ExtensionManager;
use crate::permission::{
    ApprovalRegistry, ApprovalRequest, Approver, Authorized, AutoApprover, ChannelApprover,
    GrantSet, PermissionGate,
};
use crate::prompt::PromptAssembler;
use crate::provider::{ChatMessage, ChatRequest, ContentBlock, Provider, Role, StreamChunk};
use crate::store::Store;

/// Safety bound on tool-calling rounds within a single turn.
const MAX_TOOL_ITERATIONS: usize = 8;

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

/// Orchestrates the store, a provider, tools, and the permission gate to run conversational turns.
#[derive(Clone)]
pub struct AgentService {
    store: Store,
    provider: Arc<dyn Provider>,
    model: String,
    extensions: Arc<ExtensionManager>,
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
        self.extensions.call_tool(name, input).await
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
        let session_id = session_id.to_string();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();

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
                session_id,
                tx: tx.clone(),
            };
            if let Err(e) = run.execute(user_text.as_deref()).await {
                let _ = tx.send(AgentEvent::Error(e));
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

/// Per-run state for the tool loop.
struct TurnRun {
    store: Store,
    provider: Arc<dyn Provider>,
    model: String,
    extensions: Arc<ExtensionManager>,
    grants: Arc<GrantSet>,
    approval: Option<Arc<ApprovalRegistry>>,
    enabled_tools: Option<Arc<HashSet<String>>>,
    blank_slate: bool,
    project_dir: Option<std::path::PathBuf>,
    persona: Option<String>,
    author: Option<String>,
    session_id: String,
    tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
}

impl TurnRun {
    fn emit(&self, ev: AgentEvent) -> bool {
        self.tx.send(ev).is_ok()
    }

    /// Build the permission gate for this run (ChannelApprover when an approval registry is
    /// present, else AutoApprover).
    fn gate(&self) -> PermissionGate {
        let approver: Arc<dyn Approver> = match &self.approval {
            Some(registry) => {
                let tx = self.tx.clone();
                let emit = Arc::new(move |req: ApprovalRequest| {
                    let _ = tx.send(AgentEvent::ApprovalRequest(req));
                });
                Arc::new(ChannelApprover::new(registry.clone(), emit))
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

    async fn execute(&self, user_text: Option<&str>) -> Result<(), String> {
        // 1. Persist the user turn — unless this is an answer turn over an existing transcript
        //    (Phase 4c group chat: the user message is already posted).
        if let Some(user_text) = user_text {
            self.store
                .insert_message(&self.session_id, "user", user_text)
                .map_err(|e| format!("persist user message: {e}"))?;
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
            };
            PromptAssembler::assemble(&ctx)
        };

        let gate = self.gate();

        for _ in 0..MAX_TOOL_ITERATIONS {
            let messages = self.load_transcript()?;
            let mut req = ChatRequest::new(self.model.clone(), messages);
            req.system = system.clone();
            req.tools = tools.clone();

            let mut stream = self
                .provider
                .stream(req)
                .await
                .map_err(|e| format!("provider: {e}"))?;

            let mut text = String::new();
            let mut pending: Vec<(String, String, serde_json::Value)> = Vec::new();
            while let Some(chunk) = stream.next().await {
                match chunk.map_err(|e| format!("provider stream: {e}"))? {
                    StreamChunk::TextDelta(t) => {
                        text.push_str(&t);
                        if !self.emit(AgentEvent::Delta(t)) {
                            return Ok(()); // receiver gone (disconnect / Stop)
                        }
                    }
                    StreamChunk::ToolUse { id, name, input } => {
                        self.emit(AgentEvent::ToolCallStarted {
                            id: id.clone(),
                            name: name.clone(),
                            summary: summarize(&name, &input),
                        });
                        pending.push((id, name, input));
                    }
                    StreamChunk::Done { .. } => break,
                }
            }

            // Persist the assistant turn. A text-only turn is stored as plain text (readable
            // transcripts + a human-friendly HTTP `content`); a turn with tool calls is stored
            // as JSON content blocks so the tool_use survives reload. `parse_blocks` reads both.
            let content = if pending.is_empty() {
                text.clone()
            } else {
                let mut blocks: Vec<ContentBlock> = Vec::new();
                if !text.is_empty() {
                    blocks.push(ContentBlock::Text { text: text.clone() });
                }
                for (id, name, input) in &pending {
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
                .map_err(|e| format!("persist assistant message: {e}"))?;

            if pending.is_empty() {
                self.emit(AgentEvent::Complete {
                    message_id: assistant_msg.id,
                });
                return Ok(());
            }

            // Gate + dispatch each requested tool, collecting results.
            let mut results: Vec<ContentBlock> = Vec::new();
            for (id, name, input) in pending {
                let (summary, is_error) = match gate.authorize(&name, &input).await {
                    Authorized::Denied(reason) => (format!("permission denied: {reason}"), true),
                    Authorized::Allowed => {
                        // Capture the pre-image so this op can be reverted (docs/06).
                        let pending_rev = crate::revision::capture(&self.grants, &name, &input);
                        match self.extensions.call_tool(&name, &input).await {
                            Ok((out, err)) => {
                                if !err {
                                    if let Some(rev) = pending_rev {
                                        crate::revision::commit(
                                            &self.store,
                                            Some(&self.session_id),
                                            rev,
                                        );
                                    }
                                }
                                (out, err)
                            }
                            Err(e) => (format!("tool error: {e}"), true),
                        }
                    }
                };
                self.emit(AgentEvent::ToolResult {
                    id: id.clone(),
                    summary: truncate(&summary, 200),
                    is_error,
                });
                results.push(ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: summary,
                    is_error,
                });
            }
            self.persist("tool", &blocks_json(&results))
                .map_err(|e| format!("persist tool result: {e}"))?;
            // Loop: re-call the provider with the appended results.
        }

        Err(format!(
            "tool loop exceeded {MAX_TOOL_ITERATIONS} iterations"
        ))
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

    /// Load the session transcript as provider messages, parsing JSON content blocks (with a
    /// fallback that wraps legacy plain-text rows as a single Text block).
    ///
    /// In group chat (`self.author` set), the transcript is rendered from that master's perspective:
    /// its own past turns are `Assistant`; every other author (the user + other masters) rides as
    /// `User` with a `[author]` speaker label so the model can tell participants apart (ADR-0012's
    /// "shared read context"). In ordinary chat the rows are unlabelled.
    fn load_transcript(&self) -> Result<Vec<ChatMessage>, String> {
        let rows = self
            .store
            .list_messages(&self.session_id)
            .map_err(|e| format!("load transcript: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|m| match &self.author {
                Some(me) if &m.author == me => ChatMessage {
                    role: Role::Assistant,
                    content: parse_blocks(&m.content),
                },
                Some(_) => ChatMessage {
                    // Other participants (user + other masters) ride as labelled user turns.
                    role: Role::User,
                    content: label_speaker(&m.author, parse_blocks(&m.content)),
                },
                None => {
                    let role = match m.role.as_str() {
                        "assistant" => Role::Assistant,
                        _ => Role::User, // "user" and "tool" both ride as user-role to the provider
                    };
                    ChatMessage {
                        role,
                        content: parse_blocks(&m.content),
                    }
                }
            })
            .collect())
    }
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

fn summarize(name: &str, input: &serde_json::Value) -> String {
    match input
        .get("path")
        .or_else(|| input.get("to"))
        .and_then(|v| v.as_str())
    {
        Some(p) => format!("{name} {p}"),
        None => name.to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}…")
    }
}

/// Adapt an `mpsc::UnboundedReceiver` into a `Stream` without pulling in `tokio-stream`.
fn tokio_stream_from(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
) -> impl Stream<Item = AgentEvent> + Send + 'static {
    futures::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use super::*;
    use crate::provider::MockProvider;

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
}
