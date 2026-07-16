//! The built-in **FinCalc** MCP server (docs/11 M2). One Read tool: `portfolio_overview` —
//! deterministic portfolio math the experts must cite instead of mental arithmetic
//! (NFR-INV-1). Values come with quote provenance; unvalued positions are reported as such.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler};

use crate::market::{MarketData, MarketFetcher};
use crate::store::Store;

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct PortfolioOverviewParams {}

/// FinCalc server scoped to one project.
#[derive(Clone)]
pub struct FinCalcServer {
    project_id: String,
    store: Store,
    market: MarketData,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl FinCalcServer {
    pub fn new(
        project_id: impl Into<String>,
        store: Store,
        fetcher: Arc<dyn MarketFetcher>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            market: MarketData::new(store.clone(), fetcher),
            store,
            tool_router: Self::tool_router(),
        }
    }

    /// Side-effect class per tool (Core's classifier mirrors this) — read-only math.
    pub fn tool_classes() -> &'static [(&'static str, getmasters_proto::SideEffect)] {
        use getmasters_proto::SideEffect::*;
        &[("portfolio_overview", Read)]
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tool_router]
impl FinCalcServer {
    #[tool(
        description = "Deterministic portfolio overview over recorded holdings: per-position \
                       value/weight (with quote provenance), total, HHI concentration, top-3 \
                       share. Positions missing quantity or quote are reported unvalued — \
                       never estimate them yourself."
    )]
    async fn portfolio_overview(
        &self,
        Parameters(_): Parameters<PortfolioOverviewParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(
            match super::overview(&self.store, &self.market, &self.project_id, now_ms()).await {
                Ok(o) => CallToolResult::success(vec![Content::text(
                    serde_json::to_string(&o).unwrap_or_else(|_| "{}".into()),
                )]),
                Err(e) => CallToolResult::error(vec![Content::text(format!(
                    "portfolio_overview failed: {e}"
                ))]),
            },
        )
    }
}

#[tool_handler]
impl ServerHandler for FinCalcServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Masters FinCalc server: deterministic portfolio math. Cite its numbers verbatim \
             with their provenance; never re-derive or estimate portfolio figures."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}
