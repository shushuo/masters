//! Live WS streaming of multi-master group turns (Phase 4e/4f/4g, ADR-0012). Opens a session WS bound
//! to a team and asserts the streamed lifecycle: per-round `group_start`, attributed `master_delta`
//! chunks, attributed `master_tool_call`/`master_tool_result` (4g), `master_complete` per master, and a
//! terminal `group_complete` — bounded multi-round turn-taking (4f) and a clean post-back transcript.

use std::collections::HashMap;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::masters::{Master, MasterStore};
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{ClientCommand, ServerEvent};
use getmasters_server::{group, AppState};

const TOKEN: &str = "tok";

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-groupws-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn master(name: &str) -> Master {
    Master {
        name: name.into(),
        summary: "x".into(),
        persona: format!("You are {name}."),
        default_model: "mock:m".into(),
        allowed_skills: vec![],
        allowed_tools: vec![],
        output_contract: String::new(),
        origin: "learned".into(),
        body: String::new(),
        backend: "internal".into(),
        acp: None,
    }
}

fn seed(state: &AppState, store: &Store) -> String {
    let pid = store.create_project("p", None).unwrap();
    let es = MasterStore::new(state.project_dir(&pid), pid.clone(), store.clone());
    es.create(&master("Backend Architect")).unwrap();
    es.create(&master("Copy Writer")).unwrap();
    store
        .upsert_team(
            &pid,
            "squad",
            "Squad",
            "",
            "copy-writer",
            &["backend-architect".to_string(), "copy-writer".to_string()],
        )
        .unwrap();
    pid
}

