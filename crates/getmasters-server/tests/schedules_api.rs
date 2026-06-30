//! Schedule endpoints (Phase 3d, FR-17): create a cron schedule, list it, toggle it off, read its
//! (empty) run history, and delete it — over the daemon.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{ProjectDto, RecipeDto, ScheduleDto, ScheduledRunDto};
use getmasters_server::{build_app, AppState};

const TOKEN: &str = "sched-token";

#[tokio::test]
async fn schedule_crud_over_http() {
    let dir = std::env::temp_dir().join(format!("getmasters-schedapi-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();

    let store = Store::open_in_memory().unwrap();
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock");
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

    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let project: ProjectDto = client
        .post(format!("{base}/projects"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "name": "auto" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // A schedule needs an existing recipe.
    let _: RecipeDto = client
        .post(format!("{base}/projects/{}/recipes", project.id))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({
            "name": "digest", "title": "Digest", "prompt": "summarize"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Creating a schedule for an unknown recipe is rejected.
    let bad = client
        .post(format!("{base}/projects/{}/schedules", project.id))
        .bearer_auth(TOKEN)
        .json(
            &serde_json::json!({ "recipe_name": "nope", "kind": "cron", "cron_expr": "0 9 * * *" }),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), reqwest::StatusCode::BAD_REQUEST);

    // Create a valid cron schedule.
    let sched: ScheduleDto = client
        .post(format!("{base}/projects/{}/schedules", project.id))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({
            "recipe_name": "digest", "kind": "cron", "cron_expr": "0 9 * * 1"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(sched.recipe_name, "digest");
    assert!(sched.enabled);
    assert!(sched.next_run_at.is_some(), "cron schedule has a next fire");

    // It lists.
    let all: Vec<ScheduleDto> = client
        .get(format!("{base}/projects/{}/schedules", project.id))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(all.len(), 1);

    // Disable it.
    let updated: ScheduleDto = client
        .put(format!(
            "{base}/projects/{}/schedules/{}",
            project.id, sched.id
        ))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "enabled": false }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!updated.enabled);

    // Run history is empty (it never fired).
    let runs: Vec<ScheduledRunDto> = client
        .get(format!(
            "{base}/projects/{}/schedules/{}/runs",
            project.id, sched.id
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(runs.is_empty());

    // Delete it.
    let del = client
        .delete(format!(
            "{base}/projects/{}/schedules/{}",
            project.id, sched.id
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), reqwest::StatusCode::NO_CONTENT);

    let after: Vec<ScheduleDto> = client
        .get(format!("{base}/projects/{}/schedules", project.id))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(after.is_empty());

    std::fs::remove_dir_all(&dir).ok();
}
