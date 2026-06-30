//! The built-in **Knowledge** MCP server (docs/04 §2.2). Lives in `getmasters-core` (not
//! `getmasters-mcp`) because it needs the `Store`, `Embedder`, and `VectorIndex` — all core types
//! — so a core-hosted rmcp server avoids a `core ↔ mcp` cycle. Project-scoped (ADR-0011).
//!
//! Tools: `ingest` (write), `search` (read), `status` (read). There is intentionally **no**
//! `answer` tool — retrieval is read-only and the Agent Core composes the cited answer, so the
//! single permission/audit path and per-session model selection are never bypassed.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler};

use crate::permission::GrantSet;
use crate::store::Store;

use super::extract::ExtractorRegistry;
use super::vector::VectorIndex;
use super::{ingest, search, Embedder};

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct IngestParams {
    /// File or folder (within a granted folder) to index.
    pub path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    /// What to look for in the user's documents.
    pub query: String,
    /// Max results (default 5).
    #[serde(default)]
    pub k: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct StatusParams {}

/// Knowledge server scoped to one project.
#[derive(Clone)]
pub struct KnowledgeServer {
    project_id: String,
    grants: Arc<GrantSet>,
    store: Store,
    embedder: Arc<Embedder>,
    index: Arc<dyn VectorIndex>,
    registry: Arc<ExtractorRegistry>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl KnowledgeServer {
    pub fn new(
        project_id: impl Into<String>,
        grants: Arc<GrantSet>,
        store: Store,
        embedder: Arc<Embedder>,
        index: Arc<dyn VectorIndex>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            grants,
            store,
            embedder,
            index,
            registry: Arc::new(ExtractorRegistry::default_set()),
            tool_router: Self::tool_router(),
        }
    }

    /// Side-effect class per tool (Core's classifier mirrors this).
    pub fn tool_classes() -> &'static [(&'static str, getmasters_proto::SideEffect)] {
        use getmasters_proto::SideEffect::*;
        &[("ingest", Write), ("search", Read), ("status", Read)]
    }
}

fn ok(text: String) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text)])
}
fn err(msg: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg)])
}

#[tool_router]
impl KnowledgeServer {
    #[tool(description = "Index a file or folder of the user's documents for grounded search")]
    async fn ingest(
        &self,
        Parameters(p): Parameters<IngestParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match ingest::ingest_path(
            &self.store,
            &self.project_id,
            &self.grants,
            &self.embedder,
            self.index.as_ref(),
            &self.registry,
            &p.path,
        )
        .await
        {
            Ok(r) => Ok(ok(format!(
                "indexed {} document(s), {} skipped (unchanged), {} chunks",
                r.indexed, r.skipped, r.chunks
            ))),
            Err(e) => Ok(err(format!("ingest failed: {e}"))),
        }
    }

    #[tool(
        description = "Search the user's indexed documents; returns cited chunks (path + location)"
    )]
    async fn search(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let k = p.k.unwrap_or(5);
        match search(
            &self.store,
            &self.project_id,
            &self.embedder,
            self.index.as_ref(),
            &p.query,
            k,
        )
        .await
        {
            Ok(hits) => Ok(ok(
                serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into())
            )),
            Err(e) => Ok(err(format!("search failed: {e}"))),
        }
    }

    #[tool(description = "Knowledge index status: document/chunk counts, backend, embedding model")]
    async fn status(
        &self,
        Parameters(_): Parameters<StatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.store.knowledge_status(&self.project_id) {
            Ok((docs, chunks, last)) => Ok(ok(serde_json::json!({
                "documents": docs,
                "chunks": chunks,
                "last_indexed_at": last,
                "backend": self.index.backend(),
                "embedding_provider": self.embedder.provider_name(),
                "embedding_dim": self.embedder.dim(),
            })
            .to_string())),
            Err(e) => Ok(err(format!("status failed: {e}"))),
        }
    }
}

#[tool_handler]
impl ServerHandler for KnowledgeServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Masters Knowledge server: index and search the user's documents for grounded answers."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}
