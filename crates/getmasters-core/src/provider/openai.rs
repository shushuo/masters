//! OpenAI-compatible provider over raw HTTPS (`/v1/chat/completions`).
//!
//! A single implementation with a configurable `base_url` serves OpenAI and any
//! OpenAI-compatible endpoint (Groq, Together, OpenRouter, a local Ollama). Streaming is
//! SSE `data:` lines (no `event:` type) terminated by `[DONE]`; `tool_calls` deltas are
//! accumulated by index and flushed at `finish_reason` into one [`StreamChunk::ToolUse`].

use std::collections::BTreeMap;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::stream::{self, BoxStream, StreamExt};
use serde_json::{json, Value};

use super::{ChatRequest, ContentBlock, Provider, ProviderError, Role, StreamChunk};

/// An OpenAI-compatible chat provider.
#[derive(Clone)]
pub struct OpenAiProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    /// Model used for `/v1/embeddings` (chat uses `ChatRequest.model`).
    embed_model: String,
}

impl OpenAiProvider {
    /// `base_url` is the API root (default `https://api.openai.com`); the path
    /// `/v1/chat/completions` is appended. `api_key` is omitted for keyless local endpoints.
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key,
            embed_model: "text-embedding-3-small".to_string(),
        }
    }

    pub fn openai(api_key: Option<String>) -> Self {
        Self::new("https://api.openai.com", api_key)
    }

    /// Set the embeddings model (used by the Knowledge embedder).
    pub fn with_embed_model(mut self, model: impl Into<String>) -> Self {
        self.embed_model = model.into();
        self
    }

    fn url(&self) -> String {
        format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        )
    }

    fn embeddings_url(&self) -> String {
        format!("{}/v1/embeddings", self.base_url.trim_end_matches('/'))
    }

    /// Test accessor for the embeddings request body shape.
    #[cfg(test)]
    pub(crate) fn embed_body(&self, input: Vec<String>) -> Value {
        serde_json::json!({ "model": self.embed_model, "input": input })
    }

    fn role_str(role: Role) -> &'static str {
        match role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }

    /// Map our content-block messages into OpenAI's message array (one block-message may
    /// expand to several OpenAI messages — e.g. multiple tool results).
    fn messages_json(req: &ChatRequest) -> Vec<Value> {
        let mut out = Vec::new();
        for m in &req.messages {
            let tool_results: Vec<&ContentBlock> = m
                .content
                .iter()
                .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
                .collect();
            let tool_uses: Vec<&ContentBlock> = m
                .content
                .iter()
                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                .collect();

            if !tool_results.is_empty() {
                for b in tool_results {
                    if let ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } = b
                    {
                        out.push(json!({ "role": "tool", "tool_call_id": tool_use_id, "content": content }));
                    }
                }
            } else if m.role == Role::Assistant && !tool_uses.is_empty() {
                let calls: Vec<Value> = tool_uses
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolUse { id, name, input } => Some(json!({
                            "id": id,
                            "type": "function",
                            "function": { "name": name, "arguments": input.to_string() },
                        })),
                        _ => None,
                    })
                    .collect();
                let text = m.text();
                out.push(json!({
                    "role": "assistant",
                    "content": if text.is_empty() { Value::Null } else { Value::String(text) },
                    "tool_calls": calls,
                }));
            } else {
                out.push(json!({ "role": Self::role_str(m.role), "content": m.text() }));
            }
        }
        out
    }

    fn body(req: &ChatRequest) -> Value {
        let mut messages = Vec::new();
        if let Some(system) = &req.system {
            messages.push(json!({ "role": "system", "content": system }));
        }
        messages.extend(Self::messages_json(req));

        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": messages,
            "stream": true,
        });
        if !req.tools.is_empty() {
            body["tools"] = Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.input_schema,
                            },
                        })
                    })
                    .collect(),
            );
        }
        body
    }

    fn request(&self, body: &Value) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .post(self.url())
            .header("content-type", "application/json")
            .json(body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        req
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn chat(&self, req: ChatRequest) -> Result<String, ProviderError> {
        // Non-streaming convenience: reuse the streaming path and concat text.
        let mut stream = self.stream(req).await?;
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let StreamChunk::TextDelta(t) = chunk? {
                text.push_str(&t);
            }
        }
        Ok(text)
    }

    async fn stream(
        &self,
        req: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamChunk, ProviderError>>, ProviderError> {
        let mut body = Self::body(&req);
        body["stream"] = Value::Bool(true);
        let resp = self
            .request(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::from_status(status.as_u16(), &text));
        }

        let events = resp.bytes_stream().eventsource();
        let mapped = events
            .scan(OaState::default(), |state, ev| {
                let out = match ev {
                    Err(e) => vec![Err(ProviderError::Decode(e.to_string()))],
                    Ok(event) => state.handle(&event.data),
                };
                futures::future::ready(Some(out))
            })
            .flat_map(stream::iter);
        Ok(mapped.boxed())
    }

    async fn embed(&self, input: Vec<String>) -> Result<Vec<Vec<f32>>, ProviderError> {
        let body = serde_json::json!({ "model": self.embed_model, "input": input });
        let mut req = self
            .client
            .post(self.embeddings_url())
            .header("content-type", "application/json")
            .json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(ProviderError::from_status(status.as_u16(), &text));
        }
        let v: Value =
            serde_json::from_str(&text).map_err(|e| ProviderError::Decode(e.to_string()))?;
        // Preserve input order by `index`.
        let mut data: Vec<(usize, Vec<f32>)> = v
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| ProviderError::Decode("missing data[]".into()))?
            .iter()
            .map(|item| {
                let idx = item.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let emb = item
                    .get("embedding")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_f64().map(|f| f as f32))
                            .collect()
                    })
                    .unwrap_or_default();
                (idx, emb)
            })
            .collect();
        data.sort_by_key(|(i, _)| *i);
        Ok(data.into_iter().map(|(_, e)| e).collect())
    }
}

