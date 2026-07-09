//! Projects as context containers (ADR-0011): a project bundles instructions + folder grants
//! (+ its knowledge index), auto-injected into each session under it.

use axum::extract::{Path, State};
use axum::Json;

use axum::http::StatusCode;

use getmasters_proto::{
    AddGrantRequest, CreateProjectRequest, DeckDto, DocumentDto, ExtensionDto, FolderAccess,
    FolderGrant, KnowledgeStatusDto, MemoryDto, ProjectDto, SetExtensionRequest,
    SetInstructionsRequest, SkillDto, StudyPlanDto,
};

use crate::state::{AppError, AppState, ALL_BUILTIN_SERVERS, IMPLEMENTED_SERVERS};

#[utoipa::path(
    post,
    path = "/projects",
    operation_id = "create_project",
    request_body = CreateProjectRequest,
    responses((status = 200, description = "Project created", body = ProjectDto)),
    tag = "projects"
)]
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateProjectRequest>,
) -> Result<Json<ProjectDto>, AppError> {
    let store = state.agent.store();
    let id = store.create_project(&body.name, body.instructions.as_deref())?;
    Ok(Json(store.get_project(&id)?))
}

#[utoipa::path(
    get,
    path = "/projects",
    operation_id = "list_projects",
    responses((status = 200, description = "All projects", body = [ProjectDto])),
    tag = "projects"
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<ProjectDto>>, AppError> {
    Ok(Json(state.agent.store().list_projects()?))
}

#[utoipa::path(
    get,
    path = "/projects/{id}",
    operation_id = "get_project",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project", body = ProjectDto)),
    tag = "projects"
)]
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProjectDto>, AppError> {
    Ok(Json(state.agent.store().get_project(&id)?))
}

#[utoipa::path(
    post,
    path = "/projects/{id}/grants",
    operation_id = "add_project_grant",
    params(("id" = String, Path, description = "Project id")),
    request_body = AddGrantRequest,
    responses((status = 200, description = "Grant added", body = FolderGrant)),
    tag = "projects"
)]
pub async fn add_grant(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AddGrantRequest>,
) -> Result<Json<FolderGrant>, AppError> {
    state.agent.store().get_project(&id)?; // 404 if unknown
    let access = FolderAccess::from_str_lenient(&body.access);
    let grant = state
        .agent
        .store()
        .create_folder_grant(Some(&id), &body.path, access)?;
    state.invalidate_project(&id); // rebuild the project agent with the new grant
    Ok(Json(grant))
}

#[utoipa::path(
    put,
    path = "/projects/{id}/instructions",
    operation_id = "set_project_instructions",
    params(("id" = String, Path, description = "Project id")),
    request_body = SetInstructionsRequest,
    responses((status = 200, description = "Updated project", body = ProjectDto)),
    tag = "projects"
)]
pub async fn set_instructions(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetInstructionsRequest>,
) -> Result<Json<ProjectDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    store.set_project_instructions(&id, &body.instructions)?;
    state.invalidate_project(&id);
    Ok(Json(store.get_project(&id)?))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/memories",
    operation_id = "list_project_memories",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's durable memories", body = [MemoryDto])),
    tag = "projects"
)]
pub async fn list_memories(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<MemoryDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    let memories = store
        .list_memories(&id)?
        .into_iter()
        .map(|m| MemoryDto {
            title: m.title,
            body: m.body,
            scope: m.kind,
            source: m.source_file,
        })
        .collect();
    Ok(Json(memories))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/skills",
    operation_id = "list_project_skills",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's saved skills", body = [SkillDto])),
    tag = "projects"
)]
pub async fn list_skills(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<SkillDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    let skills = store
        .list_skills(&id)?
        .into_iter()
        .map(|s| SkillDto {
            slug: s.slug,
            name: s.name,
            summary: s.summary,
            tags: Vec::new(),
            steps: String::new(),
        })
        .collect();
    Ok(Json(skills))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/decks",
    operation_id = "list_project_decks",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's flashcard decks + due counts", body = [DeckDto])),
    tag = "projects"
)]
pub async fn list_decks(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<DeckDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let decks = store
        .list_decks(&id, now)?
        .into_iter()
        .map(|d| DeckDto {
            name: d.name,
            cards: d.card_count,
            due: d.due_count,
        })
        .collect();
    Ok(Json(decks))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/study-plan",
    operation_id = "get_project_study_plan",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's active study plan (null if none)", body = Option<StudyPlanDto>)),
    tag = "projects"
)]
pub async fn study_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Option<StudyPlanDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    let plan = store.get_study_plan(&id)?.map(|p| StudyPlanDto {
        title: p.title,
        deadline_at: p.deadline_at,
        body: p.body,
    });
    Ok(Json(plan))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/knowledge",
    operation_id = "get_project_knowledge",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "Knowledge index status + documents", body = KnowledgeStatusDto)),
    tag = "projects"
)]
pub async fn knowledge_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<KnowledgeStatusDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    let (documents, chunks, last_indexed_at) = store.knowledge_status(&id)?;
    let paths = store
        .list_documents(&id)?
        .into_iter()
        .map(|(path, mime, indexed_at)| DocumentDto {
            path,
            mime,
            indexed_at,
        })
        .collect();
    Ok(Json(KnowledgeStatusDto {
        documents,
        chunks,
        last_indexed_at,
        paths,
    }))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/extensions",
    operation_id = "list_project_extensions",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "Built-in servers + enabled state", body = [ExtensionDto])),
    tag = "projects"
)]
pub async fn list_extensions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ExtensionDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    let disabled = store.disabled_extensions(&id)?;
    let exts = ALL_BUILTIN_SERVERS
        .iter()
        .map(|name| {
            let implemented = IMPLEMENTED_SERVERS.contains(name);
            ExtensionDto {
                name: name.to_string(),
                // Placeholders are never hosted; implemented servers are on unless disabled.
                enabled: implemented && !disabled.contains(*name),
                implemented,
            }
        })
        .collect();
    Ok(Json(exts))
}

#[utoipa::path(
    put,
    path = "/projects/{id}/extensions/{name}",
    operation_id = "set_project_extension",
    params(
        ("id" = String, Path, description = "Project id"),
        ("name" = String, Path, description = "Built-in server name")
    ),
    request_body = SetExtensionRequest,
    responses((status = 200, description = "Updated extension state", body = ExtensionDto)),
    tag = "projects"
)]
pub async fn set_extension(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    Json(body): Json<SetExtensionRequest>,
) -> Result<Json<ExtensionDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    if !IMPLEMENTED_SERVERS.contains(&name.as_str()) {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            format!("unknown or unimplemented extension '{name}'"),
        ));
    }
    store.set_project_extension(&id, &name, body.enabled)?;
    state.invalidate_project(&id); // rebuild the project agent with the new server set
    Ok(Json(ExtensionDto {
        name,
        enabled: body.enabled,
        implemented: true,
    }))
}
