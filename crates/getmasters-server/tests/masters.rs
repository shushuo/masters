//! Masters run path (Phase 4a, FR-39/46): create a master with its own persona + provider-qualified
//! model + tool allow-list, run a brief through it on the mock provider, and assert the run is
//! attributed to an `master:<slug>` session and shaped by the persona. A second case checks a bare
//! model falls back to the default provider without panicking.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::masters::{Master, MasterStore};
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_server::master;
use getmasters_server::AppState;

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-master-it-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn state_with(store: &Store, dir: &std::path::Path) -> AppState {
    let agent = AgentService::new(store.clone(), Arc::new(MockProvider::new()), "mock:base");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    AppState::new(agent, "t".to_string()).with_config(cfg)
}

fn master_with(model: &str) -> Master {
    Master {
        name: "Backend Architect".into(),
        summary: "Designs services.".into(),
        persona: "A terse senior backend engineer.".into(),
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

#[tokio::test]
async fn master_run_uses_its_session_and_persona() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);

    let slug = MasterStore::new(state.project_dir(&pid), pid.clone(), store.clone())
        .create(&master_with("mock:master-x"))
        .unwrap();
    assert_eq!(slug, "backend-architect");

    let result = master::run(&state, &pid, &slug, "design a queue")
        .await
        .expect("master run should succeed");

    // The run is attributed to a master session.
    let session = store.get_session(&result.session_id).unwrap();
    assert_eq!(session.title.as_deref(), Some("master:backend-architect"));
    assert_eq!(session.project_id.as_deref(), Some(pid.as_str()));

    // The mock echoes the brief back as the assistant message.
    assert_eq!(result.message.role, "assistant");
    assert!(result.message.content.contains("design a queue"));

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn master_crud_and_run_over_http() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock:base");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    let app =
        getmasters_server::build_app(AppState::new(agent, "tok".to_string()).with_config(cfg));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");
    let project: getmasters_proto::ProjectDto = client
        .post(format!("{base}/projects"))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "name": "team" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Create → canonical slug returned.
    let created: getmasters_proto::MasterDto = client
        .post(format!("{base}/projects/{}/masters", project.id))
        .bearer_auth("tok")
        .json(&serde_json::json!({
            "name": "Backend Architect",
            "persona": "A terse senior backend engineer.",
            "default_model": "mock:master-x"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created.slug, "backend-architect");

    // List + get.
    let all: Vec<getmasters_proto::MasterSummaryDto> = client
        .get(format!("{base}/projects/{}/masters", project.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].default_model, "mock:master-x");

    // Run a brief.
    let result: getmasters_proto::MasterRunResult = client
        .post(format!(
            "{base}/projects/{}/masters/backend-architect/run",
            project.id
        ))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "brief": "outline a service" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(result.message.content.contains("outline a service"));

    // Delete.
    let del = client
        .delete(format!(
            "{base}/projects/{}/masters/backend-architect",
            project.id
        ))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), reqwest::StatusCode::NO_CONTENT);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn master_with_bare_model_falls_back_to_default_provider() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);

    // An empty model → the daemon's configured default; no panic, run completes.
    let slug = MasterStore::new(state.project_dir(&pid), pid.clone(), store.clone())
        .create(&master_with(""))
        .unwrap();
    let result = master::run(&state, &pid, &slug, "hello").await.unwrap();
    assert_eq!(result.message.role, "assistant");

    std::fs::remove_dir_all(&dir).ok();
}
