//! FR-19 e2e: a project hosts all built-ins by default; disabling one (memory) makes its tools
//! disappear from the next session while the others keep working — over the daemon, mock provider.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{ExtensionDto, MessageDto, ProjectDto, SessionDto};
use getmasters_server::{build_app, AppState};

const TOKEN: &str = "ext-token";

async fn spawn() -> (u16, std::path::PathBuf) {
    let tmp = std::env::temp_dir().join(format!("getmasters-exttest-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = Config {
        db_path: tmp.join("getmasters.db"),
        ..Config::default()
    };
    let store = Store::open_in_memory().unwrap();
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

async fn new_session(client: &reqwest::Client, base: &str, project_id: &str) -> SessionDto {
    client
        .post(format!("{base}/sessions"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "project_id": project_id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn disabling_a_builtin_removes_its_tools_from_the_next_session() {
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

    // By default memory is enabled + implemented.
    let exts: Vec<ExtensionDto> = client
        .get(format!("{base}/projects/{}/extensions", project.id))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mem = exts.iter().find(|e| e.name == "memory").unwrap();
    assert!(mem.enabled && mem.implemented);
    // Placeholders are listed but not implemented.
    assert!(exts.iter().any(|e| e.name == "web" && !e.implemented));

    // Memory tool works while enabled.
    let s1 = new_session(&client, &base, &project.id).await;
    let ok = send(
        &client,
        &base,
        &s1.id,
        "[[tool:memory.remember|title=Deadline|content=due in March]]",
    )
    .await;
    assert!(ok.content.contains("remembered"), "got: {}", ok.content);

    // Disable memory.
    let updated: ExtensionDto = client
        .put(format!("{base}/projects/{}/extensions/memory", project.id))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "enabled": false }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!updated.enabled);

    // A fresh session no longer exposes memory.remember (not hosted → gate denies as not enabled),
    // while files/knowledge still work.
    let s2 = new_session(&client, &base, &project.id).await;
    let denied = send(
        &client,
        &base,
        &s2.id,
        "[[tool:memory.remember|title=X|content=Y]]",
    )
    .await;
    assert!(
        !denied.content.contains("remembered"),
        "memory should be unavailable after disabling: {}",
        denied.content
    );

    // knowledge.status still works (knowledge stays enabled).
    let kn = send(&client, &base, &s2.id, "[[tool:knowledge.status]]").await;
    assert!(
        kn.content.contains("documents") || kn.content.contains("backend"),
        "knowledge should still work: {}",
        kn.content
    );

    std::fs::remove_dir_all(&tmp).ok();
}
