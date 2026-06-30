//! The built-in **Memory** MCP server (docs/04 §2.4, ADR-0007). Lives in `getmasters-core` because
//! it needs the `Store` index and the project data dir. Project-scoped (ADR-0011).
//!
//! Tools: `remember` (write), `recall` (read), `forget` (write). Files are the source of truth;
//! every write re-indexes its file. The Core permission gate runs before any of these dispatch.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler};

use std::path::PathBuf;

use crate::store::Store;

use super::{MemoryStore, Scope};

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct RememberParams {
    /// Short, stable heading for the item (used to update/replace it later).
    pub title: String,
    /// The durable fact, decision, or profile detail to remember.
    pub content: String,
    /// Where to file it: `"fact"` (default, MEMORY.md) or `"user"` (USER.md profile).
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct RecallParams {
    /// What to look for in durable memory.
    pub query: String,
    /// Max results (default 5).
    #[serde(default)]
    pub k: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ForgetParams {
    /// Title of the remembered item to delete.
    pub title: String,
}

/// A retrieved memory (recall result).
#[derive(serde::Serialize)]
struct RecalledMemory {
    title: String,
    body: String,
    source: String,
}

/// Memory server scoped to one project.
#[derive(Clone)]
pub struct MemoryServer {
    memory: MemoryStore,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl MemoryServer {
    pub fn new(project_dir: PathBuf, project_id: impl Into<String>, store: Store) -> Self {
        Self {
            memory: MemoryStore::new(project_dir, project_id, store),
            tool_router: Self::tool_router(),
        }
    }

    /// Side-effect class per tool (Core's classifier mirrors this).
    pub fn tool_classes() -> &'static [(&'static str, getmasters_proto::SideEffect)] {
        use getmasters_proto::SideEffect::*;
        &[("remember", Write), ("recall", Read), ("forget", Write)]
    }
}

fn ok(text: String) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text)])
}
fn err(msg: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg)])
}

#[tool_router]
impl MemoryServer {
    #[tool(description = "Save a durable fact, decision, or user-profile detail to memory")]
    async fn remember(
        &self,
        Parameters(p): Parameters<RememberParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let scope = Scope::parse(p.scope.as_deref());
        Ok(match self.memory.remember(&p.title, &p.content, scope) {
            Ok(summary) => ok(summary),
            Err(e) => err(format!("remember failed: {e}")),
        })
    }

    #[tool(description = "Recall durable memories relevant to a query")]
    async fn recall(
        &self,
        Parameters(p): Parameters<RecallParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let k = p.k.unwrap_or(5);
        Ok(match self.memory.recall(&p.query, k) {
            Ok(rows) => {
                let hits: Vec<RecalledMemory> = rows
                    .into_iter()
                    .map(|m| RecalledMemory {
                        title: m.title,
                        body: m.body,
                        source: m.source_file,
                    })
                    .collect();
                ok(serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into()))
            }
            Err(e) => err(format!("recall failed: {e}")),
        })
    }

    #[tool(description = "Delete a remembered item by its title")]
    async fn forget(
        &self,
        Parameters(p): Parameters<ForgetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(match self.memory.forget(&p.title) {
            Ok(true) => ok(format!("forgot \"{}\"", p.title)),
            Ok(false) => ok(format!("no memory titled \"{}\"", p.title)),
            Err(e) => err(format!("forget failed: {e}")),
        })
    }
}

#[tool_handler]
impl ServerHandler for MemoryServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Masters Memory server: durable, file-backed memory (MEMORY.md facts, USER.md profile)."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}
