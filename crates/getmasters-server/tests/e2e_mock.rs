//! End-to-end integration test for `getmastersd` over the mock provider.
//!
//! Proves the Phase 0 exit criterion headlessly: bind the app on a loopback ephemeral port,
//! hit `/health`, create a session with the bearer token, open the WebSocket, send a prompt,
//! and assert we receive `MessageStart` → `TokenDelta`* → `MessageComplete`. Also checks that
//! a request without the token is rejected with 401.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{ClientCommand, MessageDto, ServerEvent, SessionDto};
use getmasters_server::{build_app, AppState};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as WsMessage;

const TOKEN: &str = "test-token-abc123";

/// Spawn the app on a loopback ephemeral port; return the bound port.
async fn spawn_server() -> u16 {
    let store = Store::open_in_memory().unwrap();
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock");
    let state = AppState::new(agent, TOKEN.to_string());
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    port
}

#[tokio::test]
async fn health_is_public_and_reports_mock() {
    let port = spawn_server().await;
    let resp = reqwest::get(format!("http://127.0.0.1:{port}/health"))
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["provider"], "mock");
}

#[tokio::test]
async fn sessions_require_token() {
    let port = spawn_server().await;
    let client = reqwest::Client::new();
    // No token → 401.
    let resp = client
        .post(format!("http://127.0.0.1:{port}/sessions"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Wrong token → 401.
    let resp = client
        .post(format!("http://127.0.0.1:{port}/sessions"))
        .bearer_auth("nope")
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn http_send_round_trips_over_mock() {
    let port = spawn_server().await;
    let client = reqwest::Client::new();

    let session: SessionDto = client
        .post(format!("http://127.0.0.1:{port}/sessions"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "title": "t" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let reply: MessageDto = client
        .post(format!(
            "http://127.0.0.1:{port}/sessions/{}/messages",
            session.id
        ))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "content": "hello" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reply.role, "assistant");
    assert_eq!(reply.content, "echo: hello");

    // Transcript now holds both turns.
    let msgs: Vec<MessageDto> = client
        .get(format!(
            "http://127.0.0.1:{port}/sessions/{}/messages",
            session.id
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(msgs.len(), 2);
}

#[tokio::test]
async fn websocket_streams_prompt_to_reply() {
    let port = spawn_server().await;
    let client = reqwest::Client::new();

    // Create a session over HTTP first.
    let session: SessionDto = client
        .post(format!("http://127.0.0.1:{port}/sessions"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Token via query param (the browser/webview WS path).
    let url = format!(
        "ws://127.0.0.1:{port}/sessions/{}/ws?token={TOKEN}",
        session.id
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();

    let cmd = serde_json::to_string(&ClientCommand::Send {
        content: "hi there".to_string(),
        max_rounds: None,
    })
    .unwrap();
    ws.send(WsMessage::Text(cmd.into())).await.unwrap();

    let mut started = false;
    let mut text = String::new();
    let mut completed = false;

    while let Some(Ok(msg)) = ws.next().await {
        if let WsMessage::Text(t) = msg {
            let event: ServerEvent = serde_json::from_str(&t).unwrap();
            match event {
                ServerEvent::MessageStart => started = true,
                ServerEvent::TokenDelta { text: d } => text.push_str(&d),
                ServerEvent::MessageComplete { .. } => {
                    completed = true;
                    break;
                }
                ServerEvent::Error { message } => panic!("unexpected error event: {message}"),
                // No tools / no group chat in this plain-prompt test.
                ServerEvent::ToolCallStarted { .. }
                | ServerEvent::ToolResult { .. }
                | ServerEvent::ApprovalRequest { .. }
                | ServerEvent::GroupStart { .. }
                | ServerEvent::MasterDelta { .. }
                | ServerEvent::MasterComplete { .. }
                | ServerEvent::MasterError { .. }
                | ServerEvent::MasterToolCall { .. }
                | ServerEvent::MasterToolResult { .. }
                | ServerEvent::GroupComplete => {}
            }
        }
    }

    assert!(started, "expected a MessageStart event");
    assert!(completed, "expected a MessageComplete event");
    assert_eq!(text, "echo: hi there");
}
