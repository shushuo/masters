//! Schedule endpoints (Phase 3d, FR-17): CRUD over a project's recipe schedules plus run history.
//! The daemon's background loop ([`crate::scheduler`]) fires them while it's alive.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_proto::{CreateScheduleRequest, ScheduleDto, ScheduledRunDto, SetScheduleRequest};

use crate::scheduler;
use crate::state::{AppError, AppState};

fn to_dto(r: getmasters_core::store::ScheduleRow) -> ScheduleDto {
    ScheduleDto {
        id: r.id,
        recipe_name: r.recipe_name,
        kind: r.kind,
        cron_expr: r.cron_expr,
        next_run_at: r.next_run_at,
        enabled: r.enabled,
        deliver_notify: r.deliver_notify,
        deliver_email: r.deliver_email,
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[utoipa::path(
    post,
    path = "/projects/{id}/schedules",
    operation_id = "create_project_schedule",
    params(("id" = String, Path, description = "Project id")),
    request_body = CreateScheduleRequest,
    responses((status = 200, description = "Schedule created", body = ScheduleDto)),
    tag = "projects"
)]
pub async fn create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateScheduleRequest>,
) -> Result<Json<ScheduleDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown

    // The recipe must exist.
    if store.get_recipe_meta(&id, &body.recipe_name)?.is_none() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            format!("recipe '{}' not found", body.recipe_name),
        ));
    }

    // Resolve the initial fire time from the trigger kind.
    let next_run_at = match body.kind.as_str() {
        "cron" => {
            let expr = body
                .cron_expr
                .as_deref()
                .ok_or_else(|| AppError::new(StatusCode::BAD_REQUEST, "cron_expr is required"))?;
            scheduler::first_fire(expr, now_ms())
                .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?
        }
        "once" => Some(body.run_at.ok_or_else(|| {
            AppError::new(StatusCode::BAD_REQUEST, "run_at is required for a one-off")
        })?),
        other => {
            return Err(AppError::new(
                StatusCode::BAD_REQUEST,
                format!("unknown schedule kind '{other}'"),
            ))
        }
    };

    let params = serde_json::to_string(&body.params).unwrap_or_else(|_| "{}".into());
    let sid = store.create_schedule(
        &id,
        &body.recipe_name,
        &params,
        &body.kind,
        body.cron_expr.as_deref(),
        next_run_at,
        body.deliver_notify,
        body.deliver_email,
    )?;
    let row = store
        .get_schedule(&sid)?
        .ok_or_else(|| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "schedule vanished"))?;
    Ok(Json(to_dto(row)))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/schedules",
    operation_id = "list_project_schedules",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's schedules", body = [ScheduleDto])),
    tag = "projects"
)]
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ScheduleDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    Ok(Json(
        store.list_schedules(&id)?.into_iter().map(to_dto).collect(),
    ))
}

#[utoipa::path(
    put,
    path = "/projects/{id}/schedules/{sid}",
    operation_id = "set_project_schedule",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Schedule id"),
    ),
    request_body = SetScheduleRequest,
    responses((status = 200, description = "Updated schedule", body = ScheduleDto)),
    tag = "projects"
)]
pub async fn set(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
    Json(body): Json<SetScheduleRequest>,
) -> Result<Json<ScheduleDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    let row = store
        .get_schedule(&sid)?
        .filter(|r| r.project_id == id)
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, format!("schedule {sid}")))?;

    if let Some(enabled) = body.enabled {
        if enabled && !row.enabled {
            // Re-enabling: recompute the next fire so a stale time doesn't fire immediately.
            let next = match row.kind.as_str() {
                "cron" => row
                    .cron_expr
                    .as_deref()
                    .and_then(|e| scheduler::next_after(e, now_ms()).ok().flatten()),
                _ => row.next_run_at.filter(|t| *t > now_ms()),
            };
            store.set_schedule_next(&sid, next, true)?;
        } else {
            store.set_schedule_enabled(&sid, enabled)?;
        }
    }

    // Delivery flags (Phase 3e) — change only the ones present in the request.
    if body.deliver_notify.is_some() || body.deliver_email.is_some() {
        store.set_schedule_delivery(
            &sid,
            body.deliver_notify.unwrap_or(row.deliver_notify),
            body.deliver_email.unwrap_or(row.deliver_email),
        )?;
    }

    let row = store
        .get_schedule(&sid)?
        .ok_or_else(|| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "schedule vanished"))?;
    Ok(Json(to_dto(row)))
}

#[utoipa::path(
    delete,
    path = "/projects/{id}/schedules/{sid}",
    operation_id = "delete_project_schedule",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Schedule id"),
    ),
    responses((status = 204, description = "Schedule deleted")),
    tag = "projects"
)]
pub async fn delete(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    store.delete_schedule(&sid)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/projects/{id}/schedules/{sid}/runs",
    operation_id = "list_schedule_runs",
    params(
        ("id" = String, Path, description = "Project id"),
        ("sid" = String, Path, description = "Schedule id"),
    ),
    responses((status = 200, description = "The schedule's run history", body = [ScheduledRunDto])),
    tag = "projects"
)]
pub async fn runs(
    State(state): State<AppState>,
    Path((id, sid)): Path<(String, String)>,
) -> Result<Json<Vec<ScheduledRunDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    let runs = store
        .list_scheduled_runs(&sid)?
        .into_iter()
        .map(|r| ScheduledRunDto {
            started_at: r.started_at,
            status: r.status,
            session_id: r.session_id,
            summary: r.summary,
        })
        .collect();
    Ok(Json(runs))
}
