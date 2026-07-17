//! The cloud daily-snapshot proxy endpoint (D13 heartbeat). Always 200 — best-effort, empty
//! on cloud failure (the desktop then uses its local quote pack). See [`crate::snapshot`].

use axum::extract::State;
use axum::Json;

use getmasters_proto::DailySnapshotDto;

use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/snapshot/daily",
    operation_id = "daily_snapshot",
    responses((status = 200, description = "The cloud daily payload (market cross-section + weekly bulletin + master quotes); empty when the cloud is unreachable", body = DailySnapshotDto)),
    tag = "investing"
)]
pub async fn daily(State(state): State<AppState>) -> Json<DailySnapshotDto> {
    Json(state.daily_snapshot().await)
}
