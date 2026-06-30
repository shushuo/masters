//! ACP harness endpoints (Phase 4i, ADR-0014): detect pre-installed coding agents on the machine so
//! the desktop can offer one-click registration of an external master agent.

use axum::Json;

use getmasters_proto::AvailableHarnessDto;

use crate::acp::registry;
use crate::state::AppError;

#[utoipa::path(
    get,
    path = "/acp/harnesses",
    operation_id = "list_acp_harnesses",
    responses((status = 200, description = "Known ACP coding harnesses + availability", body = [AvailableHarnessDto])),
    tag = "projects"
)]
pub async fn harnesses() -> Result<Json<Vec<AvailableHarnessDto>>, AppError> {
    Ok(Json(registry::detect()))
}
