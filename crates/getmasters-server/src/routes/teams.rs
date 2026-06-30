//! Master Team endpoints (Phase 4b, FR-38/40): CRUD over a project's teams, the read-only router
//! (`route`), and a single-master team run (route/override → dispatch via the 4a master run).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_core::skills::slugify;
use getmasters_proto::{
    CreateTeamRequest, RouteBriefRequest, RouteResultDto, RunTeamRequest, TeamDto, TeamRunResult,
    TeamSummaryDto,
};

use crate::state::{AppError, AppState};
use crate::team;

#[utoipa::path(
    post,
    path = "/projects/{id}/teams",
    operation_id = "save_project_team",
    params(("id" = String, Path, description = "Project id")),
    request_body = CreateTeamRequest,
    responses((status = 200, description = "Team saved (with its canonical slug)", body = TeamDto)),
    tag = "projects"
)]
pub async fn save(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateTeamRequest>,
) -> Result<Json<TeamDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    let slug = slugify(&body.name);
    store.upsert_team(
        &id,
        &slug,
        &body.name,
        &body.summary,
        &body.coordinator_slug,
        &body.members,
    )?;
    Ok(Json(TeamDto {
        slug,
        name: body.name,
        summary: body.summary,
        coordinator_slug: body.coordinator_slug,
        members: body.members,
    }))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/teams",
    operation_id = "list_project_teams",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's teams", body = [TeamSummaryDto])),
    tag = "projects"
)]
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TeamSummaryDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    let teams = store
        .list_teams(&id)?
        .into_iter()
        .map(|t| TeamSummaryDto {
            slug: t.slug,
            name: t.name,
            summary: t.summary,
            member_count: t.members.len() as i64,
        })
        .collect();
    Ok(Json(teams))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/teams/{slug}",
    operation_id = "get_project_team",
    params(
        ("id" = String, Path, description = "Project id"),
        ("slug" = String, Path, description = "Team slug"),
    ),
    responses((status = 200, description = "The full team", body = TeamDto)),
    tag = "projects"
)]
pub async fn get(
    State(state): State<AppState>,
    Path((id, slug)): Path<(String, String)>,
) -> Result<Json<TeamDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    let t = store
        .get_team(&id, &slug)?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, format!("team {slug}")))?;
    Ok(Json(TeamDto {
        slug: t.slug,
        name: t.name,
        summary: t.summary,
        coordinator_slug: t.coordinator_slug,
        members: t.members,
    }))
}

#[utoipa::path(
    delete,
    path = "/projects/{id}/teams/{slug}",
    operation_id = "delete_project_team",
    params(
        ("id" = String, Path, description = "Project id"),
        ("slug" = String, Path, description = "Team slug"),
    ),
    responses((status = 204, description = "Team deleted")),
    tag = "projects"
)]
pub async fn delete(
    State(state): State<AppState>,
    Path((id, slug)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    store.delete_team(&id, &slug)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/projects/{id}/teams/{slug}/route",
    operation_id = "route_team_brief",
    params(
        ("id" = String, Path, description = "Project id"),
        ("slug" = String, Path, description = "Team slug"),
    ),
    request_body = RouteBriefRequest,
    responses((status = 200, description = "Router recommendation (ranked + selected)", body = RouteResultDto)),
    tag = "projects"
)]
pub async fn route(
    State(state): State<AppState>,
    Path((id, slug)): Path<(String, String)>,
    Json(body): Json<RouteBriefRequest>,
) -> Result<Json<RouteResultDto>, AppError> {
    state.agent.store().get_project(&id)?;
    let result = team::route(&state, &id, &slug, &body.brief)
        .map_err(|e| AppError::new(StatusCode::NOT_FOUND, e))?;
    Ok(Json(result))
}

#[utoipa::path(
    post,
    path = "/projects/{id}/teams/{slug}/run",
    operation_id = "run_team_brief",
    params(
        ("id" = String, Path, description = "Project id"),
        ("slug" = String, Path, description = "Team slug"),
    ),
    request_body = RunTeamRequest,
    responses((status = 200, description = "The chosen master + its run result", body = TeamRunResult)),
    tag = "projects"
)]
pub async fn run(
    State(state): State<AppState>,
    Path((id, slug)): Path<(String, String)>,
    Json(body): Json<RunTeamRequest>,
) -> Result<Json<TeamRunResult>, AppError> {
    state.agent.store().get_project(&id)?;
    let result = team::run(&state, &id, &slug, &body.brief, body.master.as_deref())
        .await
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(result))
}
