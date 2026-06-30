//! The `Provider` trait — Masters's uniform abstraction over LLM backends (ADR-0003).
//!
//! All model access goes through this trait so the agent loop is provider-agnostic and
//! ADR-0013's per-master model selection can later dispatch each master to its own provider
//! without touching agent code. Production ships two implementations — [`anthropic`] and
//! [`openai`] (both real). A deterministic offline `MockProvider` lives in [`mock`] behind the
//! `testing` feature as the headless test fake; it is never compiled into the daemon.
//!
//! Messages are **content blocks** (text / tool-use / tool-result) so the same trait carries
//! tool-calling across providers. Each provider **accumulates** partial tool-call JSON
//! internally and emits a single completed [`StreamChunk::ToolUse`], keeping the agent loop
//! identical regardless of backend.

pub mod anthropic;
pub mod catalog;
pub mod factory;
#[cfg(feature = "testing")]
pub mod mock;
pub mod openai;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde_json::Value;

pub use anthropic::AnthropicProvider;
pub use catalog::{CatalogEntry, ProviderTransport, CATALOG};
pub use factory::{build_provider, resolve_provider};
#[cfg(feature = "testing")]
pub use mock::MockProvider;
pub use openai::OpenAiProvider;
// Re-exported at module level (defined below) for the daemon's unconfigured-start fallback.

/// Conversation role on the wire to the provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// One block of structured message content (provider-agnostic).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text.
    Text { text: String },
    /// An assistant request to call a tool, with fully-accumulated input.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// The observation returned for a prior `ToolUse`.
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

/// A single message in a chat request.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl ChatMessage {
    /// A user message with a single text block.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// An assistant message with a single text block.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// A user-role message carrying one tool result (the convention both providers accept).
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                is_error,
            }],
        }
    }

    /// Concatenated text of all `Text` blocks (persistence/mock convenience).
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The tool-use blocks in this message, if any.
    pub fn tool_uses(&self) -> impl Iterator<Item = (&str, &str, &Value)> {
        self.content.iter().filter_map(|b| match b {
            ContentBlock::ToolUse { id, name, input } => Some((id.as_str(), name.as_str(), input)),
            _ => None,
        })
    }

    /// Whether any block is a tool result (used by the mock to pick its phase).
    pub fn has_tool_result(&self) -> bool {
        self.content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
    }
}

/// A tool advertised to the model: name, description, and a JSON Schema for its arguments.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// A provider-agnostic chat request.
#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub model: String,
    /// Optional system prompt (assembled by the agent loop).
    pub system: Option<String>,
    /// Ordered turns (excluding the system prompt).
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    /// Tools the model may call; empty = no tools (the Phase 0 text-only path).
    pub tools: Vec<ToolSchema>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: model.into(),
            system: None,
            messages,
            max_tokens: 4096,
            tools: Vec::new(),
        }
    }
}

/// A streamed chunk from [`Provider::stream`].
#[derive(Clone, Debug, PartialEq)]
pub enum StreamChunk {
    /// Incremental assistant text.
    TextDelta(String),
    /// A completed tool call (the provider has fully accumulated the arguments).
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// The stream finished; `stop_reason` is e.g. `"tool_use"`/`"tool_calls"`/`"end_turn"`.
    Done { stop_reason: Option<String> },
}

/// Errors a provider can return.
#[derive(thiserror::Error, Debug)]
pub enum ProviderError {
    #[error("http error: {0}")]
    Http(String),
    #[error("authentication failed")]
    Auth,
    #[error("decode error: {0}")]
    Decode(String),
    #[error("model refused the request")]
    Refusal,
    #[error("provider not configured: {0}")]
    NotConfigured(String),
}

/// A placeholder provider used when the daemon starts **without** a usable LLM provider so the
/// HTTP surface (health, settings, onboarding) comes up and the user can configure a real provider
/// in the UI. Every model call fails clearly with [`ProviderError::NotConfigured`] — there is no
/// silent offline inference (ADR-0008); `/health` reports `configured = false` so the desktop
/// auto-opens the setup wizard.
pub struct UnconfiguredProvider;

#[async_trait]
impl Provider for UnconfiguredProvider {
    fn name(&self) -> &'static str {
        "unconfigured"
    }

    async fn chat(&self, _req: ChatRequest) -> Result<String, ProviderError> {
        Err(ProviderError::NotConfigured(
            "no LLM provider configured — set an API key in Settings".into(),
        ))
    }

    async fn stream(
        &self,
        _req: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamChunk, ProviderError>>, ProviderError> {
        Err(ProviderError::NotConfigured(
            "no LLM provider configured — set an API key in Settings".into(),
        ))
    }

    async fn embed(&self, _input: Vec<String>) -> Result<Vec<Vec<f32>>, ProviderError> {
        Err(ProviderError::NotConfigured(
            "no LLM provider configured — set an API key in Settings".into(),
        ))
    }
}

/// A pluggable LLM backend. Implementations are `Send + Sync` and shared behind `Arc`.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable provider name (`"anthropic"`, `"openai"`; `"mock"` only under the `testing`
    /// feature), surfaced in `/health` and the UI privacy boundary (docs/06 §5).
    fn name(&self) -> &'static str;

    /// Non-streaming completion: returns the full assistant text.
    async fn chat(&self, req: ChatRequest) -> Result<String, ProviderError>;

    /// Streaming completion: a boxed stream of chunks forwarded to the WebSocket as they arrive.
    async fn stream(
        &self,
        req: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamChunk, ProviderError>>, ProviderError>;

    /// Embeddings for RAG (ADR-0004). Not on the Phase 1 exit-criterion path.
    async fn embed(&self, input: Vec<String>) -> Result<Vec<Vec<f32>>, ProviderError>;
}
