//! **Standalone (global) master endpoints** (Masters sidebar) — CRUD over `<data_home>/masters/`
//! files that exist independent of any project, plus the built-in template gallery, the user-starred
//! default master, and "quick chat" (start an interactive group chat over an ad-hoc set of masters).
//!
//! These mirror the project-scoped handlers in [`crate::routes::masters`] but take no `{id}` —
//! reusing [`to_dto`]/[`from_dto`] and the same `MasterDto`. Quick chat reuses the existing group
//! machinery: it ensures the system default project (run context), upserts an ephemeral team, and
//! binds a session to it ([`crate::group::start`]); the desktop then drives the normal group WS.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_proto::{
    DefaultMasterDto, MasterDto, MasterSummaryDto, QuickChatRequest, SessionDto,
};

use crate::routes::masters::{from_dto, to_dto};
use crate::state::{AppError, AppState, DEFAULT_MASTER_KEY};

#[utoipa::path(
    post,
    path = "/masters",
    operation_id = "save_global_master",
    request_body = MasterDto,
    responses((status = 200, description = "Master saved (with its canonical slug)", body = MasterDto)),
    tag = "masters"
)]
pub async fn save(
    State(state): State<AppState>,
    Json(body): Json<MasterDto>,
) -> Result<Json<MasterDto>, AppError> {
    let master = from_dto(body);
    if master.is_acp() && master.acp.is_none() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "an ACP master requires acp_command".to_string(),
        ));
    }
    let slug = state
        .global_master_store()
        .create(&master)
        .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(to_dto(slug, master)))
}

#[utoipa::path(
    get,
    path = "/masters",
    operation_id = "list_global_masters",
    responses((status = 200, description = "Standalone masters", body = [MasterSummaryDto])),
    tag = "masters"
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<MasterSummaryDto>>, AppError> {
    let masters = state
        .global_master_store()
        .list()
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
    path = "/masters/templates",
    operation_id = "list_master_templates",
    responses((status = 200, description = "Built-in master templates (the system gallery)", body = [MasterDto])),
    tag = "masters"
)]
pub async fn templates() -> Result<Json<Vec<MasterDto>>, AppError> {
    Ok(Json(crate::master_templates::builtin()))
}

#[utoipa::path(
    get,
    path = "/masters/default",
    operation_id = "get_default_master",
    responses((status = 200, description = "The starred default master (slug empty if none)", body = DefaultMasterDto)),
    tag = "masters"
)]
pub async fn get_default(
    State(state): State<AppState>,
) -> Result<Json<DefaultMasterDto>, AppError> {
    let slug = state
        .agent
        .store()
        .get_setting(DEFAULT_MASTER_KEY)?
        .unwrap_or_default();
    Ok(Json(DefaultMasterDto { slug }))
}

#[utoipa::path(
    put,
    path = "/masters/default",
    operation_id = "set_default_master",
    request_body = DefaultMasterDto,
    responses((status = 200, description = "Default master set", body = DefaultMasterDto)),
    tag = "masters"
)]
pub async fn set_default(
    State(state): State<AppState>,
    Json(body): Json<DefaultMasterDto>,
) -> Result<Json<DefaultMasterDto>, AppError> {
    state
        .agent
        .store()
        .set_setting(DEFAULT_MASTER_KEY, &body.slug)?;
    Ok(Json(body))
}

#[utoipa::path(
    get,
    path = "/masters/{slug}",
    operation_id = "get_global_master",
    params(("slug" = String, Path, description = "Master slug")),
    responses((status = 200, description = "The full master", body = MasterDto)),
    tag = "masters"
)]
pub async fn get(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<MasterDto>, AppError> {
    let master = state
        .global_master_store()
        .load(&slug)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, format!("master {slug}")))?;
    Ok(Json(to_dto(slug, master)))
}

#[utoipa::path(
    delete,
    path = "/masters/{slug}",
    operation_id = "delete_global_master",
    params(("slug" = String, Path, description = "Master slug")),
    responses((status = 204, description = "Master deleted")),
    tag = "masters"
)]
pub async fn delete(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .global_master_store()
        .delete(&slug)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/masters/quickchat",
    operation_id = "start_quick_chat",
    request_body = QuickChatRequest,
    responses((status = 200, description = "A team-bound group session over the selected masters", body = SessionDto)),
    tag = "masters"
)]
pub async fn quickchat(
    State(state): State<AppState>,
    Json(body): Json<QuickChatRequest>,
) -> Result<Json<SessionDto>, AppError> {
    let members: Vec<String> = body
        .masters
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();
    if members.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "quick chat needs at least one master".to_string(),
        ));
    }

    let project_id = state
        .ensure_default_project()
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // Coordinator (answers unaddressed messages) = the starred default master if it's in the
    // selection, else the first selected master.
    let starred = state
        .agent
        .store()
        .get_setting(DEFAULT_MASTER_KEY)?
        .unwrap_or_default();
    let coordinator = if members.iter().any(|m| m == &starred) {
        starred
    } else {
        members[0].clone()
    };

    // An ephemeral, uniquely-slugged team so concurrent quick chats don't clobber each other.
    let slug = format!("quick-{}", uuid::Uuid::new_v4());
    let name = if members.len() == 1 {
        format!("Quick chat: {}", members[0])
    } else {
        format!("Quick chat: {} masters", members.len())
    };
    state
        .agent
        .store()
        .upsert_team(&project_id, &slug, &name, "", &coordinator, &members)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let session = crate::group::start(&state, &project_id, &slug, Some(&name))
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(session))
}
