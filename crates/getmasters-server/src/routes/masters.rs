//! Master endpoints (Phase 4a, FR-39/46): CRUD over a project's `masters/<slug>.md` files plus an
//! on-demand "run" that hands a brief to one master (persona + per-master model + tool allow-list).
//! Running is headless (auto-approved within the project's grants, fully audited), like recipes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_core::masters::{AcpLaunch, Master, BACKEND_ACP, BACKEND_INTERNAL};
use getmasters_proto::{MasterDto, MasterRunResult, MasterSummaryDto, RunMasterRequest};

use crate::master::{master_store, run as run_master};
use crate::state::{AppError, AppState};

pub(crate) fn to_dto(slug: String, e: Master) -> MasterDto {
    let (acp_command, acp_args, acp_env) = match e.acp {
        Some(a) => (
            a.command,
            a.args,
            a.env.into_iter().map(|(k, v)| [k, v]).collect(),
        ),
        None => (String::new(), Vec::new(), Vec::new()),
    };
    MasterDto {
        slug,
        name: e.name,
        summary: e.summary,
        persona: e.persona,
        default_model: e.default_model,
        allowed_skills: e.allowed_skills,
        allowed_tools: e.allowed_tools,
        output_contract: e.output_contract,
        origin: e.origin,
        body: e.body,
        backend: e.backend,
        acp_command,
        acp_args,
        acp_env,
    }
}

pub(crate) fn from_dto(d: MasterDto) -> Master {
    let backend = if d.backend.is_empty() {
        BACKEND_INTERNAL.to_string()
    } else {
        d.backend
    };
    // For an ACP master with a command, fold the launch config into `acp`.
    let acp = if backend == BACKEND_ACP && !d.acp_command.is_empty() {
        Some(AcpLaunch {
            command: d.acp_command,
            args: d.acp_args,
            env: d.acp_env.into_iter().map(|[k, v]| (k, v)).collect(),
        })
    } else {
        None
    };
    Master {
        name: d.name,
        summary: d.summary,
        persona: d.persona,
        default_model: d.default_model,
        allowed_skills: d.allowed_skills,
        allowed_tools: d.allowed_tools,
        output_contract: d.output_contract,
        origin: if d.origin.is_empty() {
            "imported".to_string()
        } else {
            d.origin
        },
        body: d.body,
        backend,
        acp,
    }
}

#[utoipa::path(
    post,
    path = "/projects/{id}/masters",
    operation_id = "save_project_master",
    params(("id" = String, Path, description = "Project id")),
    request_body = MasterDto,
    responses((status = 200, description = "Master saved (with its canonical slug)", body = MasterDto)),
    tag = "projects"
)]
pub async fn save(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MasterDto>,
) -> Result<Json<MasterDto>, AppError> {
    state.agent.store().get_project(&id)?; // 404 if unknown
    let master = from_dto(body);
    // An ACP master must declare a launch command (mirrors the connector command check).
    if master.is_acp() && master.acp.is_none() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "an ACP master requires acp_command".to_string(),
        ));
    }
    let slug = master_store(&state, &id)
        .create(&master)
        .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(to_dto(slug, master)))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/masters",
    operation_id = "list_project_masters",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's masters", body = [MasterSummaryDto])),
    tag = "projects"
)]
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<MasterSummaryDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    let masters = store
        .list_masters(&id)?
        .into_iter()
        .map(|r| MasterSummaryDto {
            slug: r.slug,
            name: r.name,
            summary: r.summary,
            default_model: r.default_model,
            backend: r.backend,
        })
        .collect();
    Ok(Json(masters))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/masters/{slug}",
    operation_id = "get_project_master",
    params(
        ("id" = String, Path, description = "Project id"),
        ("slug" = String, Path, description = "Master slug"),
    ),
    responses((status = 200, description = "The full master", body = MasterDto)),
    tag = "projects"
)]
pub async fn get(
    State(state): State<AppState>,
    Path((id, slug)): Path<(String, String)>,
) -> Result<Json<MasterDto>, AppError> {
    state.agent.store().get_project(&id)?;
    let master = master_store(&state, &id)
        .load(&slug)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, format!("master {slug}")))?;
    Ok(Json(to_dto(slug, master)))
}

#[utoipa::path(
    delete,
    path = "/projects/{id}/masters/{slug}",
    operation_id = "delete_project_master",
    params(
        ("id" = String, Path, description = "Project id"),
        ("slug" = String, Path, description = "Master slug"),
    ),
    responses((status = 204, description = "Master deleted")),
    tag = "projects"
)]
pub async fn delete(
    State(state): State<AppState>,
    Path((id, slug)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    state.agent.store().get_project(&id)?;
    master_store(&state, &id)
        .delete(&slug)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/projects/{id}/masters/{slug}/run",
    operation_id = "run_project_master",
    params(
        ("id" = String, Path, description = "Project id"),
        ("slug" = String, Path, description = "Master slug"),
    ),
    request_body = RunMasterRequest,
    responses((status = 200, description = "Run result (session + final message)", body = MasterRunResult)),
    tag = "projects"
)]
pub async fn run(
    State(state): State<AppState>,
    Path((id, slug)): Path<(String, String)>,
    Json(body): Json<RunMasterRequest>,
) -> Result<Json<MasterRunResult>, AppError> {
    state.agent.store().get_project(&id)?;
    let result = run_master(&state, &id, &slug, &body.brief)
        .await
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(result))
}
