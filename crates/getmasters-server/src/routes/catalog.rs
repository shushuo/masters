//! Cloud catalog endpoints — trigger a sync of public system masters + skills, and read status.
//! The heavy lifting lives in [`crate::catalog`].

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use getmasters_proto::CatalogStatusDto;

use crate::state::{AppError, AppState};

#[utoipa::path(
    post,
    path = "/catalog/sync",
    operation_id = "sync_catalog",
    responses(
        (status = 200, description = "Catalog synced; returns the new status", body = CatalogStatusDto),
        (status = 502, description = "Could not reach or parse the cloud catalog")
    ),
    tag = "catalog"
)]
pub async fn sync(State(state): State<AppState>) -> Result<Json<CatalogStatusDto>, AppError> {
    // Manual sync always forces a refresh (ignores the version gate).
    let status = crate::catalog::sync_catalog(state, true)
        .await
        .map_err(|e| AppError::new(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(status))
}

#[utoipa::path(
    get,
    path = "/catalog/status",
    operation_id = "catalog_status",
    responses((status = 200, description = "Last-synced version/time + installed counts", body = CatalogStatusDto)),
    tag = "catalog"
)]
pub async fn status(State(state): State<AppState>) -> Json<CatalogStatusDto> {
    Json(crate::catalog::status(&state))
}
