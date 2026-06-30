//! Recipe e2e (Phase 3c, FR-16): create a recipe, fetch it back, then run it — the run drives the
//! agent loop (mock provider) and, auto-approved within a folder grant, performs the file write the
//! recipe's prompt requests. Verifies CRUD + run-now end to end over the daemon.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{ProjectDto, RecipeDto, RecipeRunResult, RecipeSummaryDto};
use getmasters_server::{build_app, AppState};

const TOKEN: &str = "recipe-token";

async fn spawn() -> (u16, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("getmasters-recipe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();

    let store = Store::open_in_memory().unwrap();
    // No approval registry on the base agent → headless runs auto-approve.
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock");
    // Point the per-project data dir (where recipes/<name>.yaml lives) inside the temp dir.
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    let app = build_app(AppState::new(agent, TOKEN.to_string()).with_config(cfg));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (port, dir)
}

#[tokio::test]
async fn create_fetch_and_run_recipe() {
    let (port, dir) = spawn().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let project: ProjectDto = client
        .post(format!("{base}/projects"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "name": "automation" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Grant write to a temp folder so the recipe's file write is allowed.
    let r = client
        .post(format!("{base}/projects/{}/grants", project.id))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "path": dir.to_string_lossy(), "access": "read_write" }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // Create a recipe whose prompt, once substituted, is a mock tool-trigger that writes a file.
    let out_path = dir.join("note.txt");
    let prompt = format!(
        "[[tool:files.create|path={}|content=hello {{{{who}}}}]]",
        out_path.to_string_lossy()
    );
    let saved: RecipeDto = client
        .post(format!("{base}/projects/{}/recipes", project.id))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({
            "name": "Greeting Note",
            "title": "Greeting Note",
            "description": "Write a greeting file",
            "parameters": [{ "key": "who", "default": "world" }],
            "prompt": prompt,
            "extensions": ["files"],
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved.name, "greeting-note", "name is slugified");

    // It shows up in the list.
    let recipes: Vec<RecipeSummaryDto> = client
        .get(format!("{base}/projects/{}/recipes", project.id))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(recipes.len(), 1);
    assert_eq!(recipes[0].name, "greeting-note");

    // Run it with a param override.
    let result: RecipeRunResult = client
        .post(format!(
            "{base}/projects/{}/recipes/greeting-note/run",
            project.id
        ))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "params": { "who": "getmasters" } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!result.session_id.is_empty());
    assert!(!result.message.id.is_empty());

    // The run performed the file write (auto-approved within the grant).
    let written = std::fs::read_to_string(&out_path).expect("recipe run should create the file");
    assert_eq!(written, "hello getmasters");

    std::fs::remove_dir_all(&dir).ok();
}
