//! Standalone (global) masters + quick chat (Masters sidebar). Asserts: project-less CRUD over
//! `<data_home>/masters/`, the built-in template gallery, the starred default master, that a global
//! master runs through the project dispatch path via the `load_master_any` fallback, and that quick
//! chat creates a team-bound session over an ad-hoc master set.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_server::{build_app, AppState};

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-global-it-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

async fn serve() -> (String, std::path::PathBuf) {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock:base");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    let app = build_app(AppState::new(agent, "tok".to_string()).with_config(cfg));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://127.0.0.1:{port}"), dir)
}

#[tokio::test]
async fn global_master_crud_templates_default_and_quickchat() {
    let (base, dir) = serve().await;
    let client = reqwest::Client::new();

    // Templates gallery is non-empty and well-formed.
    let templates: Vec<getmasters_proto::MasterDto> = client
        .get(format!("{base}/masters/templates"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(templates.len() >= 4);
    assert!(templates.iter().all(|m| m.origin == "builtin"));

    // Create a global master (no project) by cloning a template.
    let mut tpl = templates[0].clone();
    tpl.default_model = "mock:master-x".to_string();
    let created: getmasters_proto::MasterDto = client
        .post(format!("{base}/masters"))
        .bearer_auth("tok")
        .json(&tpl)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!created.slug.is_empty());
    let slug = created.slug.clone();

    // List + get reflect it.
    let all: Vec<getmasters_proto::MasterSummaryDto> = client
        .get(format!("{base}/masters"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].slug, slug);

    // Star it as the default master.
    let put = client
        .put(format!("{base}/masters/default"))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "slug": slug }))
        .send()
        .await
        .unwrap();
    assert!(put.status().is_success());
    let def: getmasters_proto::DefaultMasterDto = client
        .get(format!("{base}/masters/default"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(def.slug, slug);

    // Quick chat over the single global master → a team-bound session (default project created).
    let session: getmasters_proto::SessionDto = client
        .post(format!("{base}/masters/quickchat"))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "masters": [slug] }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        session.team_slug.is_some(),
        "quick chat session is team-bound"
    );
    assert!(
        session.project_id.is_some(),
        "runs under the default project"
    );

    // Posting to the group session dispatches the global master (load_master_any fallback) and
    // returns its attributed reply — proving a project-less master runs via the project path.
    let result: getmasters_proto::GroupPostResult = client
        .post(format!("{base}/sessions/{}/group", session.id))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "content": "hello there" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result.addressed, vec![slug.clone()]);
    assert_eq!(result.replies.len(), 1);
    assert_eq!(result.replies[0].author, slug);
    assert!(result.replies[0].content.contains("hello there"));

    // Delete removes it.
    let del = client
        .delete(format!("{base}/masters/{slug}"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), reqwest::StatusCode::NO_CONTENT);
    let after: Vec<getmasters_proto::MasterSummaryDto> = client
        .get(format!("{base}/masters"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(after.is_empty());

    std::fs::remove_dir_all(&dir).ok();
}
