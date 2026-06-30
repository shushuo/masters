//! Drive the Files server in-process over a `tokio::io::duplex` transport with a raw rmcp
//! client — the same hosting pattern the Core Extension Manager uses. Confirms tool discovery,
//! a create/read round-trip, and out-of-grant rejection.

use getmasters_mcp::FilesServer;
use getmasters_proto::{FolderAccess, FolderGrant};
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;
use serde_json::Value;

fn temp_grant() -> (std::path::PathBuf, FolderGrant) {
    let dir = std::env::temp_dir().join(format!("getmasters-files-{}", unique()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let grant = FolderGrant {
        id: "g".into(),
        project_id: None,
        path: dir.to_string_lossy().into_owned(),
        access: FolderAccess::ReadWrite,
        created_at: 0,
    };
    (dir, grant)
}

fn unique() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn args(v: Value) -> rmcp::model::JsonObject {
    v.as_object().cloned().unwrap()
}

#[tokio::test]
async fn files_server_create_read_and_deny() {
    let (dir, grant) = temp_grant();
    let server = FilesServer::new(vec![grant]);

    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });

    let client = ().serve(client_io).await.expect("client connects");

    // Tool discovery.
    let tools = client.list_all_tools().await.expect("list tools");
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    assert!(names.contains(&"create".to_string()), "tools: {names:?}");
    assert!(names.contains(&"read".to_string()));

    // Create a file inside the grant.
    let target = dir.join("note.txt");
    let res = client
        .call_tool(CallToolRequestParams::new("create").with_arguments(args(
            serde_json::json!({ "path": target.to_str().unwrap(), "content": "hello" }),
        )))
        .await
        .expect("create call");
    assert_ne!(res.is_error, Some(true), "create should succeed");
    assert!(target.exists());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");

    // Read it back.
    let res = client
        .call_tool(CallToolRequestParams::new("read").with_arguments(args(
            serde_json::json!({ "path": target.to_str().unwrap() }),
        )))
        .await
        .expect("read call");
    let text: String = res
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect();
    assert_eq!(text, "hello");

    // Out-of-grant create is rejected as a tool error (not a transport error).
    let res = client
        .call_tool(CallToolRequestParams::new("create").with_arguments(args(
            serde_json::json!({ "path": "/etc/getmasters-should-not-exist", "content": "x" }),
        )))
        .await
        .expect("create call returns");
    assert_eq!(
        res.is_error,
        Some(true),
        "out-of-grant must be a tool error"
    );

    let _ = client.cancel().await;
}
