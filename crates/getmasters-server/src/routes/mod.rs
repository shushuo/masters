//! HTTP + WebSocket route handlers.

pub mod acp;
pub mod bundles;
pub mod catalog;
pub mod connectors;
pub mod group;
pub mod health;
pub mod investing;
pub mod masters;
pub mod masters_global;
pub mod messages;
pub mod projects;
pub mod recipes;
pub mod schedules;
pub mod sessions;
pub mod settings;
pub mod skills_global;
pub mod teams;
pub mod ws;

use axum::Json;

use crate::openapi::ApiDoc;
use utoipa::OpenApi;

/// Serve the daemon's OpenAPI description (dev aid; the file emitted by `gen_openapi` is the
/// source for TypeScript client generation).
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}
