//! Extension Manager — the MCP client host (docs/04 §1, ADR-0005).
//!
//! Hosts one or more built-in servers **in-process** over `tokio::io::duplex` transports and
//! exposes their tools as a single flat registry, namespaced `{prefix}.{tool}`. Phase 2a hosts
//! `files` (from `getmasters-mcp`) and `knowledge` (from `getmasters-core`). `call_tool` routes by
//! prefix. The Permission & Audit gate runs in the agent loop *before* `call_tool`, so nothing
//! here can bypass approval.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServerHandler, ServiceExt};
use serde_json::Value;

use getmasters_mcp::FilesServer;
use getmasters_proto::FolderGrant;

use crate::assets::AssetsServer;
use crate::fincalc::FinCalcServer;
use crate::knowledge::{Embedder, KnowledgeServer, VectorIndex};
use crate::market::{MarketDataServer, MarketFetcher};
use crate::memory::MemoryServer;
use crate::permission::GrantSet;
use crate::provider::ToolSchema;
use crate::skills::SkillsServer;
use crate::store::Store;
use crate::study::StudyServer;

/// One hosted MCP server and its client handle. `task` is the in-process server's loop (built-ins);
/// external (subprocess) connectors have `None` — dropping the client kills the child process.
struct HostedServer {
    prefix: String,
    client: RunningService<RoleClient, ()>,
    task: Option<tokio::task::JoinHandle<()>>,
}

/// An external MCP server to spawn over stdio (Phase 4d, ADR-0005). `env` is the *only* environment
/// the child receives (credential stripping, ADR-0008).
#[derive(Clone, Debug)]
pub struct ExternalConnector {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The execution interface the agent loop dispatches tools through — the "hands" seam:
/// `execute(name, input) → (text, is_error)`. [`ExtensionManager`] is the in-process
/// implementation; a remote (device-side) executor behind the same trait is the documented
/// upgrade path for running the loop off-machine. The Permission & Audit gate runs in the
/// agent loop *before* `execute`, so no implementation can bypass approval.
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    /// The tool schemas advertised to the model.
    fn tool_schemas(&self) -> Vec<ToolSchema>;
    /// Dispatch a namespaced tool call. Returns `(summary_text, is_error)`.
    async fn execute(&self, name: &str, input: &Value) -> Result<(String, bool), String>;
}

/// Hosts built-in MCP servers and aggregates their tools.
pub struct ExtensionManager {
    servers: Vec<HostedServer>,
    tools: Vec<ToolSchema>,
}

#[async_trait::async_trait]
impl ToolExecutor for ExtensionManager {
    fn tool_schemas(&self) -> Vec<ToolSchema> {
        self.tools.clone()
    }

    async fn execute(&self, name: &str, input: &Value) -> Result<(String, bool), String> {
        self.call_tool(name, input).await
    }
}

impl ExtensionManager {
    /// A manager with no tools (the plain chat path).
    pub fn empty() -> Self {
        Self {
            servers: Vec::new(),
            tools: Vec::new(),
        }
    }

    /// Host one in-process `ServerHandler` over a duplex transport, namespacing its tools under `prefix`.
    async fn host<S>(&mut self, prefix: &str, server: S) -> Result<(), String>
    where
        S: ServerHandler + Send + 'static,
    {
        let (server_io, client_io) = tokio::io::duplex(256 * 1024);
        let task = tokio::spawn(async move {
            match server.serve(server_io).await {
                Ok(running) => {
                    let _ = running.waiting().await;
                }
                Err(e) => tracing::error!(error = %e, "MCP server failed to start"),
            }
        });
        let client = ().serve(client_io).await.map_err(|e| e.to_string())?;
        self.register(prefix, client, Some(task)).await
    }