#[tokio::test]
async fn group_turn_streams_attributed_events() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let agent = AgentService::new(store.clone(), Arc::new(MockProvider::new()), "mock:base");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    let state = AppState::new(agent, TOKEN.to_string()).with_config(cfg);
    let pid = seed(&state, &store);

    // A group session bound to the team.
    let session = group::start(&state, &pid, "squad", None).unwrap();
    let session_id = session.id.clone();

    let app = getmasters_server::build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let url = format!("ws://127.0.0.1:{port}/sessions/{session_id}/ws?token={TOKEN}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();

    let cmd = serde_json::to_string(&ClientCommand::Send {
        content: "@all kickoff".into(),
        max_rounds: None,
    })
    .unwrap();
    ws.send(WsMessage::Text(cmd.into())).await.unwrap();

    // Round 0 addresses both; each reply echoes "@all" → a follow-up round, capped at MAX_GROUP_ROUNDS
    // (3). So we expect rounds 0,1,2 then a terminal group_complete (the cap stops the echo loop).
    let mut round_starts: Vec<u32> = Vec::new();
    let mut deltas: HashMap<(u32, String), String> = HashMap::new();
    let mut completes: Vec<(u32, String)> = Vec::new();
    let mut group_done = false;

    while let Some(Ok(msg)) = ws.next().await {
        let WsMessage::Text(t) = msg else { continue };
        match serde_json::from_str::<ServerEvent>(&t).unwrap() {
            ServerEvent::GroupStart {
                round,
                mut addressed,
            } => {
                // Round 0 is participant order; follow-up rounds are mention order — compare as a set.
                addressed.sort();
                assert_eq!(addressed, vec!["backend-architect", "copy-writer"]);
                round_starts.push(round);
            }
            ServerEvent::MasterDelta {
                round,
                author,
                text,
            } => deltas.entry((round, author)).or_default().push_str(&text),
            ServerEvent::MasterComplete {
                round,
                author,
                message_id,
            } => {
                assert!(!message_id.is_empty());
                completes.push((round, author));
            }
            ServerEvent::MasterError {
                author, message, ..
            } => {
                panic!("unexpected master error from {author}: {message}")
            }
            ServerEvent::GroupComplete => {
                group_done = true;
                break;
            }
            ServerEvent::Error { message } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }

    assert!(group_done, "expected a terminal group_complete");
    // The cap stopped the echo loop at exactly three rounds.
    assert_eq!(round_starts, vec![0, 1, 2]);
    assert_eq!(completes.len(), 6, "3 rounds × 2 masters");
    // Streaming is attributed per (round, author) and carries the brief echo.
    assert!(deltas[&(0, "backend-architect".to_string())].contains("kickoff"));
    assert!(deltas[&(2, "copy-writer".to_string())].contains("kickoff"));

    // The post-back left a clean transcript: the user message + every round's replies (1 + 6).
    let transcript = store.list_messages(&session_id).unwrap();
    assert_eq!(transcript.len(), 7);
    assert_eq!(transcript[0].author, "user");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mention_streams_only_the_addressed_master() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let agent = AgentService::new(store.clone(), Arc::new(MockProvider::new()), "mock:base");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    let state = AppState::new(agent, TOKEN.to_string()).with_config(cfg);
    let pid = seed(&state, &store);
    let session = group::start(&state, &pid, "squad", None).unwrap();
    let session_id = session.id.clone();

    let app = getmasters_server::build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let url = format!("ws://127.0.0.1:{port}/sessions/{session_id}/ws?token={TOKEN}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    // Exercise the WS `max_rounds` field end-to-end (Phase 4f): a per-call cap threaded through
    // ws.rs → group::stream_post → clamp_rounds. `Some(2)` is within `1..=5`, so the addressed
    // master still replies; the field just bounds any mention-driven follow-up rounds.
    let cmd = serde_json::to_string(&ClientCommand::Send {
        content: "@backend-architect design".into(),
        max_rounds: Some(2),
    })
    .unwrap();
    ws.send(WsMessage::Text(cmd.into())).await.unwrap();

    let mut completes: Vec<String> = Vec::new();
    while let Some(Ok(msg)) = ws.next().await {
        let WsMessage::Text(t) = msg else { continue };
        match serde_json::from_str::<ServerEvent>(&t).unwrap() {
            ServerEvent::MasterComplete { author, .. } => completes.push(author),
            ServerEvent::GroupComplete => break,
            _ => {}
        }
    }
    assert_eq!(completes, vec!["backend-architect"]);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn master_tool_calls_stream_attributed() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let agent = AgentService::new(store.clone(), Arc::new(MockProvider::new()), "mock:base");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    let state = AppState::new(agent, TOKEN.to_string()).with_config(cfg);
    let pid = seed(&state, &store);
    let session = group::start(&state, &pid, "squad", None).unwrap();
    let session_id = session.id.clone();

    let app = getmasters_server::build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let url = format!("ws://127.0.0.1:{port}/sessions/{session_id}/ws?token={TOKEN}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    // Address one master with a brief that triggers a no-grant Read tool (Study is hosted by default;
    // `list_decks` classifies as Read so it auto-allows). The mock runs ToolUse → execute → summary.
    let cmd = serde_json::to_string(&ClientCommand::Send {
        content: "@backend-architect [[tool:study.list_decks]]".into(),
        max_rounds: None,
    })
    .unwrap();
    ws.send(WsMessage::Text(cmd.into())).await.unwrap();

    let mut tool_call: Option<(u32, String, String)> = None; // (round, author, tool)
    let mut tool_result: Option<(u32, String, bool)> = None; // (round, author, is_error)
    let mut completed_after_tool = false;

    while let Some(Ok(msg)) = ws.next().await {
        let WsMessage::Text(t) = msg else { continue };
        match serde_json::from_str::<ServerEvent>(&t).unwrap() {
            ServerEvent::MasterToolCall {
                round,
                author,
                tool,
                ..
            } => tool_call = Some((round, author, tool)),
            ServerEvent::MasterToolResult {
                round,
                author,
                is_error,
                ..
            } => tool_result = Some((round, author, is_error)),
            ServerEvent::MasterComplete { .. } => {
                completed_after_tool = tool_call.is_some() && tool_result.is_some();
            }
            ServerEvent::MasterError {
                author, message, ..
            } => {
                panic!("unexpected master error from {author}: {message}")
            }
            ServerEvent::GroupComplete => break,
            _ => {}
        }
    }

    assert_eq!(
        tool_call,
        Some((
            0,
            "backend-architect".to_string(),
            "study.list_decks".to_string()
        )),
        "the tool call should be attributed to the master + round"
    );
    assert_eq!(
        tool_result,
        Some((0, "backend-architect".to_string(), false)),
        "the tool result should be attributed and succeed"
    );
    assert!(
        completed_after_tool,
        "the master reply should complete after its tool call + result"
    );

    std::fs::remove_dir_all(&dir).ok();
}
