//! The built-in **Market data** MCP server (investing vertical, ADR-0017). Lives in
//! `getmasters-core` (needs the `Store`); the outbound fetch is the injected [`MarketFetcher`]
//! (no HTTP in core, ADR-0015). Hosted globally-per-project like the other built-ins.
//!
//! Tools: `get_quote` (read), `search_symbol` (read). Both are deliberately Read-classified
//! despite the injected network fetch — public market data, zero user-data egress, results
//! cached with provenance (see `permission::policy::classify`). Responses carry the full
//! provenance (`source`/`fetched_at`/`validation`/`stale`) the persona contract requires the
//! experts to cite ("数据截至 …").

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler};

use crate::store::Store;

use super::{now_ms, MarketData, MarketFetcher};

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct GetQuoteParams {
    /// The instrument symbol (`sh600519`, `600519.SH`, or bare `600519`).
    pub symbol: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct SearchSymbolParams {
    /// Code, name, or pinyin fragment to search for.
    pub query: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ListAnnouncementsParams {
    /// The instrument symbol (`sh600519`, `600519.SH`, or bare `600519`).
    pub symbol: String,
    /// Look-back window in days (default 2, max 30).
    #[serde(default)]
    pub days: Option<u32>,
}

/// An announcement as returned to the model — statutory-channel provenance included.
#[derive(serde::Serialize)]
struct AnnouncementOut {
    title: String,
    ann_date: String,
    url: Option<String>,
    source: String,
}

/// The quote payload returned to the model — always with provenance.
#[derive(serde::Serialize)]
struct QuoteOut {
    symbol: String,
    name: Option<String>,
    market: String,
    trade_date: String,
    close: Option<f64>,
    prev_close: Option<f64>,
    change_pct: Option<f64>,
    source: String,
    fetched_at: i64,
    validation: String,
    /// True when this is an old cached value served because a refresh failed — say so.
    stale: bool,
}

/// Market data server (global data, hosted per project like every built-in).
#[derive(Clone)]
pub struct MarketDataServer {
    market: MarketData,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl MarketDataServer {
    pub fn new(store: Store, fetcher: Arc<dyn MarketFetcher>) -> Self {
        Self {
            market: MarketData::new(store, fetcher),
            tool_router: Self::tool_router(),
        }
    }

    /// Side-effect class per tool (Core's classifier mirrors this) — all reads.
    pub fn tool_classes() -> &'static [(&'static str, getmasters_proto::SideEffect)] {
        use getmasters_proto::SideEffect::*;
        &[
            ("get_quote", Read),
            ("search_symbol", Read),
            ("list_announcements", Read),
        ]
    }
}

fn ok(text: String) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text)])
}
fn err(msg: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg)])
}

#[tool_router]
impl MarketDataServer {
    #[tool(
        description = "Get the latest end-of-day quote for an instrument (cached, with source \
                       and data-as-of provenance). Cite trade_date + source when quoting numbers."
    )]
    async fn get_quote(
        &self,
        Parameters(p): Parameters<GetQuoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(match self.market.quote(&p.symbol, now_ms()).await {
            Ok(v) => {
                let out = QuoteOut {
                    symbol: v.row.symbol,
                    name: v.row.name,
                    market: v.row.market,
                    trade_date: v.row.trade_date,
                    close: v.row.close,
                    prev_close: v.row.prev_close,
                    change_pct: v.row.change_pct,
                    source: v.row.source,
                    fetched_at: v.row.fetched_at,
                    validation: v.row.validation,
                    stale: v.stale,
                };
                ok(serde_json::to_string(&out).unwrap_or_else(|_| "{}".into()))
            }
            Err(e) => err(format!("get_quote failed: {e}")),
        })
    }

    #[tool(description = "Search instruments by code, name, or pinyin; returns canonical symbols")]
    async fn search_symbol(
        &self,
        Parameters(p): Parameters<SearchSymbolParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(match self.market.fetcher.search(&p.query).await {
            Ok(hits) => ok(serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into())),
            Err(e) => err(format!("search_symbol failed: {e}")),
        })
    }

    #[tool(
        description = "Recent disclosure announcements for an instrument (statutory channel, \
                       with dates + document links). Empty list = none published in the window."
    )]
    async fn list_announcements(
        &self,
        Parameters(p): Parameters<ListAnnouncementsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let days = p.days.unwrap_or(2).min(30);
        Ok(
            match self.market.announcements(&p.symbol, days, now_ms()).await {
                Ok(rows) => {
                    let out: Vec<AnnouncementOut> = rows
                        .into_iter()
                        .map(|a| AnnouncementOut {
                            title: a.title,
                            ann_date: a.ann_date,
                            url: a.url,
                            source: a.source,
                        })
                        .collect();
                    ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".into()))
                }
                Err(e) => err(format!("list_announcements failed: {e}")),
            },
        )
    }
}

#[tool_handler]
impl ServerHandler for MarketDataServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Masters market-data server: end-of-day quotes and symbol search with provenance. \
             Always cite trade_date + source; a stale or missing quote must be stated, never \
             estimated."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}
