//! Master Teams + router (Phase 4b, FR-38/40): seed two masters + a team, route a brief (the
//! relevant master ranks first + is selected), run the team (dispatches the routed master on the
//! mock provider), and check a manual override bypasses the ranking. Plus an HTTP CRUD+route+run e2e.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::masters::{Master, MasterStore};
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_server::{team, AppState};

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-team-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn state_with(store: &Store, dir: &std::path::Path) -> AppState {
    let agent = AgentService::new(store.clone(), Arc::new(MockProvider::new()), "mock:base");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    AppState::new(agent, "tok".to_string()).with_config(cfg)
}

fn master(name: &str, summary: &str, model: &str) -> Master {
    Master {
        name: name.into(),
        summary: summary.into(),
        persona: format!("You are {name}."),
        default_model: model.into(),
        allowed_skills: vec![],
        allowed_tools: vec![],
        output_contract: String::new(),
        origin: "learned".into(),
        body: String::new(),
        backend: "internal".into(),
        acp: None,
    }
}

/// Seed an architect + a writer master and a team with the writer as coordinator.
fn seed(state: &AppState, store: &Store, pid: &str) {
    let es = MasterStore::new(state.project_dir(pid), pid.to_string(), store.clone());
    es.create(&master(
        "Backend Architect",
        "Designs API and database schema decisions.",
        "mock:opus",
    ))
    .unwrap();
    es.create(&master(
        "Copy Writer",
        "Drafts marketing prose.",
        "mock:haiku",
    ))
    .unwrap();
    store
        .upsert_team(
            pid,
            "squad",
            "Squad",
            "A small build team.",
            "copy-writer", // coordinator
            &["backend-architect".to_string(), "copy-writer".to_string()],
        )
        .unwrap();
}

#[tokio::test]
async fn route_ranks_and_selects_relevant_master() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);
    seed(&state, &store, &pid);

    let routed = team::route(&state, &pid, "squad", "design the API database schema").unwrap();
    assert_eq!(routed.ranked[0].slug, "backend-architect");
    assert_eq!(routed.selected_slug, "backend-architect");

    // A brief matching nothing falls back to the coordinator.
    let unmatched = team::route(&state, &pid, "squad", "zzzz qqqq").unwrap();
    assert_eq!(unmatched.selected_slug, "copy-writer");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn run_dispatches_routed_master_and_honors_override() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);
    seed(&state, &store, &pid);

    // Auto-route → the architect handles a schema brief.
    let run = team::run(&state, &pid, "squad", "design the schema", None)
        .await
        .unwrap();
    assert_eq!(run.selected_slug, "backend-architect");
    let session = store.get_session(&run.result.session_id).unwrap();
    assert_eq!(session.title.as_deref(), Some("master:backend-architect"));
    assert!(run.result.message.content.contains("design the schema"));

    // Manual override → the writer handles the same brief.
    let overridden = team::run(
        &state,
        &pid,
        "squad",
        "design the schema",
        Some("copy-writer"),
    )
    .await
    .unwrap();
    assert_eq!(overridden.selected_slug, "copy-writer");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn team_crud_route_run_over_http() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);
    seed(&state, &store, &pid);

    let app = getmasters_server::build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    // List the seeded team.
    let teams: Vec<getmasters_proto::TeamSummaryDto> = client
        .get(format!("{base}/projects/{pid}/teams"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0].member_count, 2);

    // Route over HTTP.
    let routed: getmasters_proto::RouteResultDto = client
        .post(format!("{base}/projects/{pid}/teams/squad/route"))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "brief": "design the API schema" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(routed.selected_slug, "backend-architect");

    // Run over HTTP.
    let run: getmasters_proto::TeamRunResult = client
        .post(format!("{base}/projects/{pid}/teams/squad/run"))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "brief": "design the API schema" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(run.selected_slug, "backend-architect");

    // Delete.
    let del = client
        .delete(format!("{base}/projects/{pid}/teams/squad"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), reqwest::StatusCode::NO_CONTENT);

    std::fs::remove_dir_all(&dir).ok();
}
