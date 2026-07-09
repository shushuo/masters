//! External ACP master agents (Phase 4i, ADR-0014). Spawns the real `acp-echo` stdio fixture as a
//! `backend: acp` master and asserts: the `initialize`/`session/new`/`session/prompt` round-trip
//! returns the echoed reply; configured `acp_env` reaches the child; and the agent's `fs/write_text_file`
//! callback is routed through the Permission & Audit gate — allowed + audited inside a granted folder,
//! denied + audited outside. Also covers `GET /acp/harnesses` detection.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::masters::{AcpLaunch, Master, BACKEND_ACP};
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_proto::FolderAccess;
use getmasters_server::master::{master_store, run as run_master};
use getmasters_server::AppState;

/// Path to the compiled `acp-echo` fixture bin (cargo sets this for the crate's bins).
const ACP_ECHO_BIN: &str = env!("CARGO_BIN_EXE_acp-echo");

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-acp-{tag}-{}", uuid::Uuid::new_v4()));
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

/// Build an `acp` master pointing at the echo fixture, with the given `acp_env`.
fn acp_master(env: Vec<(String, String)>) -> Master {
    Master {
        name: "Echo Agent".into(),
        summary: "External ACP echo coding agent.".into(),
        persona: String::new(),
        default_model: String::new(),
        allowed_skills: vec![],
        allowed_tools: vec![],
        output_contract: String::new(),
        origin: "imported".into(),
        body: String::new(),
        backend: BACKEND_ACP.into(),
        acp: Some(AcpLaunch {
            command: ACP_ECHO_BIN.into(),
            args: vec![],
            env,
        }),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acp_master_round_trips_and_passes_configured_env() {
    let dir = temp_dir("echo");
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);
    store
        .create_folder_grant(Some(&pid), dir.to_str().unwrap(), FolderAccess::ReadWrite)
        .unwrap();

    let slug = master_store(&state, &pid)
        .create(&acp_master(vec![("GREETING".into(), "hi".into())]))
        .unwrap();

    let result = run_master(&state, &pid, &slug, "hello world")
        .await
        .unwrap();
    assert!(
        result.message.content.contains("echo: hello world"),
        "reply did not echo the prompt: {}",
        result.message.content
    );
    assert!(
        result.message.content.contains("GREETING=hi"),
        "configured acp_env did not reach the child: {}",
        result.message.content
    );
    // The reply is attributed to the master, not the bare role.
    assert_eq!(result.message.author, slug);

    // The fixture's tool call + completion were mapped into the durable event log (Phase 4g
    // visibility for ACP masters).
    let kinds: Vec<String> = store
        .list_events(&result.session_id)
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert!(kinds.contains(&"tool_call".to_string()), "{kinds:?}");
    assert!(kinds.contains(&"tool_result".to_string()), "{kinds:?}");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acp_write_inside_grant_is_allowed_and_audited() {
    let dir = temp_dir("write-ok");
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);
    store
        .create_folder_grant(Some(&pid), dir.to_str().unwrap(), FolderAccess::ReadWrite)
        .unwrap();

    let target = dir.join("out.txt");
    let slug = master_store(&state, &pid)
        .create(&acp_master(vec![(
            "ACP_ECHO_WRITE".into(),
            target.to_string_lossy().into_owned(),
        )]))
        .unwrap();

    let result = run_master(&state, &pid, &slug, "please write")
        .await
        .unwrap();

    assert!(
        target.exists(),
        "gated write inside the grant did not happen"
    );
    let audit = store.list_audit(&result.session_id).unwrap();
    assert!(
        audit
            .iter()
            .any(|(tool, decision, _)| tool == "files.create" && decision != "denied"),
        "expected an approved files.create audit row, got {audit:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acp_write_outside_grant_is_denied_and_audited() {
    let dir = temp_dir("write-deny");
    let outside = temp_dir("outside");
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);
    // Grant only `dir`; the write targets `outside`, which is NOT granted.
    store
        .create_folder_grant(Some(&pid), dir.to_str().unwrap(), FolderAccess::ReadWrite)
        .unwrap();

    let target = outside.join("evil.txt");
    let slug = master_store(&state, &pid)
        .create(&acp_master(vec![(
            "ACP_ECHO_WRITE".into(),
            target.to_string_lossy().into_owned(),
        )]))
        .unwrap();

    let result = run_master(&state, &pid, &slug, "please write")
        .await
        .unwrap();

    assert!(!target.exists(), "write outside the grant should be denied");
    let audit = store.list_audit(&result.session_id).unwrap();
    assert!(
        audit
            .iter()
            .any(|(tool, decision, _)| tool == "files.create" && decision == "denied"),
        "expected a denied files.create audit row, got {audit:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&outside).ok();
}

#[tokio::test]
async fn acp_harnesses_endpoint_lists_known_coding_agents() {
    let dir = temp_dir("http");
    let store = Store::open_in_memory().unwrap();
    let state = state_with(&store, &dir);

    let app = getmasters_server::build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let harnesses: Vec<getmasters_proto::AvailableHarnessDto> = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/acp/harnesses"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(harnesses.iter().any(|h| h.id == "claude-code"));
    assert!(harnesses.iter().any(|h| h.id == "codex"));
    assert!(harnesses.iter().any(|h| h.id == "gemini"));

    std::fs::remove_dir_all(&dir).ok();
}
