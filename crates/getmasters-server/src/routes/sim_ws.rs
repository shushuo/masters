//! `GET /projects/{id}/simulations/{sid}/ws` — live streaming of a simulation round (模拟盘).
//!
//! On connect the daemon starts one round and streams each master's reasoning token-by-token,
//! attributed, reusing the group-chat wire events (`GroupStart` / `MasterDelta` / `MasterComplete` /
//! `MasterError` / `MasterToolCall` / `MasterToolResult` → `GroupComplete`). A `Stop` (or socket
//! close) aborts the in-flight masters and recovers the sim's `running` state. The deterministic
//! settle + persistence happen in the orchestrator once every master finishes.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;

use getmasters_core::agent::AgentEvent;
use getmasters_proto::{ClientCommand, ServerEvent};

use crate::group::GroupStreamEvent;
use crate::simlab::SimStreamTurn;
use crate::state::AppState;

pub async fn handler(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| run(socket, state, id, sid))
}

async fn send_event(socket: &mut WebSocket, event: &ServerEvent) -> bool {
    match serde_json::to_string(event) {
        Ok(json) => socket.send(Message::text(json)).await.is_ok(),
        Err(_) => false,
    }
}

/// Map a sim stream event onto the group wire events (the desktop already renders these).
fn map_event(ev: GroupStreamEvent) -> Option<ServerEvent> {
    match ev {
        GroupStreamEvent::RoundStart { round, addressed } => {
            Some(ServerEvent::GroupStart { round, addressed })
        }
        GroupStreamEvent::Master {
            round,
            author,
            event,
        } => match event {
            AgentEvent::Delta(text) => Some(ServerEvent::MasterDelta {
                round,
                author,
                text,
            }),
            AgentEvent::Complete { message_id } => Some(ServerEvent::MasterComplete {
                round,
                author,
                message_id,
            }),
            AgentEvent::Error(message) => Some(ServerEvent::MasterError {
                round,
                author,
                message,
            }),
            AgentEvent::ToolCallStarted { id, name, summary } => {
                Some(ServerEvent::MasterToolCall {
                    round,
                    author,
                    id,
                    tool: name,
                    summary,
                })
            }
            AgentEvent::ToolResult {
                id,
                summary,
                is_error,
            } => Some(ServerEvent::MasterToolResult {
                round,
                author,
                id,
                summary,
                is_error,
            }),
            // Sim dispatch is headless — no ApprovalRequest can occur.
            AgentEvent::ApprovalRequest(_) => None,
        },
    }
}

async fn run(mut socket: WebSocket, state: AppState, project_id: String, sim_id: String) {
    let owned = state
        .agent
        .store()
        .get_simulation(&sim_id)
        .ok()
        .flatten()
        .is_some_and(|s| s.project_id == project_id);
    if !owned {
        let _ = send_event(
            &mut socket,
            &ServerEvent::Error {
                message: "simulation not found".into(),
            },
        )
        .await;
        return;
    }

    let mut turn = match crate::simlab::stream_round(&state, &sim_id).await {
        Ok(t) => t,
        Err(message) => {
            let _ = send_event(&mut socket, &ServerEvent::Error { message }).await;
            return;
        }
    };

    let abort_all = |turn: &SimStreamTurn| {
        for a in turn.aborts.lock().unwrap().iter() {
            a.abort();
        }
    };
    // Abort the round and recover the sim's `running` state (the orchestrator's own cleanup won't
    // run if it's aborted mid-flight).
    let recover = |state: &AppState| {
        let _ = state.agent.store().set_simulation_state(&sim_id, "active");
    };

    loop {
        tokio::select! {
            maybe_event = turn.events.recv() => {
                match maybe_event {
                    Some(ev) => {
                        if let Some(event) = map_event(ev) {
                            if !send_event(&mut socket, &event).await {
                                abort_all(&turn);
                                recover(&state);
                                return;
                            }
                        }
                    }
                    None => {
                        // The round settled and the channel closed.
                        let _ = send_event(&mut socket, &ServerEvent::GroupComplete).await;
                        return;
                    }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(ClientCommand::Stop) = serde_json::from_str::<ClientCommand>(&t) {
                            abort_all(&turn);
                            recover(&state);
                            return;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        abort_all(&turn);
                        recover(&state);
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
}
