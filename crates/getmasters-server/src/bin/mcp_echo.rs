//! `mcp-echo` — a minimal stdio MCP server used as a **test fixture** for external connectors
//! (Phase 4d, ADR-0005). It exposes one `echo` tool and reports two environment facts so the
//! connector integration test can prove credential stripping: the child receives **only** the
//! connector's configured env (`GREETING` shows through) and **not** the daemon's environment
//! (`GETMASTERS_LEAK_CHECK` must be absent — the host calls `env_clear()` before spawning).
//!
//! This is a real MCP server (rmcp), so the test spawns it exactly like any third-party server.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt};

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct EchoParams {
    /// The text to echo back.
    text: String,
}

#[derive(Clone)]
struct EchoServer {
    // Populated by `#[tool_router]`; consumed by the generated `#[tool_handler]` dispatch.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl EchoServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Echo the input text back, plus this server's view of its environment")]
    async fn echo(&self, Parameters(p): Parameters<EchoParams>) -> CallToolResult {
        let greeting = std::env::var("GREETING").unwrap_or_else(|_| "none".into());
        let leaked = std::env::var("GETMASTERS_LEAK_CHECK").is_ok();
        CallToolResult::success(vec![Content::text(format!(
            "{}|GREETING={greeting}|LEAK={leaked}",
            p.text
        ))])
    }
}

#[tool_handler]
impl ServerHandler for EchoServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some("Echo test MCP server (Masters connector fixture).".into());
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = EchoServer::new()
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    service.waiting().await?;
    Ok(())
}
