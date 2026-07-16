#![cfg(feature = "testing")]
//! Knowledge ingest → search end-to-end over the mock embedder (no key, no network).
//! Hybrid retrieval ranks the right chunk via FTS even though the mock embedding is weak.

use std::sync::Arc;

use getmasters_core::config::Config;
use getmasters_core::knowledge::vector::BruteForceIndex;
use getmasters_core::knowledge::{ingest_path, search, Embedder};
use getmasters_core::permission::GrantSet;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{FolderAccess, FolderGrant};

fn temp_corpus() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-kn-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("rust.md"),
        "# Rust\nRust is a systems programming language with ownership and borrowing.",
    )
    .unwrap();
    std::fs::write(
        dir.join("cooking.md"),
        "# Cooking\nPasta needs boiling water and a pinch of salt.",
    )
    .unwrap();
    dir.canonicalize().unwrap()
}

fn grant_for(dir: &std::path::Path) -> FolderGrant {
    FolderGrant {
        id: "g".into(),
        project_id: None,
        path: dir.to_string_lossy().into_owned(),
        access: FolderAccess::Read,
        created_at: 0,
    }
}

#[tokio::test]
async fn ingest_then_search_returns_cited_chunk() {
    let dir = temp_corpus();
    let store = Store::open_in_memory().unwrap();
    let project = store.create_project("course", None).unwrap();
    let grants = GrantSet::new(vec![grant_for(&dir)]);
    let embedder = Embedder::from_provider(Arc::new(MockProvider::new()), 8);
    let index = BruteForceIndex::new(store.clone(), 8);
    let registry = getmasters_core::knowledge::extract::ExtractorRegistry::default_set();

    let r = ingest_path(
        &store,
        &project,
        &grants,
        &embedder,
        &index,
        &registry,
        &dir.to_string_lossy(),
    )
    .await
    .unwrap();
    assert_eq!(r.indexed, 2);
    assert!(r.chunks >= 2);

    let hits = search(&store, &project, &embedder, &index, "ownership", 3)
        .await
        .unwrap();
    assert!(!hits.is_empty(), "expected a hit for 'ownership'");
    assert!(
        hits[0].path.ends_with("rust.md"),
        "top hit: {:?}",
        hits[0].path
    );
    assert_eq!(hits[0].location.as_deref(), Some("heading: Rust"));

    // Re-ingest unchanged → everything skipped.
    let r2 = ingest_path(
        &store,
        &project,
        &grants,
        &embedder,
        &index,
        &registry,
        &dir.to_string_lossy(),
    )
    .await
    .unwrap();
    assert_eq!(r2.indexed, 0);
    assert_eq!(r2.skipped, 2);
}

#[tokio::test]
async fn search_is_project_scoped() {
    let dir = temp_corpus();
    let store = Store::open_in_memory().unwrap();
    let project_a = store.create_project("a", None).unwrap();
    let project_b = store.create_project("b", None).unwrap();
    let grants = GrantSet::new(vec![grant_for(&dir)]);
    let embedder = Embedder::from_provider(Arc::new(MockProvider::new()), 8);
    let index = BruteForceIndex::new(store.clone(), 8);
    let registry = getmasters_core::knowledge::extract::ExtractorRegistry::default_set();

    // Ingest only into project A.
    ingest_path(
        &store,
        &project_a,
        &grants,
        &embedder,
        &index,
        &registry,
        &dir.to_string_lossy(),
    )
    .await
    .unwrap();

    let hits_a = search(&store, &project_a, &embedder, &index, "ownership", 3)
        .await
        .unwrap();
    assert!(!hits_a.is_empty());
    let hits_b = search(&store, &project_b, &embedder, &index, "ownership", 3)
        .await
        .unwrap();
    assert!(
        hits_b.is_empty(),
        "project B should see none of project A's documents"
    );
}

#[tokio::test]
async fn extension_manager_hosts_files_and_knowledge() {
    use getmasters_core::extensions::ExtensionManager;
    use getmasters_core::knowledge::vector::VectorIndex;
    use serde_json::json;

    let dir = temp_corpus();
    let store = Store::open_in_memory().unwrap();
    let project = store.create_project("p", None).unwrap();
    let grants = Arc::new(GrantSet::new(vec![grant_for(&dir)]));
    let embedder = Arc::new(Embedder::from_provider(Arc::new(MockProvider::new()), 8));
    let index: Arc<dyn VectorIndex> = Arc::new(BruteForceIndex::new(store.clone(), 8));

    let project_dir = std::env::temp_dir().join(format!("getmasters-kis-{}", uuid::Uuid::new_v4()));
    let enabled = ["files", "knowledge", "memory", "skills"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mgr = ExtensionManager::with_project(
        project.clone(),
        grants,
        store.clone(),
        embedder,
        index,
        project_dir,
        &enabled,
        &[],
        None,
    )
    .await
    .unwrap();
    let names: Vec<String> = mgr.tool_schemas().iter().map(|t| t.name.clone()).collect();
    assert!(
        names.contains(&"files.create".to_string()),
        "tools: {names:?}"
    );
    assert!(names.contains(&"knowledge.search".to_string()));

    // Ingest then search through the namespaced router.
    let (out, is_err) = mgr
        .call_tool(
            "knowledge.ingest",
            &json!({ "path": dir.to_string_lossy() }),
        )
        .await
        .unwrap();
    assert!(!is_err, "ingest failed: {out}");
    let (out, is_err) = mgr
        .call_tool("knowledge.search", &json!({ "query": "ownership" }))
        .await
        .unwrap();
    assert!(!is_err);
    assert!(out.contains("rust.md"), "search result: {out}");
}

#[tokio::test]
async fn config_embedder_resolves_to_mock_without_key() {
    let store = Store::open_in_memory().unwrap();
    let embedder = Embedder::resolve(&Config::default(), &store);
    assert_eq!(embedder.provider_name(), "mock");
    assert_eq!(embedder.dim(), 8);
}
