//! External MCP connector endpoints (Phase 4d, FR-20; ADR-0005): CRUD over a project's stdio MCP
//! servers. Each mutation invalidates the cached project agent so the next session rebuilds its
//! ExtensionManager with the new connector set. The connectors' tools are gated/audited like built-ins.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_core::store::ConnectorRow;
use getmasters_proto::{ConnectorDto, CreateConnectorRequest, SetConnectorEnabledRequest};

use crate::state::{AppError, AppState};

fn to_dto(c: ConnectorRow) -> ConnectorDto {
    ConnectorDto {
        name: c.name,
        command: c.command,
        args: c.args,
        env: c.env.into_iter().map(|(k, v)| [k, v]).collect(),
        enabled: c.enabled,
    }
}

fn env_pairs(env: &[[String; 2]]) -> Vec<(String, String)> {
    env.iter()
        .map(|kv| (kv[0].clone(), kv[1].clone()))
        .collect()
}

#[utoipa::path(
    post,
    path = "/projects/{id}/connectors",
    operation_id = "save_project_connector",
    params(("id" = String, Path, description = "Project id")),
    request_body = CreateConnectorRequest,
    responses((status = 200, description = "Connector saved", body = ConnectorDto)),
    tag = "projects"
)]
pub async fn save(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateConnectorRequest>,
) -> Result<Json<ConnectorDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    if body.name.trim().is_empty() || body.command.trim().is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "connector name and command are required".to_string(),
        ));
    }
    let env = env_pairs(&body.env);
    store.upsert_connector(
        &id,
        &body.name,
        &body.command,
        &body.args,
        &env,
        body.enabled,
    )?;
    state.invalidate_project(&id);
    Ok(Json(ConnectorDto {
        name: body.name,
        command: body.command,
        args: body.args,
        env: body.env,
        enabled: body.enabled,
    }))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/connectors",
    operation_id = "list_project_connectors",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's external connectors", body = [ConnectorDto])),
    tag = "projects"
)]
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ConnectorDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    let connectors = store
        .list_connectors(&id)?
        .into_iter()
        .map(to_dto)
        .collect();
    Ok(Json(connectors))
}

#[utoipa::path(
    put,
    path = "/projects/{id}/connectors/{name}",
    operation_id = "set_project_connector_enabled",
    params(
        ("id" = String, Path, description = "Project id"),
        ("name" = String, Path, description = "Connector name"),
    ),
    request_body = SetConnectorEnabledRequest,
    responses((status = 200, description = "Connector updated", body = ConnectorDto)),
    tag = "projects"
)]
pub async fn set_enabled(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    Json(body): Json<SetConnectorEnabledRequest>,
) -> Result<Json<ConnectorDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    store.set_connector_enabled(&id, &name, body.enabled)?;
    state.invalidate_project(&id);
    let row = store
        .get_connector(&id, &name)?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, format!("connector {name}")))?;
    Ok(Json(to_dto(row)))
}

#[utoipa::path(
    delete,
    path = "/projects/{id}/connectors/{name}",
    operation_id = "delete_project_connector",
    params(
        ("id" = String, Path, description = "Project id"),
        ("name" = String, Path, description = "Connector name"),
    ),
    responses((status = 204, description = "Connector deleted")),
    tag = "projects"
)]
pub async fn delete(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?;
    store.delete_connector(&id, &name)?;
    state.invalidate_project(&id);
    Ok(StatusCode::NO_CONTENT)
}
