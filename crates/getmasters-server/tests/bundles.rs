//! Portable team/master bundles (Phase 4h; ADR-0010): export a team from one project as a
//! self-contained JSON bundle and import it into another — recreating the masters (files + DB rows)
//! and the team — then prove the imported team is functional (runs the routed master).

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::masters::{Master, MasterStore};
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_server::{bundle, team, AppState};

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-bundle-{}", uuid::Uuid::new_v4()));
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

/// Seed an architect + a writer master and a `squad` team (writer = coordinator) into `pid`.
fn seed(state: &AppState, store: &Store, pid: &str) {
    let es = MasterStore::new(state.project_dir(pid), pid.to_string(), store.clone());
    es.create(&master(
        "Backend Architect",
        "Designs API and schema.",
        "mock:opus",
    ))
    .unwrap();
    es.create(&master("Copy Writer", "Drafts prose.", "mock:haiku"))
        .unwrap();
    store
        .upsert_team(
            pid,
            "squad",
            "Squad",
            "A small build team.",
            "copy-writer",
            &["backend-architect".to_string(), "copy-writer".to_string()],
        )
        .unwrap();
}

#[tokio::test]
async fn export_then_import_round_trips_team_and_masters() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);

    // Project A: the source of the bundle.
    let a = store.create_project("A", None).unwrap();
    seed(&state, &store, &a);

    // Export captures the team fields + both masters in full.
    let bundle = bundle::export(&state, &a, "squad").unwrap();
    assert_eq!(bundle.version, 1);
    assert_eq!(bundle.name, "Squad");
    assert_eq!(bundle.coordinator_slug, "copy-writer");
    assert_eq!(bundle.members, vec!["backend-architect", "copy-writer"]);
    assert_eq!(bundle.masters.len(), 2);
    let arch = bundle
        .masters
        .iter()
        .find(|e| e.slug == "backend-architect")
        .unwrap();
    assert_eq!(arch.default_model, "mock:opus");
    assert!(arch.persona.contains("Backend Architect"));

    // Project B: import into a fresh project (no masters/team of its own).
    let b = store.create_project("B", None).unwrap();
    let result = bundle::import(&state, &b, bundle).unwrap();
    assert_eq!(result.team_slug, "squad");
    assert_eq!(result.masters.len(), 2);

    // Masters were recreated in B as files + DB rows, with persona/model intact.
    let es_b = MasterStore::new(state.project_dir(&b), b.clone(), store.clone());
    let imported = es_b.load("backend-architect").unwrap().unwrap();
    assert_eq!(imported.default_model, "mock:opus");
    assert!(imported.persona.contains("Backend Architect"));
    assert!(state
        .project_dir(&b)
        .join("masters/backend-architect.md")
        .exists());

    // The team was recreated in B with the right coordinator + members.
    let team_b = store.get_team(&b, "squad").unwrap().unwrap();
    assert_eq!(team_b.coordinator_slug, "copy-writer");
    assert_eq!(team_b.members, vec!["backend-architect", "copy-writer"]);

    // And the imported team is functional: a brief dispatches the routed master.
    let run = team::run(&state, &b, "squad", "design the schema", None)
        .await
        .unwrap();
    assert!(!run.selected_slug.is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn import_is_idempotent_overwrite() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);
    let a = store.create_project("A", None).unwrap();
    seed(&state, &store, &a);
    let bundle = bundle::export(&state, &a, "squad").unwrap();

    let b = store.create_project("B", None).unwrap();
    let first = bundle::import(&state, &b, bundle.clone()).unwrap();
    // Re-importing the same bundle overwrites (upsert) — no error, same slugs.
    let second = bundle::import(&state, &b, bundle).unwrap();
    assert_eq!(first.team_slug, second.team_slug);
    assert_eq!(first.masters, second.masters);
    // Still exactly two masters in B (overwritten, not duplicated).
    assert_eq!(store.list_masters(&b).unwrap().len(), 2);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn import_over_http() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);
    let a = store.create_project("A", None).unwrap();
    seed(&state, &store, &a);
    let b = store.create_project("B", None).unwrap();

    let app = getmasters_server::build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let bundle: getmasters_proto::TeamBundle = client
        .get(format!("{base}/projects/{a}/teams/squad/bundle"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(bundle.masters.len(), 2);

    let result: getmasters_proto::BundleImportResult = client
        .post(format!("{base}/projects/{b}/bundles"))
        .bearer_auth("tok")
        .json(&bundle)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result.team_slug, "squad");
    assert_eq!(result.masters.len(), 2);

    std::fs::remove_dir_all(&dir).ok();
}
