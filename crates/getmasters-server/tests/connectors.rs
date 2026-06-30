//! External MCP connectors (Phase 4d, FR-20; ADR-0005). Spawns the real `mcp-echo` stdio fixture as
//! a project connector and asserts: its tool appears namespaced and is callable; credential stripping
//! holds (the child sees the configured env but NOT the daemon's env); a bogus connector is skipped
//! (built-ins survive); and HTTP CRUD works.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_server::AppState;

/// Path to the compiled `mcp-echo` fixture bin (cargo sets this for the crate's bins).
const ECHO_BIN: &str = env!("CARGO_BIN_EXE_mcp-echo");

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-conn-{}", uuid::Uuid::new_v4()));
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

#[tokio::test]
async fn external_connector_tool_is_hosted_and_isolated() {
    // A daemon-process env var that must NOT leak into the spawned child.
    std::env::set_var("GETMASTERS_LEAK_CHECK", "1");

    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);

    // A connector spawning the echo fixture, with one configured env var the child SHOULD see.
    store
        .upsert_connector(
            &pid,
            "echo",
            ECHO_BIN,
            &[],
            &[("GREETING".into(), "hi".into())],
            true,
        )
        .unwrap();

    let agent = state.project_agent(&pid).await.unwrap();
    let tools = agent.extension_tool_names();
    assert!(
        tools.iter().any(|t| t == "echo.echo"),
        "echo.echo missing from {tools:?}"
    );

    let (out, is_error) = agent
        .call_tool_ungated("echo.echo", &serde_json::json!({ "text": "ping" }))
        .await
        .unwrap();
    assert!(!is_error);
    // Echoed text + the configured env shows through + the daemon env does NOT.
    assert!(out.contains("ping"), "out={out}");
    assert!(out.contains("GREETING=hi"), "configured env missing: {out}");
    assert!(out.contains("LEAK=false"), "daemon env leaked: {out}");

    std::env::remove_var("GETMASTERS_LEAK_CHECK");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn bogus_connector_is_skipped_and_builtins_survive() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);

    store
        .upsert_connector(&pid, "broken", "/no/such/command-xyz", &[], &[], true)
        .unwrap();

    // The agent still builds (the bad connector is logged + skipped); built-in tools remain.
    let agent = state.project_agent(&pid).await.unwrap();
    let tools = agent.extension_tool_names();
    assert!(tools.iter().any(|t| t.starts_with("files.")));
    assert!(!tools.iter().any(|t| t.starts_with("broken.")));

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn connector_crud_over_http() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let pid = store.create_project("p", None).unwrap();
    let state = state_with(&store, &dir);

    let app = getmasters_server::build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let created: getmasters_proto::ConnectorDto = client
        .post(format!("{base}/projects/{pid}/connectors"))
        .bearer_auth("tok")
        .json(&serde_json::json!({
            "name": "filesystem",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem"],
            "env": [["TOKEN", "abc"]]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created.name, "filesystem");
    assert_eq!(created.env, vec![["TOKEN".to_string(), "abc".to_string()]]);

    let all: Vec<getmasters_proto::ConnectorDto> = client
        .get(format!("{base}/projects/{pid}/connectors"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(all.len(), 1);

    // Disable.
    let updated: getmasters_proto::ConnectorDto = client
        .put(format!("{base}/projects/{pid}/connectors/filesystem"))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "enabled": false }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!updated.enabled);

    let del = client
        .delete(format!("{base}/projects/{pid}/connectors/filesystem"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), reqwest::StatusCode::NO_CONTENT);

    std::fs::remove_dir_all(&dir).ok();
}
