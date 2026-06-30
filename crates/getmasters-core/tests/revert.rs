#![cfg(feature = "testing")]
//! Revert/undo of file operations after a gated tool loop (mock provider, auto-approver).

use std::sync::Arc;

use futures::StreamExt;
use getmasters_core::agent::AgentService;
use getmasters_core::extensions::ExtensionManager;
use getmasters_core::permission::GrantSet;
use getmasters_core::provider::MockProvider;
use getmasters_core::revision;
use getmasters_core::store::Store;
use getmasters_proto::{FolderAccess, FolderGrant};

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-revert-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

async fn agent_for(store: Store, dir: &std::path::Path) -> AgentService {
    let grant = FolderGrant {
        id: "g".into(),
        project_id: None,
        path: dir.to_string_lossy().into_owned(),
        access: FolderAccess::ReadWrite,
        created_at: 0,
    };
    let extensions = ExtensionManager::with_builtin_files(vec![grant.clone()])
        .await
        .unwrap();
    AgentService::new(store, Arc::new(MockProvider::new()), "mock")
        .with_extensions(Arc::new(extensions), Arc::new(GrantSet::new(vec![grant])))
}

async fn run(agent: &AgentService, session: &str, prompt: &str) {
    let _: Vec<_> = agent.run_turn(session, prompt).await.collect().await;
}

#[tokio::test]
async fn revert_create_deletes_file() {
    let dir = temp_dir();
    let target = dir.join("new.txt");
    let store = Store::open_in_memory().unwrap();
    let session = store.create_session(None, None).unwrap();
    let agent = agent_for(store.clone(), &dir).await;

    run(
        &agent,
        &session.id,
        &format!(
            "[[tool:files.create|path={}|content=fresh]]",
            target.to_str().unwrap()
        ),
    )
    .await;
    assert!(target.exists());

    let summary = revision::revert_last(&store, &session.id).unwrap();
    assert!(summary.contains("deleted"));
    assert!(!target.exists(), "revert of create should delete the file");
}

#[tokio::test]
async fn revert_edit_restores_prior_content() {
    let dir = temp_dir();
    let target = dir.join("doc.txt");
    std::fs::write(&target, "original").unwrap();
    let store = Store::open_in_memory().unwrap();
    let session = store.create_session(None, None).unwrap();
    let agent = agent_for(store.clone(), &dir).await;

    run(
        &agent,
        &session.id,
        &format!(
            "[[tool:files.edit|path={}|find=original|replace=changed]]",
            target.to_str().unwrap()
        ),
    )
    .await;
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "changed");

    revision::revert_last(&store, &session.id).unwrap();
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "original",
        "revert of edit should restore the prior content"
    );

    // Nothing left to revert.
    assert!(revision::revert_last(&store, &session.id).is_err());
}
