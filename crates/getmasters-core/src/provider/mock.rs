//! Deterministic, offline provider.
//!
//! Two behaviors, both keyless and network-free:
//! - **Echo**: a plain prompt streams back `"echo: <text>"`.
//! - **Tool trigger**: a prompt containing a sentinel `[[tool:<name>|k=v|k=v]]` drives a
//!   deterministic two-phase tool exchange so the full gated tool loop (and the WS approval
//!   round-trip) is testable with no API key:
//!     1. no `ToolResult` in the transcript yet → emit one `ToolUse` then `Done{tool_use}`.
//!     2. a `ToolResult` is present → emit a final text summary then `Done{end_turn}`.

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use serde_json::Value;

use super::{ChatMessage, ChatRequest, ContentBlock, Provider, ProviderError, Role, StreamChunk};

/// Offline mock provider.
#[derive(Clone, Default)]
pub struct MockProvider;

impl MockProvider {
    pub fn new() -> Self {
        Self
    }

    /// The latest user message's concatenated text.
    fn last_user_text(req: &ChatRequest) -> String {
        req.messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User && !m.has_tool_result())
            .map(|m| m.text())
            .unwrap_or_default()
    }

    /// True when the **most recent** message carries a tool result (→ second phase). Checking
    /// only the last message means prior turns' tool results don't pollute phase detection in a
    /// multi-turn session.
    fn has_tool_result(req: &ChatRequest) -> bool {
        req.messages
            .last()
            .is_some_and(ChatMessage::has_tool_result)
    }

    /// The last tool-result content, for the phase-2 summary.
    fn last_tool_result(req: &ChatRequest) -> Option<String> {
        req.messages.last().and_then(|m| {
            m.content.iter().rev().find_map(|b| match b {
                ContentBlock::ToolResult { content, .. } => Some(content.clone()),
                _ => None,
            })
        })
    }

    /// Parse a `[[tool:<name>|k=v|...]]` sentinel into a tool name + JSON args.
    fn parse_trigger(text: &str) -> Option<(String, Value)> {
        let start = text.find("[[tool:")? + "[[tool:".len();
        let end = text[start..].find("]]")? + start;
        let inner = &text[start..end];
        let mut parts = inner.split('|');
        let name = parts.next()?.trim().to_string();
        let mut obj = serde_json::Map::new();
        for kv in parts {
            if let Some((k, v)) = kv.split_once('=') {
                obj.insert(k.trim().to_string(), Value::String(v.to_string()));
            }
        }
        Some((name, Value::Object(obj)))
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    async fn chat(&self, req: ChatRequest) -> Result<String, ProviderError> {
        Ok(format!("echo: {}", Self::last_user_text(&req)))
    }

    async fn stream(
        &self,
        req: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamChunk, ProviderError>>, ProviderError> {
        // Phase 2: a tool result came back → summarize and end.
        if Self::has_tool_result(&req) {
            let summary = Self::last_tool_result(&req).unwrap_or_default();
            let chunks = vec![
                Ok(StreamChunk::TextDelta(format!("done: {summary}"))),
                Ok(StreamChunk::Done {
                    stop_reason: Some("end_turn".into()),
                    usage: None,
                }),
            ];
            return Ok(stream::iter(chunks).boxed());
        }

        let text = Self::last_user_text(&req);

        // Phase 1: a tool trigger → emit one ToolUse.
        if let Some((name, input)) = Self::parse_trigger(&text) {
            let chunks = vec![
                Ok(StreamChunk::ToolUse {
                    id: "call_1".into(),
                    name,
                    input,
                }),
                Ok(StreamChunk::Done {
                    stop_reason: Some("tool_use".into()),
                    usage: None,
                }),
            ];
            return Ok(stream::iter(chunks).boxed());
        }

        // Default: echo, word by word so the UI visibly streams.
        let reply = format!("echo: {text}");
        let mut chunks: Vec<Result<StreamChunk, ProviderError>> = reply
            .split_inclusive(' ')
            .map(|w| Ok(StreamChunk::TextDelta(w.to_string())))
            .collect();
        chunks.push(Ok(StreamChunk::Done {
            stop_reason: Some("end_turn".into()),
            usage: None,
        }));
        Ok(stream::iter(chunks).boxed())
    }

    async fn embed(&self, input: Vec<String>) -> Result<Vec<Vec<f32>>, ProviderError> {
        Ok(input
            .iter()
            .map(|s| {
                let seed = s.len() as f32;
                (0..8).map(|i| (seed + i as f32) * 0.001).collect()
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use serde_json::json;

    fn req(text: &str) -> ChatRequest {
        ChatRequest::new("mock", vec![ChatMessage::user(text)])
    }

    #[tokio::test]
    async fn chat_echoes_last_user() {
        let p = MockProvider::new();
        assert_eq!(p.chat(req("hello")).await.unwrap(), "echo: hello");
    }

    #[tokio::test]
    async fn stream_echoes_plain_prompt() {
        let p = MockProvider::new();
        let collected: Vec<_> = p.stream(req("hi there")).await.unwrap().collect().await;
        let text: String = collected
            .iter()
            .filter_map(|c| match c {
                Ok(StreamChunk::TextDelta(t)) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "echo: hi there");
        assert!(matches!(
            collected.last(),
            Some(Ok(StreamChunk::Done { .. }))
        ));
    }

    #[tokio::test]
    async fn trigger_emits_tool_use() {
        let p = MockProvider::new();
        let r = req("please [[tool:files.create|path=a.txt|content=hello world]] now");
        let collected: Vec<_> = p.stream(r).await.unwrap().collect().await;
        match &collected[0] {
            Ok(StreamChunk::ToolUse { name, input, .. }) => {
                assert_eq!(name, "files.create");
                assert_eq!(input["path"], "a.txt");
                assert_eq!(input["content"], "hello world");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn second_phase_summarizes_tool_result() {
        let p = MockProvider::new();
        let mut r = req("[[tool:files.create|path=a.txt|content=x]]");
        r.messages.push(ChatMessage {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call_1".into(),
                name: "files.create".into(),
                input: json!({"path":"a.txt"}),
            }],
        });
        r.messages
            .push(ChatMessage::tool_result("call_1", "created a.txt", false));
        let collected: Vec<_> = p.stream(r).await.unwrap().collect().await;
        let text: String = collected
            .iter()
            .filter_map(|c| match c {
                Ok(StreamChunk::TextDelta(t)) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "done: created a.txt");
    }
}
