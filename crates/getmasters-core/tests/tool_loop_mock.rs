#![cfg(feature = "testing")]
//! End-to-end tool loop over the mock provider — no key, no network, no daemon.
//!
//! Drives: mock tool trigger → Permission gate (AutoApprover) → Files server writes a file →
//! ToolResult fed back → mock final summary. Asserts the file is written and the audit log
//! records the call. A second test asserts an out-of-grant write is denied and audited.

use std::sync::Arc;

use futures::StreamExt;
use getmasters_core::agent::{AgentEvent, AgentService};
use getmasters_core::extensions::ExtensionManager;
use getmasters_core::permission::GrantSet;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::{FolderAccess, FolderGrant};

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-loop-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

async fn build_agent(store: Store, dir: &std::path::Path) -> AgentService {
    let grant = FolderGrant {
        id: "g".into(),
        project_id: None,
        path: dir.to_string_lossy().into_owned(),
        access: FolderAccess::ReadWrite,
        created_at: 0,
    };
    let extensions = ExtensionManager::with_builtin_files(vec![grant.clone()])
        .await
        .expect("files server hosts");
    let grants = GrantSet::new(vec![grant]);
    AgentService::new(store, Arc::new(MockProvider::new()), "mock")
        .with_extensions(Arc::new(extensions), Arc::new(grants))
}

#[tokio::test]
async fn gated_tool_loop_writes_file_and_audits() {
    let dir = temp_dir();
    let target = dir.join("note.txt");
    let store = Store::open_in_memory().unwrap();
    let session = store.create_session(None, None).unwrap();
    let agent = build_agent(store.clone(), &dir).await;

    let prompt = format!(
        "please [[tool:files.create|path={}|content=hello from getmasters]] thanks",
        target.to_str().unwrap()
    );
    let events: Vec<AgentEvent> = agent.run_turn(&session.id, &prompt).await.collect().await;

    // The tool ran and the file exists.
    assert!(target.exists(), "expected the file to be created");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "hello from getmasters"
    );

    // We saw a tool-call + tool-result and a final completion.
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCallStarted { .. })));
    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::ToolResult {
            is_error: false,
            ..
        }
    )));
    assert!(matches!(events.last(), Some(AgentEvent::Complete { .. })));

    // The audit log recorded the gated call.
    let audit = store.list_audit(&session.id).unwrap();
    assert!(
        audit
            .iter()
            .any(|(tool, decision, _)| tool == "files.create"
                && (decision == "approved" || decision == "auto")),
        "audit: {audit:?}"
    );
}

#[tokio::test]
async fn blank_slate_denies_unenabled_tool() {
    let dir = temp_dir();
    let target = dir.join("blank.txt");
    let store = Store::open_in_memory().unwrap();
    let session = store.create_session(None, None).unwrap();
    // Blank Slate with no tools enabled.
    let agent = build_agent(store.clone(), &dir).await.blank_slate(true);

    let prompt = format!(
        "[[tool:files.create|path={}|content=should not write]]",
        target.to_str().unwrap()
    );
    let events: Vec<AgentEvent> = agent.run_turn(&session.id, &prompt).await.collect().await;

    // The tool was requested but denied (Blank Slate), so the file is never created.
    assert!(
        !target.exists(),
        "Blank Slate must withhold the create tool"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolResult { is_error: true, .. })));
    let audit = store.list_audit(&session.id).unwrap();
    assert!(audit
        .iter()
        .any(|(tool, decision, _)| tool == "files.create" && decision == "denied"));
}

#[tokio::test]
async fn out_of_grant_write_is_denied_and_audited() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let session = store.create_session(None, None).unwrap();
    let agent = build_agent(store.clone(), &dir).await;

    // Target a path OUTSIDE the grant.
    let prompt = "[[tool:files.create|path=/etc/getmasters-nope|content=x]]";
    let events: Vec<AgentEvent> = agent.run_turn(&session.id, prompt).await.collect().await;

    assert!(!std::path::Path::new("/etc/getmasters-nope").exists());
    // The tool result surfaced an error, but the run still completed.
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolResult { is_error: true, .. })));
    assert!(matches!(events.last(), Some(AgentEvent::Complete { .. })));

    let audit = store.list_audit(&session.id).unwrap();
    assert!(
        audit
            .iter()
            .any(|(tool, decision, _)| tool == "files.create" && decision == "denied"),
        "audit: {audit:?}"
    );
}
