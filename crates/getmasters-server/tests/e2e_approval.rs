//! WS approval round-trip: a tool-enabled agent with a `ChannelApprover` emits an
//! `ApprovalRequest`; the client replies `ApprovalDecision{allow}`; the run proceeds to a
//! `ToolResult` and `MessageComplete`, and the file is written.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::extensions::ExtensionManager;
use getmasters_core::permission::{ApprovalRegistry, GrantSet};
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{ClientCommand, FolderAccess, FolderGrant, ServerEvent};
use getmasters_server::{build_app, AppState};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as WsMessage;

const TOKEN: &str = "approval-token";

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-approval-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

#[tokio::test]
async fn ws_approval_round_trip_runs_tool() {
    let dir = temp_dir();
    let target = dir.join("approved.txt");

    let store = Store::open_in_memory().unwrap();
    let session = store.create_session(None, None).unwrap();
    let session_id = session.id.clone();

    let grant = FolderGrant {
        id: "g".into(),
        project_id: None,
        path: dir.to_string_lossy().into_owned(),
        access: FolderAccess::ReadWrite,
        created_at: 0,
    };
    let extensions = ExtensionManager::with_builtin_files(vec![grant.clone()])
        .await
        .unwrap();
    let grants = GrantSet::new(vec![grant]);
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock")
        .with_extensions(Arc::new(extensions), Arc::new(grants))
        .with_approval_registry(Arc::new(ApprovalRegistry::new()));

    let state = AppState::new(agent, TOKEN.to_string());
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("ws://127.0.0.1:{port}/sessions/{session_id}/ws?token={TOKEN}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();

    // Trigger a gated files.create.
    let prompt = format!(
        "[[tool:files.create|path={}|content=approved write]]",
        target.to_str().unwrap()
    );
    let cmd = serde_json::to_string(&ClientCommand::Send {
        content: prompt,
        max_rounds: None,
    })
    .unwrap();
    ws.send(WsMessage::Text(cmd.into())).await.unwrap();

    let mut approved = false;
    let mut saw_tool_result = false;
    let mut completed = false;

    while let Some(Ok(msg)) = ws.next().await {
        let WsMessage::Text(t) = msg else { continue };
        let event: ServerEvent = serde_json::from_str(&t).unwrap();
        match event {
            ServerEvent::ApprovalRequest {
                request_id,
                classes,
                ..
            } => {
                assert!(classes.contains(&"write".to_string()));
                let reply = serde_json::to_string(&ClientCommand::ApprovalDecision {
                    request_id,
                    decision: "allow".into(),
                })
                .unwrap();
                ws.send(WsMessage::Text(reply.into())).await.unwrap();
                approved = true;
            }
            ServerEvent::ToolResult { is_error, .. } => {
                assert!(!is_error, "tool should succeed after approval");
                saw_tool_result = true;
            }
            ServerEvent::MessageComplete { .. } => {
                completed = true;
                break;
            }
            ServerEvent::Error { message } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }

    assert!(approved, "expected an ApprovalRequest");
    assert!(saw_tool_result, "expected a ToolResult");
    assert!(completed, "expected MessageComplete");
    assert!(target.exists(), "file should be written after approval");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "approved write");

    // The gated call must surface on the audit endpoint (Enhancement A): the approved
    // `files.create` row, with a real id + timestamp.
    let audit: Vec<serde_json::Value> = reqwest::Client::new()
        .get(format!(
            "http://127.0.0.1:{port}/sessions/{session_id}/audit"
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let created = audit
        .iter()
        .find(|e| e["tool"] == "files.create")
        .expect("expected a files.create audit row");
    assert_eq!(created["decision"], "approved");
    assert!(created["id"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(created["created_at"].as_i64().unwrap() > 0);
}
