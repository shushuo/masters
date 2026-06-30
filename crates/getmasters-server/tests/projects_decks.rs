//! `GET /projects/{id}/decks` contract: seed decks/cards in the store, then confirm the daemon
//! reports each deck with its card + due counts (Phase 3a, FR-13/14).

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{DeckDto, ProjectDto};
use getmasters_server::{build_app, AppState};

const TOKEN: &str = "decks-token";

#[tokio::test]
async fn lists_decks_with_due_counts() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("course", None).unwrap();

    // One deck, two cards. Both are created due immediately, then one is scheduled far out.
    let deck = store.upsert_deck(&pid, "Chapter 1", None).unwrap();
    let c1 = store.add_card(&deck, &pid, "2+2?", "4", "qa").unwrap();
    store
        .add_card(&deck, &pid, "H2O is ___", "water", "cloze")
        .unwrap();
    // Push c1's due date a year out so only one card remains due.
    let far = 4_000_000_000_000;
    store
        .update_card_schedule(&c1, 2.6, 365, 1, 0, far)
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

    // Sanity: the project exists over HTTP.
    let project: ProjectDto = client
        .get(format!("{base}/projects/{pid}"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(project.name, "course");

    let decks: Vec<DeckDto> = client
        .get(format!("{base}/projects/{pid}/decks"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(decks.len(), 1);
    assert_eq!(decks[0].name, "Chapter 1");
    assert_eq!(decks[0].cards, 2);
    assert_eq!(
        decks[0].due, 1,
        "one card was scheduled out of the due window"
    );

    // Unknown project → 404.
    let missing = client
        .get(format!("{base}/projects/nope/decks"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
}
