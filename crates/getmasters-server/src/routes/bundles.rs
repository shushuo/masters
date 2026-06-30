//! Portable team/master bundle endpoints (Phase 4h; ADR-0010): export a team as a self-contained
//! JSON bundle, and import a bundle into a project (recreating its masters + the team).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_proto::{BundleImportResult, TeamBundle};

use crate::bundle;
use crate::state::{AppError, AppState};

#[utoipa::path(
    get,
    path = "/projects/{id}/teams/{slug}/bundle",
    operation_id = "export_team_bundle",
    params(
        ("id" = String, Path, description = "Project id"),
        ("slug" = String, Path, description = "Team slug"),
    ),
    responses((status = 200, description = "The team + its masters as a portable bundle", body = TeamBundle)),
    tag = "projects"
)]
pub async fn export(
    State(state): State<AppState>,
    Path((id, slug)): Path<(String, String)>,
) -> Result<Json<TeamBundle>, AppError> {
    state.agent.store().get_project(&id)?; // 404 if unknown
    let bundle =
        bundle::export(&state, &id, &slug).map_err(|e| AppError::new(StatusCode::NOT_FOUND, e))?;
    Ok(Json(bundle))
}

#[utoipa::path(
    post,
    path = "/projects/{id}/bundles",
    operation_id = "import_team_bundle",
    params(("id" = String, Path, description = "Target project id")),
    request_body = TeamBundle,
    responses((status = 200, description = "The imported team + master slugs", body = BundleImportResult)),
    tag = "projects"
)]
pub async fn import(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TeamBundle>,
) -> Result<Json<BundleImportResult>, AppError> {
    state.agent.store().get_project(&id)?; // 404 if unknown
    let result =
        bundle::import(&state, &id, body).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(result))
}
