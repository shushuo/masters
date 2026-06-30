//! `GET /sessions/{id}/ws` — the streaming agent run (docs/02 §4).
//!
//! Protocol: the client sends a [`ClientCommand`]; the server replies with a sequence of
//! [`ServerEvent`]s (`MessageStart` → `TokenDelta`/`ToolCallStarted`/`ApprovalRequest`/
//! `ToolResult`* → `MessageComplete` | `Error`). An `ApprovalDecision` resolves a pending
//! approval; a `Stop` (or socket close) cancels the in-flight run between chunks.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use futures::StreamExt;

use getmasters_core::agent::AgentEvent;
use getmasters_core::permission::ApprovalDecision;
use getmasters_proto::{ClientCommand, ServerEvent};

use crate::group::GroupStreamEvent;
use crate::state::AppState;

pub async fn handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| run(socket, state, session_id))
}

/// Serialize and send a [`ServerEvent`]; returns `false` if the socket is gone.
async fn send_event(socket: &mut WebSocket, event: &ServerEvent) -> bool {
    match serde_json::to_string(event) {
        Ok(json) => socket.send(Message::text(json)).await.is_ok(),
        Err(_) => false,
    }
}

fn parse_command(text: &str) -> Option<ClientCommand> {
    serde_json::from_str(text).ok()
}

/// Resolve an approval decision against the shared registry, if one is wired.
fn resolve_decision(state: &AppState, request_id: &str, decision: &str) {
    if let Some(reg) = &state.approvals {
        reg.resolve(request_id, ApprovalDecision::from_wire(decision));
    }
}

/// Map an [`AgentEvent`] to the wire [`ServerEvent`]; returns `None` for events with no
/// direct wire form. The bool in the tuple marks a terminal event (ends the turn).
fn to_server_event(ev: AgentEvent) -> (Option<ServerEvent>, bool) {
    match ev {
        AgentEvent::Delta(text) => (Some(ServerEvent::TokenDelta { text }), false),
        AgentEvent::ToolCallStarted { id, name, summary } => (
            Some(ServerEvent::ToolCallStarted {
                id,
                tool: name,
                summary,
            }),
            false,
        ),
        AgentEvent::ApprovalRequest(req) => (
            Some(ServerEvent::ApprovalRequest {
                request_id: req.request_id,
                tool: req.tool,
                summary: req.summary,
                classes: req.classes.iter().map(|c| c.as_str().to_string()).collect(),
            }),
            false,
        ),
        AgentEvent::ToolResult {
            id,
            summary,
            is_error,
        } => (
            Some(ServerEvent::ToolResult {
                id,
                summary,
                is_error,
            }),
            false,
        ),
        AgentEvent::Complete { message_id } => {
            (Some(ServerEvent::MessageComplete { message_id }), true)
        }
        AgentEvent::Error(message) => (Some(ServerEvent::Error { message }), true),
    }
}

async fn run(mut socket: WebSocket, state: AppState, session_id: String) {
    while let Some(Ok(msg)) = socket.recv().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        match parse_command(&text) {
            Some(ClientCommand::Send {
                content,
                max_rounds,
            }) => {
                // A team-bound session is a group chat — stream the addressed masters (Phase 4e);
                // otherwise stream the single assistant turn.
                let is_group = state
                    .agent
                    .store()
                    .get_session(&session_id)
                    .ok()
                    .and_then(|s| s.team_slug)
                    .is_some();
                let ok = if is_group {
                    stream_group(&mut socket, &state, &session_id, &content, max_rounds).await
                } else {
                    stream_turn(&mut socket, &state, &session_id, &content).await
                };
                if !ok {
                    break; // socket closed during streaming
                }
            }
            Some(ClientCommand::ApprovalDecision {
                request_id,
                decision,
            }) => {
                // A decision with no run in flight (or after it ended) is harmless to resolve.
                resolve_decision(&state, &request_id, &decision);
            }
            Some(ClientCommand::Stop) => { /* nothing in flight between commands */ }
            None => {
                let _ = send_event(
                    &mut socket,
                    &ServerEvent::Error {
                        message: "invalid command".into(),
                    },
                )
                .await;
            }
        }
    }
}

