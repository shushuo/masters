//! Project session e2e: create a project, grant a folder, ingest it, then ask a question and
//! get a citation — over the daemon, mock provider + mock embedder, auto-approved.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{MessageDto, ProjectDto, SessionDto};
use getmasters_server::{build_app, AppState};

const TOKEN: &str = "proj-token";

fn corpus() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-proj-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("rust.md"),
        "# Rust\nRust is a systems language with ownership and borrowing.",
    )
    .unwrap();
    dir.canonicalize().unwrap()
}

async fn spawn() -> u16 {
    let store = Store::open_in_memory().unwrap();
    // No approval registry → AutoApprover, so ingest (write) doesn't block the HTTP call.
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock");
    let app = build_app(AppState::new(agent, TOKEN.to_string()));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

#[tokio::test]
async fn project_session_ingests_and_answers_with_citation() {
    let dir = corpus();
    let port = spawn().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    // Create a project.
    let project: ProjectDto = client
        .post(format!("{base}/projects"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "name": "course" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Grant the corpus folder (read).
    let r = client
        .post(format!("{base}/projects/{}/grants", project.id))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "path": dir.to_string_lossy(), "access": "read" }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // A session under the project.
    let session: SessionDto = client
        .post(format!("{base}/sessions"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "project_id": project.id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Message 1: ingest the folder (mock trigger).
    let ingest: MessageDto = client
        .post(format!("{base}/sessions/{}/messages", session.id))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({
            "content": format!("[[tool:knowledge.ingest|path={}]]", dir.to_string_lossy())
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        ingest.content.contains("indexed"),
        "ingest reply: {}",
        ingest.content
    );

    // Message 2: ask — the model searches and the result carries the citation.
    let answer: MessageDto = client
        .post(format!("{base}/sessions/{}/messages", session.id))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({
            "content": "[[tool:knowledge.search|query=ownership]]"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        answer.content.contains("rust.md"),
        "answer should cite the source: {}",
        answer.content
    );
}
