//! `getmastersd` daemon library: builds the axum app and owns the API surface.
//!
//! The `getmastersd` binary (`main.rs`) and the `gen_openapi` binary both depend on this lib,
//! and integration tests build the app in-process against the mock provider. Keeping the
//! router/handlers/OpenAPI here (not in `main`) is what makes the daemon testable headless.

pub mod acp;
pub mod auth;
pub mod bundle;
pub mod delivery;
pub mod group;
pub mod home;
pub mod install;
pub mod master;
pub mod master_templates;
pub mod openapi;
pub mod recipe;
pub mod routes;
pub mod scheduler;
pub mod state;
pub mod team;

use axum::routing::{get, post};
use axum::Router;

pub use openapi::ApiDoc;
pub use state::AppState;

/// Build the full application router for a given [`AppState`].
///
/// `/health` and `/openapi.json` are public; every other route sits behind the
/// per-launch bearer-token middleware (docs/06 §3).
pub fn build_app(state: AppState) -> Router {
    let protected = Router::new()
        .route(
            "/sessions",
            post(routes::sessions::create).get(routes::sessions::list),
        )
        .route(
            "/sessions/{id}/messages",
            get(routes::messages::list).post(routes::messages::send),
        )
        .route("/sessions/{id}/ws", get(routes::ws::handler))
        .route("/sessions/{id}/revert", post(routes::sessions::revert))
        .route("/sessions/{id}/audit", get(routes::sessions::list_audit))
        .route(
            "/projects",
            post(routes::projects::create).get(routes::projects::list),
        )
        .route("/projects/{id}", get(routes::projects::get))
        .route("/projects/{id}/grants", post(routes::projects::add_grant))
        .route(
            "/projects/{id}/instructions",
            axum::routing::put(routes::projects::set_instructions),
        )
        .route(
            "/projects/{id}/memories",
            get(routes::projects::list_memories),
        )
        .route("/projects/{id}/skills", get(routes::projects::list_skills))
        .route("/projects/{id}/decks", get(routes::projects::list_decks))
        .route(
            "/projects/{id}/study-plan",
            get(routes::projects::study_plan),
        )
        .route(
            "/projects/{id}/recipes",
            post(routes::recipes::save).get(routes::recipes::list),
        )
        .route("/projects/{id}/recipes/{name}", get(routes::recipes::get))
        .route(
            "/projects/{id}/recipes/{name}/run",
            post(routes::recipes::run),
        )
        .route(
            "/projects/{id}/masters",
            post(routes::masters::save).get(routes::masters::list),
        )
        .route(
            "/projects/{id}/masters/{slug}",
            get(routes::masters::get).delete(routes::masters::delete),
        )
        .route(
            "/projects/{id}/masters/{slug}/run",
            post(routes::masters::run),
        )
        .route(
            "/projects/{id}/teams",
            post(routes::teams::save).get(routes::teams::list),
        )
        .route(
            "/projects/{id}/teams/{slug}",
            get(routes::teams::get).delete(routes::teams::delete),
        )
        .route(
            "/projects/{id}/teams/{slug}/route",
            post(routes::teams::route),
        )
        .route("/projects/{id}/teams/{slug}/run", post(routes::teams::run))
        .route(
            "/projects/{id}/teams/{slug}/bundle",
            get(routes::bundles::export),
        )
        .route("/projects/{id}/bundles", post(routes::bundles::import))
        .route(
            "/projects/{id}/teams/{slug}/session",
            post(routes::group::start),
        )
        .route("/sessions/{id}/group", post(routes::group::post))
        // Standalone (global) masters — managed from the Masters sidebar, no project required.
        // Literal segments (`templates`/`default`/`quickchat`) take precedence over `{slug}`.
        .route(
            "/masters",
            post(routes::masters_global::save).get(routes::masters_global::list),
        )
        .route("/masters/templates", get(routes::masters_global::templates))
        .route(
            "/masters/default",
            get(routes::masters_global::get_default).put(routes::masters_global::set_default),
        )
        .route(
            "/masters/quickchat",
            post(routes::masters_global::quickchat),
        )
        .route(
            "/masters/{slug}",
            get(routes::masters_global::get).delete(routes::masters_global::delete),
        )
        .route("/acp/harnesses", get(routes::acp::harnesses))
        .route(
            "/projects/{id}/connectors",
            post(routes::connectors::save).get(routes::connectors::list),
        )
        .route(
            "/projects/{id}/connectors/{name}",
            axum::routing::put(routes::connectors::set_enabled).delete(routes::connectors::delete),
        )
        .route(
            "/projects/{id}/schedules",
            post(routes::schedules::create).get(routes::schedules::list),
        )
        .route(
            "/projects/{id}/schedules/{sid}",
            axum::routing::put(routes::schedules::set).delete(routes::schedules::delete),
        )
        .route(
            "/projects/{id}/schedules/{sid}/runs",
            get(routes::schedules::runs),
        )
        .route(
            "/projects/{id}/knowledge",
            get(routes::projects::knowledge_status),
        )
        .route(
            "/projects/{id}/extensions",
            get(routes::projects::list_extensions),
        )
        .route(
            "/projects/{id}/extensions/{name}",
            axum::routing::put(routes::projects::set_extension),
        )
        .route(
            "/settings",
            get(routes::settings::get).put(routes::settings::update),
        )
        .route("/settings/providers", get(routes::settings::providers))
        .route("/settings/environment", get(routes::settings::environment))
        .route("/settings/check", post(routes::settings::check))
        .route(
            "/settings/email",
            get(routes::settings::get_email).put(routes::settings::update_email),
        )
        .route(
            "/settings/secret",
            axum::routing::put(routes::settings::set_secret),
        )
        .route(
            "/settings/secret/{name}",
            axum::routing::delete(routes::settings::delete_secret),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_token,
        ));

    let app = Router::new()
        .route("/health", get(routes::health::health))
        .route("/openapi.json", get(routes::openapi_json))
        .merge(protected)
        .with_state(state);

    // Dev-only: in the packaged Tauri app the front-end and daemon share an origin, so no CORS
    // is needed. When developing the UI in a plain browser (e.g. `pnpm dev` on WSL/headless),
    // the page is served from Vite's port while the API lives on the daemon's port — a
    // cross-origin call that the browser blocks without these headers. Gated behind an env var
    // so the default (Tauri) posture is unchanged. The daemon is still loopback-only + bearer-
    // gated, so a permissive policy here only widens the browser-dev seam.
    if std::env::var("GETMASTERS_DEV_CORS").is_ok_and(|v| v == "1") {
        app.layer(tower_http::cors::CorsLayer::permissive())
    } else {
        app
    }
}
