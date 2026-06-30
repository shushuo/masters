//! Multi-master group chat (Phase 4c, FR-43): start a group session from a 2-master team, then check
//! addressing — a single @-mention answers alone, no mention falls back to the coordinator, and @all
//! makes both reply (parallel snapshot) while the shared transcript stays clean (user + replies only).

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::masters::{Master, MasterStore};
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_server::{group, AppState};

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-group-{}", uuid::Uuid::new_v4()));
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

fn master(name: &str, summary: &str) -> Master {
    Master {
        name: name.into(),
        summary: summary.into(),
        persona: format!("You are {name}."),
        default_model: "mock:m".into(),
        allowed_skills: vec![],
        allowed_tools: vec![],
        output_contract: String::new(),
        origin: "learned".into(),
        body: String::new(),
        backend: "internal".into(),
        acp: None,
    }
}

/// Seed two masters + a team (coordinator = the writer) and return the project id.
fn seed(state: &AppState, store: &Store) -> String {
    let pid = store.create_project("p", None).unwrap();
    let es = MasterStore::new(state.project_dir(&pid), pid.clone(), store.clone());
    es.create(&master("Backend Architect", "Designs API and schema."))
        .unwrap();
    es.create(&master("Copy Writer", "Drafts prose.")).unwrap();
    store
        .upsert_team(
            &pid,
            "squad",
            "Squad",
            "",
            "copy-writer",
            &["backend-architect".to_string(), "copy-writer".to_string()],
        )
        .unwrap();
    pid
}

#[tokio::test]
async fn mention_addresses_one_master() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);
    let pid = seed(&state, &store);

    let session = group::start(&state, &pid, "squad", None).unwrap();
    let res = group::post(
        &state,
        &session.id,
        "@backend-architect design the schema",
        None,
    )
    .await
    .unwrap();

    assert_eq!(res.addressed, vec!["backend-architect"]);
    assert_eq!(res.replies.len(), 1);
    assert_eq!(res.replies[0].author, "backend-architect");
    assert_eq!(res.replies[0].role, "assistant");

    // The group transcript is clean: the user message + the one attributed reply (no tool scratch).
    let transcript = store.list_messages(&session.id).unwrap();
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0].author, "user");
    assert_eq!(transcript[1].author, "backend-architect");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn no_mention_falls_back_to_coordinator() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);
    let pid = seed(&state, &store);

    let session = group::start(&state, &pid, "squad", None).unwrap();
    let res = group::post(&state, &session.id, "what should we build?", None)
        .await
        .unwrap();
    assert_eq!(res.addressed, vec!["copy-writer"]);
    assert_eq!(res.replies[0].author, "copy-writer");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn all_addresses_everyone_in_parallel() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);
    let pid = seed(&state, &store);

    let session = group::start(&state, &pid, "squad", None).unwrap();
    // Cap at one round so the @all echo doesn't drive follow-ups — this checks round-0 addressing.
    let res = group::post(&state, &session.id, "@all kickoff", Some(1))
        .await
        .unwrap();
    assert_eq!(res.addressed, vec!["backend-architect", "copy-writer"]);
    assert_eq!(res.replies.len(), 2);
    let authors: Vec<&str> = res.replies.iter().map(|r| r.author.as_str()).collect();
    assert!(authors.contains(&"backend-architect"));
    assert!(authors.contains(&"copy-writer"));

    // Group transcript = the user message + the two attributed replies, nothing else.
    let transcript = store.list_messages(&session.id).unwrap();
    assert_eq!(transcript.len(), 3);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn followup_mentions_drive_bounded_rounds() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);
    let pid = seed(&state, &store);

    let session = group::start(&state, &pid, "squad", None).unwrap();
    // Round 0 addresses both; each reply echoes the *other's* @mention → a follow-up round. The mock
    // would ping-pong forever, so the cap (2) must stop it: 2 rounds × 2 masters = 4 replies.
    let res = group::post(
        &state,
        &session.id,
        "@backend-architect @copy-writer sync up",
        Some(2),
    )
    .await
    .unwrap();

    assert_eq!(res.addressed, vec!["backend-architect", "copy-writer"]);
    assert_eq!(res.replies.len(), 4, "two capped rounds × two masters");
    // Transcript: the user message + every round's attributed replies (1 + 4).
    let transcript = store.list_messages(&session.id).unwrap();
    assert_eq!(transcript.len(), 5);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn start_and_post_over_http() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);
    let pid = seed(&state, &store);

    let app = getmasters_server::build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let session: getmasters_proto::SessionDto = client
        .post(format!("{base}/projects/{pid}/teams/squad/session"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(session.team_slug.as_deref(), Some("squad"));

    let res: getmasters_proto::GroupPostResult = client
        .post(format!("{base}/sessions/{}/group", session.id))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "content": "@copy-writer draft the intro" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(res.addressed, vec!["copy-writer"]);
    assert_eq!(res.replies[0].author, "copy-writer");

    std::fs::remove_dir_all(&dir).ok();
}
