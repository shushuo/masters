//! Project-session e2e (ADR-0006/0007): under a project, the agent remembers a fact and recalls
//! it, and authors a skill then recalls it — over the daemon, mock provider, auto-approved. Proves
//! the file-backed Memory + Skills servers are hosted, gated, and round-trip through the index.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{MessageDto, ProjectDto, SessionDto};
use getmasters_server::{build_app, AppState};

const TOKEN: &str = "mem-token";

async fn spawn() -> (u16, std::path::PathBuf) {
    // A temp DB path so the per-project data dir (derived from it) lands under temp, not cwd.
    let tmp = std::env::temp_dir().join(format!("getmasters-memtest-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = Config {
        db_path: tmp.join("getmasters.db"),
        ..Config::default()
    };

    let store = Store::open_in_memory().unwrap();
    // No approval registry → AutoApprover, so writes (remember/create_skill) don't block.
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock");
    let app = build_app(AppState::new(agent, TOKEN.to_string()).with_config(cfg));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (port, tmp)
}

async fn send(client: &reqwest::Client, base: &str, session: &str, content: &str) -> MessageDto {
    client
        .post(format!("{base}/sessions/{session}/messages"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "content": content }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn project_session_remembers_recalls_and_learns_skills() {
    let (port, tmp) = spawn().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

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

    // Remember a fact, then recall it (the mock summarizes the tool result).
    let remembered = send(
        &client,
        &base,
        &session.id,
        "[[tool:memory.remember|title=Deadline|content=The thesis is due in March]]",
    )
    .await;
    assert!(
        remembered.content.contains("remembered"),
        "remember reply: {}",
        remembered.content
    );

    let recalled = send(
        &client,
        &base,
        &session.id,
        "[[tool:memory.recall|query=deadline]]",
    )
    .await;
    assert!(
        recalled.content.contains("Deadline"),
        "recall should surface the remembered item: {}",
        recalled.content
    );

    // The file-backed truth exists on disk under the project data dir.
    let mem_file = tmp.join("projects").join(&project.id).join("MEMORY.md");
    assert!(mem_file.exists(), "MEMORY.md should exist at {mem_file:?}");

    // Author a skill, then recall it.
    let learned = send(
        &client,
        &base,
        &session.id,
        "[[tool:skills.create_skill|name=Summarize PDF|summary=bullet notes|steps=read then outline]]",
    )
    .await;
    assert!(
        learned.content.contains("created skill"),
        "create_skill reply: {}",
        learned.content
    );

    let recalled_skill = send(
        &client,
        &base,
        &session.id,
        "[[tool:skills.recall_skill|query=summarize pdf]]",
    )
    .await;
    assert!(
        recalled_skill.content.contains("Summarize PDF"),
        "recall_skill should surface the skill: {}",
        recalled_skill.content
    );

    std::fs::remove_dir_all(&tmp).ok();
}
