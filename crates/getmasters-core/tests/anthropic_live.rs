//! Live Anthropic provider test — **ignored by default**.
//!
//! Runs only on demand against the real API. Requires `ANTHROPIC_API_KEY` and network
//! egress, and an opt-in `GETMASTERS_RUN_LIVE=1` guard so a stray key in the environment can't
//! turn an offline `cargo test` into a billed network call.
//!
//! Run with: `GETMASTERS_RUN_LIVE=1 cargo test -p getmasters-core --test anthropic_live -- --ignored`

use futures::StreamExt;
use getmasters_core::config::DEFAULT_MODEL;
use getmasters_core::provider::{
    AnthropicProvider, ChatMessage, ChatRequest, Provider, StreamChunk,
};

fn live_enabled() -> Option<String> {
    if std::env::var("GETMASTERS_RUN_LIVE").as_deref() != Ok("1") {
        return None;
    }
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
}

#[tokio::test]
#[ignore = "hits the real Anthropic API; needs ANTHROPIC_API_KEY + GETMASTERS_RUN_LIVE=1"]
async fn streams_a_real_reply() {
    let Some(key) = live_enabled() else {
        eprintln!("skipping live test (set GETMASTERS_RUN_LIVE=1 and ANTHROPIC_API_KEY)");
        return;
    };
    let provider = AnthropicProvider::new(key);
    let mut req = ChatRequest::new(
        DEFAULT_MODEL,
        vec![ChatMessage::user("Reply with exactly the word: pong")],
    );
    req.max_tokens = 16;

    let mut stream = provider.stream(req).await.expect("stream opens");
    let mut text = String::new();
    let mut saw_done = false;
    while let Some(chunk) = stream.next().await {
        match chunk.expect("chunk ok") {
            StreamChunk::TextDelta(t) => text.push_str(&t),
            StreamChunk::ToolUse { .. } => {}
            StreamChunk::Done { .. } => saw_done = true,
        }
    }
    assert!(saw_done, "expected a Done chunk");
    assert!(!text.trim().is_empty(), "expected non-empty streamed text");
}
