//! Real Anthropic/Claude provider over raw HTTPS (there is no official Rust SDK).
//!
//! Wire reference: `POST https://api.anthropic.com/v1/messages` with headers
//! `x-api-key` + `anthropic-version: 2023-06-01`. For `claude-opus-4-8` the body stays
//! minimal — `model`, `max_tokens`, `system?`, `messages`, `tools?`, `stream` — with **no**
//! `temperature`/`top_p`/`thinking` (those 400 on 4.8). Streaming is server-sent events;
//! a small state machine accumulates `tool_use` blocks and emits one [`StreamChunk::ToolUse`].

use std::collections::HashMap;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::stream::{BoxStream, StreamExt};
use serde_json::{json, Value};

use super::{ChatRequest, ContentBlock, Provider, ProviderError, Role, StreamChunk};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Claude provider holding the API key and an HTTP client.
#[derive(Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
        }
    }

    /// Serialize one message's content blocks into Anthropic's block array.
    fn blocks_json(blocks: &[ContentBlock]) -> Value {
        let arr: Vec<Value> = blocks
            .iter()
            .map(|b| match b {
                ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
                ContentBlock::ToolUse { id, name, input } => {
                    json!({ "type": "tool_use", "id": id, "name": name, "input": input })
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": content,
                    "is_error": is_error,
                }),
            })
            .collect();
        Value::Array(arr)
    }

    /// Build the JSON request body shared by `chat` and `stream`.
    fn body(req: &ChatRequest, stream: bool) -> Value {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::Assistant => "assistant",
                    _ => "user",
                };
                json!({ "role": role, "content": Self::blocks_json(&m.content) })
            })
            .collect();

        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": messages,
            "stream": stream,
        });
        // Prompt caching: the system prompt and tool list are the large, stable prefix a
        // multi-round tool loop re-sends verbatim every iteration — mark them ephemeral-cached.
        if let Some(system) = &req.system {
            body["system"] = json!([{
                "type": "text",
                "text": system,
                "cache_control": { "type": "ephemeral" },
            }]);
        }
        if !req.tools.is_empty() {
            let mut tools: Vec<Value> = req
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.input_schema,
                    })
                })
                .collect();
            if let Some(last) = tools.last_mut() {
                last["cache_control"] = json!({ "type": "ephemeral" });
            }
            body["tools"] = Value::Array(tools);
        }
        body
    }

    fn request(&self, body: &Value) -> reqwest::RequestBuilder {
        self.client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(body)
    }

    fn map_status(status: reqwest::StatusCode, body: &str) -> ProviderError {
        ProviderError::from_status(status.as_u16(), body)
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    async fn chat(&self, req: ChatRequest) -> Result<String, ProviderError> {
        let body = Self::body(&req, false);
        let resp = self
            .request(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(Self::map_status(status, &text));
        }

        let v: Value =
            serde_json::from_str(&text).map_err(|e| ProviderError::Decode(e.to_string()))?;
        if v.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
            return Err(ProviderError::Refusal);
        }
        let out = v
            .get("content")
            .and_then(Value::as_array)
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect::<String>()
            })
            .unwrap_or_default();
        Ok(out)
    }

    async fn stream(
        &self,
        req: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamChunk, ProviderError>>, ProviderError> {
        let body = Self::body(&req, true);
        let resp = self
            .request(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(Self::map_status(status, &text));
        }

        // Stateful SSE translation: `tool_use` blocks accumulate `input_json_delta` fragments
        // by index and flush a single `ToolUse` on `content_block_stop`.
        let events = resp.bytes_stream().eventsource();
        let mapped = events
            .scan(SseState::default(), |state, ev| {
                let out = match ev {
                    Err(e) => Some(Err(ProviderError::Decode(e.to_string()))),
                    Ok(event) => state.handle(&event.event, &event.data).map(Ok),
                };
                // `scan` yields the (possibly-None) item and continues.
                futures::future::ready(Some(out))
            })
            .filter_map(|opt| async move { opt });
        Ok(mapped.boxed())
    }

    async fn embed(&self, _input: Vec<String>) -> Result<Vec<Vec<f32>>, ProviderError> {
        Err(ProviderError::NotConfigured(
            "anthropic embeddings not configured (Phase 2)".into(),
        ))
    }
}

/// Per-index accumulation buffer for an in-flight `tool_use` block.
struct ToolBuf {
    id: String,
    name: String,
    args: String,
}