/// Stream one turn, forwarding agent events and honoring interleaved `ApprovalDecision`/`Stop`.
/// Returns `false` if the socket closed (caller should stop).
async fn stream_turn(
    socket: &mut WebSocket,
    state: &AppState,
    session_id: &str,
    content: &str,
) -> bool {
    if !send_event(socket, &ServerEvent::MessageStart).await {
        return false;
    }

    // Project sessions get a tools+knowledge agent; project-less sessions use the base agent.
    let agent = state.agent_for_session(session_id).await;
    let mut stream = agent.run_turn(session_id, content).await;
    loop {
        tokio::select! {
            maybe_event = stream.next() => {
                match maybe_event {
                    Some(ev) => {
                        let (wire, terminal) = to_server_event(ev);
                        if let Some(event) = wire {
                            if !send_event(socket, &event).await {
                                return false;
                            }
                        }
                        if terminal {
                            return true;
                        }
                    }
                    None => return true, // stream ended without an explicit terminal event
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(t))) => {
                        match parse_command(&t) {
                            Some(ClientCommand::Stop) => {
                                drop(stream); // cancels the run between chunks
                                return true;
                            }
                            Some(ClientCommand::ApprovalDecision { request_id, decision }) => {
                                resolve_decision(state, &request_id, &decision);
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return false,
                    _ => {}
                }
            }
        }
    }
}

/// Stream a multi-master group turn (Phase 4e/4f): forward each round's `GroupStart{round}` and each
/// master's `MasterDelta`/`MasterComplete`/`MasterError` (tagged by round), then `GroupComplete`. A
/// `Stop` (or close) aborts the orchestrator + every in-flight master run. Returns `false` if closed.
async fn stream_group(
    socket: &mut WebSocket,
    state: &AppState,
    session_id: &str,
    content: &str,
    max_rounds: Option<u32>,
) -> bool {
    let mut turn = match crate::group::stream_post(state, session_id, content, max_rounds).await {
        Ok(t) => t,
        Err(message) => {
            let _ = send_event(socket, &ServerEvent::Error { message }).await;
            return true;
        }
    };

    let abort_all = |turn: &crate::group::GroupTurn| {
        for a in turn.aborts.lock().unwrap().iter() {
            a.abort();
        }
    };

    loop {
        tokio::select! {
            maybe_event = turn.events.recv() => {
                let wire = match maybe_event {
                    Some(GroupStreamEvent::RoundStart { round, addressed }) => {
                        Some(ServerEvent::GroupStart { round, addressed })
                    }
                    Some(GroupStreamEvent::Master { round, author, event }) => match event {
                        AgentEvent::Delta(text) => Some(ServerEvent::MasterDelta { round, author, text }),
                        AgentEvent::Complete { message_id } => {
                            Some(ServerEvent::MasterComplete { round, author, message_id })
                        }
                        AgentEvent::Error(message) => {
                            Some(ServerEvent::MasterError { round, author, message })
                        }
                        // Attributed tool-call visibility (Phase 4g).
                        AgentEvent::ToolCallStarted { id, name, summary } => {
                            Some(ServerEvent::MasterToolCall { round, author, id, tool: name, summary })
                        }
                        AgentEvent::ToolResult { id, summary, is_error } => {
                            Some(ServerEvent::MasterToolResult { round, author, id, summary, is_error })
                        }
                        // Group dispatch is headless — no ApprovalRequest can occur.
                        AgentEvent::ApprovalRequest(_) => None,
                    },
                    None => {
                        // Every round finished — the channel closed.
                        let _ = send_event(socket, &ServerEvent::GroupComplete).await;
                        return true;
                    }
                };
                if let Some(event) = wire {
                    if !send_event(socket, &event).await {
                        return false;
                    }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(t))) => {
                        if let Some(ClientCommand::Stop) = parse_command(&t) {
                            abort_all(&turn);
                            return true;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        abort_all(&turn);
                        return false;
                    }
                    _ => {}
                }
            }
        }
    }
}
