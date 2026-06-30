//! Session CRUD.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_proto::{AuditEntryDto, CreateSessionRequest, RevertResult, SessionDto};

use crate::state::{AppError, AppState};

#[utoipa::path(
    post,
    path = "/sessions",
    operation_id = "create_session",
    request_body = CreateSessionRequest,
    responses((status = 200, description = "Session created", body = SessionDto)),
    tag = "sessions"
)]
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<Json<SessionDto>, AppError> {
    let session = state
        .agent
        .store()
        .create_session(body.project_id.as_deref(), body.title.as_deref())?;
    Ok(Json(session))
}

#[utoipa::path(
    get,
    path = "/sessions",
    operation_id = "list_sessions",
    responses((status = 200, description = "All sessions", body = [SessionDto])),
    tag = "sessions"
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<SessionDto>>, AppError> {
    Ok(Json(state.agent.store().list_sessions()?))
}

#[utoipa::path(
    post,
    path = "/sessions/{id}/revert",
    operation_id = "revert_last",
    params(("id" = String, Path, description = "Session id")),
    responses((status = 200, description = "Reverted the last file operation", body = RevertResult)),
    tag = "sessions"
)]
pub async fn revert(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RevertResult>, AppError> {
    let summary = getmasters_core::revision::revert_last(state.agent.store(), &id)
        .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(RevertResult { summary }))
}

#[utoipa::path(
    get,
    path = "/sessions/{id}/audit",
    operation_id = "list_audit",
    params(("id" = String, Path, description = "Session id")),
    responses((status = 200, description = "The session's gated tool-call audit trail", body = [AuditEntryDto])),
    tag = "sessions"
)]
pub async fn list_audit(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AuditEntryDto>>, AppError> {
    let store = state.agent.store();
    store.get_session(&id)?; // 404 if the session is unknown
    Ok(Json(store.audit_entries(&id)?))
}