/// Streaming state across SSE events.
#[derive(Default)]
struct SseState {
    tools: HashMap<i64, ToolBuf>,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

impl SseState {
    /// Translate one SSE event into at most one [`StreamChunk`], updating accumulation state.
    fn handle(&mut self, event: &str, data: &str) -> Option<StreamChunk> {
        let v: Value = serde_json::from_str(data).ok()?;
        match event {
            "message_start" => {
                // `message.usage.input_tokens` arrives once, up front.
                self.input_tokens = v
                    .get("message")
                    .and_then(|m| m.get("usage"))
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(Value::as_u64)
                    .map(|n| n as u32);
                None
            }
            "content_block_start" => {
                let index = v.get("index").and_then(Value::as_i64)?;
                let block = v.get("content_block")?;
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    self.tools.insert(
                        index,
                        ToolBuf {
                            id: block
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            name: block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            args: String::new(),
                        },
                    );
                }
                None
            }
            "content_block_delta" => {
                let delta = v.get("delta")?;
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let text = delta
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        Some(StreamChunk::TextDelta(text.to_string()))
                    }
                    Some("input_json_delta") => {
                        let index = v.get("index").and_then(Value::as_i64)?;
                        if let Some(buf) = self.tools.get_mut(&index) {
                            buf.args.push_str(
                                delta
                                    .get("partial_json")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default(),
                            );
                        }
                        None
                    }
                    _ => None,
                }
            }
            "content_block_stop" => {
                let index = v.get("index").and_then(Value::as_i64)?;
                let buf = self.tools.remove(&index)?;
                let input: Value = if buf.args.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&buf.args).unwrap_or_else(|_| json!({}))
                };
                Some(StreamChunk::ToolUse {
                    id: buf.id,
                    name: buf.name,
                    input,
                })
            }
            "message_delta" => {
                let stop_reason = v
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                // Cumulative output tokens ride on the top-level `usage` of message_delta.
                self.output_tokens = v
                    .get("usage")
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(Value::as_u64)
                    .map(|n| n as u32)
                    .or(self.output_tokens);
                let usage = match (self.input_tokens, self.output_tokens) {
                    (None, None) => None,
                    (i, o) => Some(crate::provider::TokenUsage {
                        input_tokens: i.unwrap_or(0),
                        output_tokens: o.unwrap_or(0),
                    }),
                };
                Some(StreamChunk::Done { stop_reason, usage })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ChatMessage;

    #[test]
    fn body_is_minimal_and_omits_banned_fields() {
        let mut req = ChatRequest::new("claude-opus-4-8", vec![ChatMessage::user("hi")]);
        req.system = Some("be terse".into());
        let body = AnthropicProvider::body(&req, true);
        assert_eq!(body["model"], "claude-opus-4-8");
        assert_eq!(body["stream"], true);
        // System rides as a cache-marked block array (prompt caching).
        assert_eq!(body["system"][0]["text"], "be terse");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert!(body.get("tools").is_none());
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn body_includes_tools_when_present() {
        use crate::provider::ToolSchema;
        let mut req = ChatRequest::new("claude-opus-4-8", vec![ChatMessage::user("hi")]);
        req.tools = vec![ToolSchema {
            name: "files.read".into(),
            description: "read a file".into(),
            input_schema: json!({"type":"object"}),
        }];
        let body = AnthropicProvider::body(&req, true);
        assert_eq!(body["tools"][0]["name"], "files.read");
        // The last tool carries the cache marker (stable-prefix caching).
        assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn sse_message_delta_carries_usage() {
        let mut s = SseState::default();
        assert_eq!(
            s.handle(
                "message_start",
                r#"{"type":"message_start","message":{"usage":{"input_tokens":11}}}"#
            ),
            None
        );
        let done = s.handle(
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}}"#,
        );
        match done {
            Some(StreamChunk::Done { usage, .. }) => {
                let u = usage.expect("usage");
                assert_eq!(u.input_tokens, 11);
                assert_eq!(u.output_tokens, 7);
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn sse_text_delta() {
        let mut s = SseState::default();
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        assert_eq!(
            s.handle("content_block_delta", data),
            Some(StreamChunk::TextDelta("Hello".into()))
        );
    }

    #[test]
    fn sse_accumulates_tool_use() {
        let mut s = SseState::default();
        assert_eq!(
            s.handle(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"files.create"}}"#
            ),
            None
        );
        assert_eq!(
            s.handle(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#
            ),
            None
        );
        assert_eq!(
            s.handle(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"a.txt\"}"}}"#
            ),
            None
        );
        let chunk = s.handle(
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        );
        match chunk {
            Some(StreamChunk::ToolUse { id, name, input }) => {
                assert_eq!(id, "t1");
                assert_eq!(name, "files.create");
                assert_eq!(input["path"], "a.txt");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn sse_ignores_ping() {
        let mut s = SseState::default();
        assert_eq!(s.handle("ping", "{}"), None);
    }
}
