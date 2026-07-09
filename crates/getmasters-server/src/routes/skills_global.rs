//! **Standalone (global) skill endpoints** — read + delete over `<data_home>/skills/` files that
//! exist independent of any project. These are **system skills synced from the cloud catalog**
//! ([`crate::catalog`]); there is no user-facing create here (project skills are still authored by
//! the agent via the `create_skill` MCP tool). Mirrors [`crate::routes::masters_global`], keyed on
//! `slug` alone.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_core::store::SkillRow;
use getmasters_proto::SkillDto;

use crate::state::{AppError, AppState};

/// Map an index row to the list DTO (name/summary only; steps/tags omitted in listings).
fn summary_dto(r: SkillRow) -> SkillDto {
    SkillDto {
        slug: r.slug,
        name: r.name,
        summary: r.summary,
        tags: Vec::new(),
        steps: String::new(),
    }
}

#[utoipa::path(
    get,
    path = "/skills",
    operation_id = "list_global_skills",
    responses((status = 200, description = "Standalone (system) skills", body = [SkillDto])),
    tag = "skills"
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<SkillDto>>, AppError> {
    let skills = state
        .global_skill_store()
        .list()
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .into_iter()
        .map(summary_dto)
        .collect();
    Ok(Json(skills))
}

#[utoipa::path(
    get,
    path = "/skills/{slug}",
    operation_id = "get_global_skill",
    params(("slug" = String, Path, description = "Skill slug")),
    responses(
        (status = 200, description = "The skill (full definition)", body = SkillDto),
        (status = 404, description = "No such skill")
    ),
    tag = "skills"
)]
pub async fn get(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<SkillDto>, AppError> {
    // The file is the source of truth — load it so `tags`/`steps` are populated.
    let skill = state
        .global_skill_store()
        .load(&slug)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "no such skill".to_string()))?;
    Ok(Json(SkillDto {
        slug: getmasters_core::skills::slugify(&skill.name),
        name: skill.name,
        summary: skill.summary,
        tags: skill.tags,
        steps: skill.body,
    }))
}

#[utoipa::path(
    delete,
    path = "/skills/{slug}",
    operation_id = "delete_global_skill",
    params(("slug" = String, Path, description = "Skill slug")),
    responses((status = 204, description = "Deleted")),
    tag = "skills"
)]
pub async fn delete(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .global_skill_store()
        .delete(&slug)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}
