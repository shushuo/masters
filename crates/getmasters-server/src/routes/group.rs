//! Group-chat endpoints (Phase 4c, FR-43): start a group session for a team, and post a user
//! message into it (resolve mentions → dispatch the addressed masters → return attributed replies).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_proto::{GroupPostRequest, GroupPostResult, SessionDto};

use crate::group;
use crate::state::{AppError, AppState};

#[utoipa::path(
    post,
    path = "/projects/{id}/teams/{slug}/session",
    operation_id = "start_team_group_session",
    params(
        ("id" = String, Path, description = "Project id"),
        ("slug" = String, Path, description = "Team slug"),
    ),
    responses((status = 200, description = "The new group session", body = SessionDto)),
    tag = "projects"
)]
pub async fn start(
    State(state): State<AppState>,
    Path((id, slug)): Path<(String, String)>,
) -> Result<Json<SessionDto>, AppError> {
    state.agent.store().get_project(&id)?;
    let session = group::start(&state, &id, &slug, None)
        .map_err(|e| AppError::new(StatusCode::NOT_FOUND, e))?;
    Ok(Json(session))
}

#[utoipa::path(
    post,
    path = "/sessions/{id}/group",
    operation_id = "post_group_message",
    params(("id" = String, Path, description = "Group session id")),
    request_body = GroupPostRequest,
    responses((status = 200, description = "Addressed masters + their attributed replies", body = GroupPostResult)),
    tag = "sessions"
)]
pub async fn post(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<GroupPostRequest>,
) -> Result<Json<GroupPostResult>, AppError> {
    state.agent.store().get_session(&id)?;
    let result = group::post(&state, &id, &body.content, body.max_rounds)
        .await
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(result))
}