/// Accumulated state for one streaming OpenAI tool call.
struct OaTool {
    id: String,
    name: String,
    args: String,
}

#[derive(Default)]
struct OaState {
    tools: BTreeMap<i64, OaTool>,
    done: bool,
    usage: Option<crate::provider::TokenUsage>,
}

impl OaState {
    /// Flush accumulated tool calls into `ToolUse` chunks (in index order).
    fn flush_tools(&mut self) -> Vec<Result<StreamChunk, ProviderError>> {
        std::mem::take(&mut self.tools)
            .into_values()
            .map(|t| {
                let input: Value = if t.args.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&t.args).unwrap_or_else(|_| json!({}))
                };
                Ok(StreamChunk::ToolUse {
                    id: t.id,
                    name: t.name,
                    input,
                })
            })
            .collect()
    }

    /// Handle one SSE `data:` payload, returning zero or more chunks.
    fn handle(&mut self, data: &str) -> Vec<Result<StreamChunk, ProviderError>> {
        let data = data.trim();
        if data == "[DONE]" {
            if self.done {
                return vec![];
            }
            self.done = true;
            let mut out = self.flush_tools();
            out.push(Ok(StreamChunk::Done {
                stop_reason: None,
                usage: self.usage.take(),
            }));
            return out;
        }

        let v: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => return vec![Err(ProviderError::Decode(e.to_string()))],
        };
        // Some OpenAI-compatible backends emit a usage chunk (often with an empty
        // `choices`) — capture it whenever present so `Done` can carry it.
        if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
            let read = |k: &str| u.get(k).and_then(Value::as_u64).map(|n| n as u32);
            if read("prompt_tokens").is_some() || read("completion_tokens").is_some() {
                self.usage = Some(crate::provider::TokenUsage {
                    input_tokens: read("prompt_tokens").unwrap_or(0),
                    output_tokens: read("completion_tokens").unwrap_or(0),
                });
            }
        }

        let choice = match v.get("choices").and_then(|c| c.get(0)) {
            Some(c) => c,
            None => return vec![],
        };
        let delta = choice.get("delta");
        let mut out = Vec::new();

        if let Some(content) = delta.and_then(|d| d.get("content")).and_then(Value::as_str) {
            if !content.is_empty() {
                out.push(Ok(StreamChunk::TextDelta(content.to_string())));
            }
        }

        if let Some(tcs) = delta
            .and_then(|d| d.get("tool_calls"))
            .and_then(Value::as_array)
        {
            for tc in tcs {
                let index = tc.get("index").and_then(Value::as_i64).unwrap_or(0);
                let entry = self.tools.entry(index).or_insert_with(|| OaTool {
                    id: String::new(),
                    name: String::new(),
                    args: String::new(),
                });
                if let Some(id) = tc.get("id").and_then(Value::as_str) {
                    if !id.is_empty() {
                        entry.id = id.to_string();
                    }
                }
                if let Some(func) = tc.get("function") {
                    if let Some(name) = func.get("name").and_then(Value::as_str) {
                        if !name.is_empty() {
                            entry.name = name.to_string();
                        }
                    }
                    if let Some(args) = func.get("arguments").and_then(Value::as_str) {
                        entry.args.push_str(args);
                    }
                }
            }
        }

        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            out.append(&mut self.flush_tools());
            out.push(Ok(StreamChunk::Done {
                stop_reason: Some(reason.to_string()),
                usage: self.usage.take(),
            }));
            self.done = true;
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatMessage, ToolSchema};

    #[test]
    fn body_maps_tools_and_system() {
        let mut req = ChatRequest::new("gpt-x", vec![ChatMessage::user("hi")]);
        req.system = Some("sys".into());
        req.tools = vec![ToolSchema {
            name: "files.read".into(),
            description: "read".into(),
            input_schema: json!({"type":"object"}),
        }];
        let body = OpenAiProvider::body(&req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "files.read");
    }

    #[test]
    fn tool_result_maps_to_tool_role() {
        let req = ChatRequest::new(
            "gpt-x",
            vec![ChatMessage::tool_result("call_1", "ok", false)],
        );
        let msgs = OpenAiProvider::messages_json(&req);
        assert_eq!(msgs[0]["role"], "tool");
        assert_eq!(msgs[0]["tool_call_id"], "call_1");
    }

    #[test]
    fn sse_accumulates_tool_call_across_deltas() {
        let mut s = OaState::default();
        // First delta: id + name.
        let a = s.handle(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"files.create","arguments":"{\"path\":"}}]}}]}"#);
        assert!(a.is_empty());
        // Second delta: more argument text.
        let b = s.handle(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.txt\"}"}}]}}]}"#);
        assert!(b.is_empty());
        // Finish → flush one ToolUse + Done.
        let c = s.handle(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#);
        match &c[0] {
            Ok(StreamChunk::ToolUse { name, input, .. }) => {
                assert_eq!(name, "files.create");
                assert_eq!(input["path"], "a.txt");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
        assert!(matches!(c[1], Ok(StreamChunk::Done { .. })));
    }

    #[test]
    fn embed_body_has_model_and_input() {
        let p = OpenAiProvider::openai(Some("k".into())).with_embed_model("text-embedding-3-small");
        let body = p.embed_body(vec!["a".into(), "b".into()]);
        assert_eq!(body["model"], "text-embedding-3-small");
        assert_eq!(body["input"][0], "a");
        assert_eq!(body["input"][1], "b");
    }

    #[test]
    fn sse_text_then_done() {
        let mut s = OaState::default();
        let a = s.handle(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#);
        assert_eq!(
            a[0].as_ref().unwrap(),
            &StreamChunk::TextDelta("Hello".into())
        );
        let b = s.handle("[DONE]");
        assert!(matches!(b[0], Ok(StreamChunk::Done { .. })));
    }
}