    /// Spawn + connect an **external** MCP server over stdio (Phase 4d, ADR-0005). The child gets a
    /// **cleared environment** plus only the connector's configured `env` (credential stripping,
    /// ADR-0008); its tools are namespaced under `prefix`. Dropping the client kills the child.
    pub async fn host_external(
        &mut self,
        prefix: &str,
        command: &str,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<(), String> {
        let cmd = tokio::process::Command::new(command).configure(|c| {
            c.args(args);
            c.env_clear();
            for (k, v) in env {
                c.env(k, v);
            }
        });
        let child = TokioChildProcess::new(cmd).map_err(|e| e.to_string())?;
        let client = ().serve(child).await.map_err(|e| e.to_string())?;
        self.register(prefix, client, None).await
    }

    /// Aggregate a connected client's tools under `prefix` and record the hosted server.
    async fn register(
        &mut self,
        prefix: &str,
        client: RunningService<RoleClient, ()>,
        task: Option<tokio::task::JoinHandle<()>>,
    ) -> Result<(), String> {
        for t in client.list_all_tools().await.map_err(|e| e.to_string())? {
            self.tools.push(ToolSchema {
                name: format!("{prefix}.{}", t.name),
                description: t.description.map(|d| d.to_string()).unwrap_or_default(),
                input_schema: Value::Object((*t.input_schema).clone()),
            });
        }
        self.servers.push(HostedServer {
            prefix: prefix.to_string(),
            client,
            task,
        });
        Ok(())
    }

    /// Host just the Files server (no project / no knowledge).
    pub async fn with_builtin_files(grants: Vec<FolderGrant>) -> Result<Self, String> {
        let mut mgr = Self::empty();
        mgr.host("files", FilesServer::new(grants)).await?;
        Ok(mgr)
    }

    /// Host a project's built-ins (ADR-0011 context container): Files (scoped to grants),
    /// Knowledge (scoped to the project), and the file-backed Memory + Skills servers rooted at
    /// the project data dir (`project_dir`). Only the servers named in `enabled` are hosted
    /// (FR-19) — a server that isn't hosted contributes no tools, so the model never sees it.
    #[allow(clippy::too_many_arguments)]
    pub async fn with_project(
        project_id: impl Into<String>,
        grants: Arc<GrantSet>,
        store: Store,
        embedder: Arc<Embedder>,
        index: Arc<dyn VectorIndex>,
        project_dir: PathBuf,
        enabled: &std::collections::HashSet<String>,
        connectors: &[ExternalConnector],
        market_fetcher: Option<Arc<dyn MarketFetcher>>,
    ) -> Result<Self, String> {
        let project_id = project_id.into();
        let mut mgr = Self::empty();
        if enabled.contains("files") {
            mgr.host("files", FilesServer::new(grants.grants().to_vec()))
                .await?;
        }
        if enabled.contains("knowledge") {
            mgr.host(
                "knowledge",
                KnowledgeServer::new(project_id.clone(), grants, store.clone(), embedder, index),
            )
            .await?;
        }
        if enabled.contains("memory") {
            mgr.host(
                "memory",
                MemoryServer::new(project_dir.clone(), project_id.clone(), store.clone()),
            )
            .await?;
        }
        if enabled.contains("skills") {
            mgr.host(
                "skills",
                SkillsServer::new(project_dir, project_id.clone(), store.clone()),
            )
            .await?;
        }
        if enabled.contains("study") {
            mgr.host("study", StudyServer::new(project_id.clone(), store.clone()))
                .await?;
        }
        let project_id2 = project_id.clone();
        if enabled.contains("assets") {
            mgr.host("assets", AssetsServer::new(project_id, store.clone()))
                .await?;
        }
        // Market data + FinCalc need the injected upstream fetcher (ADR-0015/0017: core is
        // HTTP-free — the adapter lives in the server crate). `None` → not hosted (graceful
        // absence).
        if let Some(fetcher) = market_fetcher {
            if enabled.contains("market") {
                mgr.host(
                    "market",
                    MarketDataServer::new(store.clone(), fetcher.clone()),
                )
                .await?;
            }
            if enabled.contains("fincalc") {
                mgr.host("fincalc", FinCalcServer::new(project_id2, store, fetcher))
                    .await?;
            }
        }
        // External MCP connectors (Phase 4d). A connector that fails to spawn/connect is logged and
        // skipped — one bad server must never take down the project's whole toolset.
        for c in connectors {
            if let Err(e) = mgr
                .host_external(&c.name, &c.command, &c.args, &c.env)
                .await
            {
                tracing::warn!(connector = %c.name, error = %e, "external MCP connector failed to start; skipping");
            }
        }
        Ok(mgr)
    }

    /// The aggregated tool schemas advertised to the model.
    pub fn tool_schemas(&self) -> &[ToolSchema] {
        &self.tools
    }

    /// Whether any tools are available.
    pub fn has_tools(&self) -> bool {
        !self.tools.is_empty()
    }

    /// Dispatch a namespaced tool call. Returns `(summary_text, is_error)`.
    pub async fn call_tool(&self, name: &str, input: &Value) -> Result<(String, bool), String> {
        let (prefix, bare) = name
            .split_once('.')
            .ok_or_else(|| format!("tool '{name}' has no prefix"))?;
        let server = self
            .servers
            .iter()
            .find(|s| s.prefix == prefix)
            .ok_or_else(|| format!("no hosted server for prefix '{prefix}'"))?;

        let mut param = CallToolRequestParams::new(bare.to_string());
        if let Some(obj) = input.as_object() {
            param = param.with_arguments(obj.clone());
        }
        let res = server
            .client
            .call_tool(param)
            .await
            .map_err(|e| e.to_string())?;
        let text: String = res
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect();
        Ok((text, res.is_error.unwrap_or(false)))
    }
}

impl Drop for ExtensionManager {
    fn drop(&mut self) {
        for s in &self.servers {
            if let Some(task) = &s.task {
                task.abort();
            }
        }
    }
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use super::*;
    use crate::knowledge::{build_index, Embedder};
    use crate::provider::MockProvider;
    use serde_json::json;

