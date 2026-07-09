//! Cloud catalog sync (Part C): applying a catalog installs global masters + skills, is
//! version-gated, and never clobbers a user-authored master that shares a slug.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::masters::{Master, MasterStore};
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::CatalogDto;
use getmasters_server::{catalog, AppState};

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-catalog-{}", uuid::Uuid::new_v4()));
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

fn catalog_json(version: &str) -> CatalogDto {
    serde_json::from_value(serde_json::json!({
        "version": version,
        "masters": [{
            "name": "Researcher",
            "summary": "Finds and cites sources.",
            "persona": "You are a meticulous researcher.",
            "default_model": "anthropic:claude-opus-4-8",
            "origin": "system",
            "body": "Do research.",
        }],
        "skills": [{
            "slug": "cite-sources",
            "name": "Cite Sources",
            "summary": "Add citations to claims.",
            "tags": ["research", "writing"],
            "steps": "1. find source\n2. cite",
        }],
    }))
    .unwrap()
}

#[test]
fn apply_installs_masters_and_skills_then_version_gates() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);

    // First apply installs both, and writes the source files under the data home.
    let status = catalog::apply_catalog(&state, catalog_json("v1"), false);
    assert_eq!(status.masters, 1);
    assert_eq!(status.skills, 1);
    assert_eq!(status.version.as_deref(), Some("v1"));
    assert!(dir.join("masters/researcher.md").exists());
    assert!(dir.join("skills/cite-sources.md").exists());

    // The synced skill round-trips its tags/body from the file.
    let skill = state
        .global_skill_store()
        .load("cite-sources")
        .unwrap()
        .unwrap();
    assert_eq!(
        skill.tags,
        vec!["research".to_string(), "writing".to_string()]
    );

    // Same version, not forced → no-op (still one of each, no duplicates).
    let again = catalog::apply_catalog(&state, catalog_json("v1"), false);
    assert_eq!(again.masters, 1);
    assert_eq!(again.skills, 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn sync_does_not_clobber_a_user_authored_master() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);

    // A user creates a global master whose slug collides with a catalog entry ("researcher").
    let user_master = Master {
        name: "Researcher".into(),
        summary: "MY custom researcher".into(),
        persona: "custom".into(),
        default_model: "anthropic:claude-opus-4-8".into(),
        allowed_skills: vec![],
        allowed_tools: vec![],
        output_contract: String::new(),
        origin: "imported".into(),
        body: "mine".into(),
        backend: "internal".into(),
        acp: None,
    };
    MasterStore::global(state.data_base(), store.clone())
        .create(&user_master)
        .unwrap();

    catalog::apply_catalog(&state, catalog_json("v1"), true);

    // The user's master is preserved (origin + summary unchanged) — the catalog skipped it.
    let after = state
        .global_master_store()
        .load("researcher")
        .unwrap()
        .unwrap();
    assert_eq!(after.origin, "imported");
    assert_eq!(after.summary, "MY custom researcher");

    std::fs::remove_dir_all(&dir).ok();
}
