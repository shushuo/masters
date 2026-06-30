//! `GET /health` — unauthenticated readiness probe.

use axum::extract::State;
use axum::Json;

use getmasters_proto::HealthDto;

use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/health",
    responses((status = 200, description = "Daemon is serving", body = HealthDto)),
    tag = "system"
)]
pub async fn health(State(state): State<AppState>) -> Json<HealthDto> {
    // `configured` reflects what the daemon can actually serve right now: the placeholder
    // `UnconfiguredProvider` (started without a key) reports `false`, so the desktop opens the setup
    // wizard. Provider/key changes made in the UI take effect on the next daemon launch.
    let provider = state.agent.provider_name();
    Json(HealthDto {
        status: "ok".to_string(),
        provider: provider.to_string(),
        configured: provider != "unconfigured",
        version: state.version.to_string(),
    })
}