    #[tokio::test]
    async fn hosts_and_routes_memory_and_skills() {
        let dir = std::env::temp_dir().join(format!("getmasters-ext-{}", uuid::Uuid::new_v4()));
        let store = Store::open_in_memory().unwrap();
        let pid = store.create_project("p", None).unwrap();
        let grants = Arc::new(GrantSet::empty());
        let embedder = Arc::new(Embedder::from_provider(Arc::new(MockProvider::new()), 8));
        let index = build_index(store.clone(), 8);
        let all = [
            "files",
            "knowledge",
            "memory",
            "skills",
            "study",
            "assets",
            "market",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let fetcher: Arc<dyn crate::market::MarketFetcher> = Arc::new(
            crate::market::testing::FixtureFetcher::single("sh600519", "贵州茅台", 1700.0),
        );
        let mgr = ExtensionManager::with_project(
            pid.clone(),
            grants,
            store,
            embedder,
            index,
            dir.clone(),
            &all,
            &[],
            Some(fetcher),
        )
        .await
        .unwrap();

        // All four servers' tools are aggregated and namespaced.
        let names: Vec<&str> = mgr.tool_schemas().iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"memory.remember"));
        assert!(names.contains(&"skills.create_skill"));
        assert!(names.contains(&"knowledge.search"));
        assert!(names.contains(&"files.read"));
        assert!(names.contains(&"study.save_flashcards"));
        assert!(names.contains(&"study.review_stats"));
        assert!(names.contains(&"study.create_study_plan"));
        assert!(names.contains(&"assets.track_asset"));
        assert!(names.contains(&"market.get_quote"));

        // Assets round-trip: track → listed; market: quote served from the fixture.
        let (out, err) = mgr
            .call_tool(
                "assets.track_asset",
                &json!({"symbol":"600519","name":"贵州茅台","reason":"test",
                        "snapshot_price":1700.0,"snapshot_date":"2026-07-15"}),
            )
            .await
            .unwrap();
        assert!(!err, "{out}");
        assert!(out.contains("now watching"));
        let (out, err) = mgr
            .call_tool("assets.list_assets", &json!({}))
            .await
            .unwrap();
        assert!(!err);
        assert!(out.contains("sh600519"));
        let (out, err) = mgr
            .call_tool("market.get_quote", &json!({"symbol":"sh600519"}))
            .await
            .unwrap();
        assert!(!err);
        assert!(out.contains("\"source\":\"fixture\""));
        assert!(out.contains("1700"));

        // The study server round-trips: save a card, then it shows up as due for review.
        let (_, err) = mgr
            .call_tool(
                "study.save_flashcards",
                &json!({"deck":"Ch1","cards":[{"front":"2+2?","back":"4"}]}),
            )
            .await
            .unwrap();
        assert!(!err);
        let (out, err) = mgr
            .call_tool("study.start_review", &json!({"deck":"Ch1"}))
            .await
            .unwrap();
        assert!(!err);
        assert!(out.contains("2+2?"));

        // Routing reaches the memory server and round-trips through the index.
        let (_, err) = mgr
            .call_tool(
                "memory.remember",
                &json!({"title":"Deadline","content":"due in March"}),
            )
            .await
            .unwrap();
        assert!(!err);
        let (out, err) = mgr
            .call_tool("memory.recall", &json!({"query":"deadline"}))
            .await
            .unwrap();
        assert!(!err);
        assert!(out.contains("Deadline"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn disabled_server_is_not_hosted() {
        let dir = std::env::temp_dir().join(format!("getmasters-ext2-{}", uuid::Uuid::new_v4()));
        let store = Store::open_in_memory().unwrap();
        let pid = store.create_project("p", None).unwrap();
        let grants = Arc::new(GrantSet::empty());
        let embedder = Arc::new(Embedder::from_provider(Arc::new(MockProvider::new()), 8));
        let index = build_index(store.clone(), 8);
        // Everything except memory.
        let enabled = ["files", "knowledge", "skills"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mgr = ExtensionManager::with_project(
            pid,
            grants,
            store,
            embedder,
            index,
            dir.clone(),
            &enabled,
            &[],
            None,
        )
        .await
        .unwrap();

        let names: Vec<&str> = mgr.tool_schemas().iter().map(|t| t.name.as_str()).collect();
        assert!(
            !names.iter().any(|n| n.starts_with("memory.")),
            "memory should not be hosted: {names:?}"
        );
        // No fetcher injected → the market server is gracefully absent even if enabled.
        assert!(!names.iter().any(|n| n.starts_with("market.")));
        assert!(names.contains(&"skills.create_skill"));
        assert!(names.contains(&"files.read"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
