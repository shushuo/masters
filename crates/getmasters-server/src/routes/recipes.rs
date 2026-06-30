//! Recipe endpoints (Phase 3c, FR-16): CRUD over a project's `recipes/<name>.yaml` files plus an
//! on-demand "run now" that seeds the agent loop. Running is headless (auto-approved within the
//! project's grants, fully audited) — the Scheduler (3d) and delivery (3e) build on this.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_proto::{RecipeDto, RecipeRunResult, RecipeSummaryDto, RunRecipeRequest};

use crate::recipe::RecipeStore;
use crate::state::{AppError, AppState};

/// The project's recipe store (files under the project data dir + the DB index).
fn recipe_store(state: &AppState, project_id: &str) -> RecipeStore {
    RecipeStore::new(
        state.project_dir(project_id),
        project_id.to_string(),
        state.agent.store().clone(),
    )
}

#[utoipa::path(
    post,
    path = "/projects/{id}/recipes",
    operation_id = "save_project_recipe",
    params(("id" = String, Path, description = "Project id")),
    request_body = RecipeDto,
    responses((status = 200, description = "Recipe saved (with its canonical name)", body = RecipeDto)),
    tag = "projects"
)]
pub async fn save(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RecipeDto>,
) -> Result<Json<RecipeDto>, AppError> {
    state.agent.store().get_project(&id)?; // 404 if unknown
    let stored = recipe_store(&state, &id)
        .save(&body)
        .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(stored))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/recipes",
    operation_id = "list_project_recipes",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's recipes", body = [RecipeSummaryDto])),
    tag = "projects"
)]
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<RecipeSummaryDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    let recipes = store
        .list_recipes(&id)?
        .into_iter()
        .map(|r| RecipeSummaryDto {
            name: r.name,
            title: r.title,
            description: r.description,
        })
        .collect();
    Ok(Json(recipes))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/recipes/{name}",
    operation_id = "get_project_recipe",
    params(
        ("id" = String, Path, description = "Project id"),
        ("name" = String, Path, description = "Recipe name"),
    ),
    responses((status = 200, description = "The full recipe", body = RecipeDto)),
    tag = "projects"
)]
pub async fn get(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
) -> Result<Json<RecipeDto>, AppError> {
    state.agent.store().get_project(&id)?;
    let recipe = recipe_store(&state, &id)
        .load(&name)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, format!("recipe {name}")))?;
    Ok(Json(recipe))
}

#[utoipa::path(
    post,
    path = "/projects/{id}/recipes/{name}/run",
    operation_id = "run_project_recipe",
    params(
        ("id" = String, Path, description = "Project id"),
        ("name" = String, Path, description = "Recipe name"),
    ),
    request_body = RunRecipeRequest,
    responses((status = 200, description = "Run result (session + final message)", body = RecipeRunResult)),
    tag = "projects"
)]
pub async fn run(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    Json(body): Json<RunRecipeRequest>,
) -> Result<Json<RecipeRunResult>, AppError> {
    state.agent.store().get_project(&id)?;
    let recipe = recipe_store(&state, &id)
        .load(&name)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, format!("recipe {name}")))?;

    // Headless run, shared with the Scheduler: auto-approved within the project's grants (audited).
    let result = crate::recipe::run_loaded(&state, &id, &recipe, &body.params)
        .await
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(result))
}
