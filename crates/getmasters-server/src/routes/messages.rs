//! Message listing + non-streaming send.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_proto::{MessageDto, SendMessageRequest};

use crate::state::{AppError, AppState};

#[utoipa::path(
    get,
    path = "/sessions/{id}/messages",
    operation_id = "list_messages",
    params(("id" = String, Path, description = "Session id")),
    responses((status = 200, description = "Messages in the session", body = [MessageDto])),
    tag = "sessions"
)]
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<MessageDto>>, AppError> {
    Ok(Json(state.agent.store().list_messages(&id)?))
}

#[utoipa::path(
    post,
    path = "/sessions/{id}/messages",
    operation_id = "send_message",
    params(("id" = String, Path, description = "Session id")),
    request_body = SendMessageRequest,
    responses((status = 200, description = "The persisted assistant reply", body = MessageDto)),
    tag = "sessions"
)]
pub async fn send(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> Result<Json<MessageDto>, AppError> {
    // Ensure the session exists for a clear 404 (run_turn would otherwise fail mid-stream).
    state.agent.store().get_session(&id)?;
    // Use the project's tool/knowledge-enabled agent when the session belongs to a project.
    let agent = state.agent_for_session(&id).await;
    let assistant = agent
        .complete_turn(&id, &body.content)
        .await
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(assistant))
}
