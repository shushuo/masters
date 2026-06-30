//! `GET /projects/{id}/study-plan` contract: seed a plan in the store, then confirm the daemon
//! returns it (and `null` when absent / 404 for an unknown project) (Phase 3b, FR-15).

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::StudyPlanDto;
use getmasters_server::{build_app, AppState};

const TOKEN: &str = "plan-token";

#[tokio::test]
async fn returns_study_plan_or_null() {
    let store = Store::open_in_memory().unwrap();
    let with_plan = store.create_project("exam", None).unwrap();
    let without_plan = store.create_project("empty", None).unwrap();

    let deadline = 4_000_000_000_000;
    store
        .upsert_study_plan(
            &with_plan,
            "Final exam",
            deadline,
            "Day 1: review weak decks",
        )
        .unwrap();

    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock");
    let app = build_app(AppState::new(agent, TOKEN.to_string()));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    // Project with a plan → the DTO.
    let plan: Option<StudyPlanDto> = client
        .get(format!("{base}/projects/{with_plan}/study-plan"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let plan = plan.expect("a plan was seeded");
    assert_eq!(plan.title, "Final exam");
    assert_eq!(plan.deadline_at, deadline);
    assert!(plan.body.contains("weak decks"));

    // Project without a plan → null.
    let none: Option<StudyPlanDto> = client
        .get(format!("{base}/projects/{without_plan}/study-plan"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(none.is_none());

    // Unknown project → 404.
    let missing = client
        .get(format!("{base}/projects/nope/study-plan"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
}
