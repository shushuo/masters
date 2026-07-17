//! The built-in **Assets** MCP server (investing vertical, ADR-0016). Lives in
//! `getmasters-core` (needs the `Store`). Project-scoped (ADR-0011).
//!
//! Tools: `track_asset` (write), `untrack_asset` (write), `list_assets` (read).
//!
//! `track_asset` realizes D8 **silent-but-revocable tracking**: it is Write-classified (gated +
//! audited + event-logged like every side effect), and under the group chat's headless dispatch
//! it executes without a prompt — which is the design: a reversible, local-only list entry the
//! user can remove in one click. **Narrow-scope warning (ADR-0016):** this
//! silent-under-headless posture is justified only by that reversibility and locality; it must
//! not become a precedent for silently writing anything else.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler};

use crate::market::normalize_symbol;
use crate::store::{DeleteAssetOutcome, Store};

use super::AssetsStore;

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct TrackAssetParams {
    /// The instrument symbol (`sh600519`, `600519.SH`, or bare `600519`).
    pub symbol: String,
    /// Display name (e.g. `贵州茅台`).
    pub name: String,
    /// Market id (default `cn-a`).
    #[serde(default)]
    pub market: Option<String>,
    /// `"stock"` (default) or `"fund"`.
    #[serde(default)]
    pub kind: Option<String>,
    /// One sentence on why the user cares (from the conversation) — the watch reason.
    #[serde(default)]
    pub reason: Option<String>,
    /// The close price at watch time (from `market.get_quote`), for the first-interest snapshot.
    #[serde(default)]
    pub snapshot_price: Option<f64>,
    /// `YYYY-MM-DD` the snapshot price is for (the quote's `trade_date`).
    #[serde(default)]
    pub snapshot_date: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct UntrackAssetParams {
    /// The canonical symbol to stop watching.
    pub symbol: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct RecordPositionParams {
    /// The instrument symbol (`sh600519`, `600519.SH`, or bare `600519`).
    pub symbol: String,
    /// Display name (e.g. `贵州茅台`).
    pub name: String,
    /// Number of shares/units held, if the user said (partial data is fine).
    #[serde(default)]
    pub quantity: Option<f64>,
    /// Average cost per share/unit, if the user said.
    #[serde(default)]
    pub cost: Option<f64>,
    /// Which account it sits in (e.g. `券商A`), if the user said.
    #[serde(default)]
    pub account: Option<String>,
    /// `"stock"` (default) or `"fund"`.
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct RecordTxnParams {
    /// The instrument symbol.
    pub symbol: String,
    /// Display name.
    pub name: String,
    /// `"buy"` | `"sell"` | `"dividend"`.
    pub kind: String,
    #[serde(default)]
    pub quantity: Option<f64>,
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(default)]
    pub fee: Option<f64>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ListAssetsParams {
    /// Filter by lifecycle state (`watching` | `holding` | `sold`). Omit for all.
    #[serde(default)]
    pub state: Option<String>,
}

/// An asset row as returned to the model.
#[derive(serde::Serialize)]
struct AssetOut {
    symbol: String,
    name: String,
    market: String,
    kind: String,
    state: String,
    watch_reason: Option<String>,
    watched_at: i64,
    snapshot_price: Option<f64>,
    snapshot_date: Option<String>,
}

/// Assets server scoped to one project.
#[derive(Clone)]
pub struct AssetsServer {
    assets: AssetsStore,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl AssetsServer {
    pub fn new(project_id: impl Into<String>, store: Store) -> Self {
        Self {
            assets: AssetsStore::new(project_id, store),
            tool_router: Self::tool_router(),
        }
    }

    /// Side-effect class per tool (Core's classifier mirrors this).
    pub fn tool_classes() -> &'static [(&'static str, getmasters_proto::SideEffect)] {
        use getmasters_proto::SideEffect::*;
        &[
            ("track_asset", Write),
            ("untrack_asset", Write),
            ("record_position", Write),
            ("record_txn", Write),
            ("list_assets", Read),
        ]
    }
}

fn ok(text: String) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text)])
}
fn err(msg: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg)])
}

fn to_out(row: crate::store::AssetRow) -> AssetOut {
    AssetOut {
        symbol: row.symbol,
        name: row.name,
        market: row.market,
        kind: row.kind,
        state: row.state,
        watch_reason: row.watch_reason,
        watched_at: row.watched_at,
        snapshot_price: row.snapshot_price,
        snapshot_date: row.snapshot_date,
    }
}

#[tool_router]
impl AssetsServer {
    #[tool(
        description = "Start watching an instrument the user showed interest in (silent, \
                       revocable). Records a first-interest snapshot (price/date/reason). \
                       Idempotent — re-tracking an already-watched symbol is a no-op."
    )]
    async fn track_asset(
        &self,
        Parameters(p): Parameters<TrackAssetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let Some(symbol) = normalize_symbol(&p.symbol) else {
            return Ok(err(format!("track_asset: unknown symbol '{}'", p.symbol)));
        };
        let market = p.market.as_deref().unwrap_or("cn-a");
        let kind = match p.kind.as_deref() {
            Some("fund") => "fund",
            _ => "stock",
        };
        Ok(
            match self.assets.track(
                &symbol,
                &p.name,
                market,
                kind,
                p.reason.as_deref(),
                p.snapshot_price,
                p.snapshot_date.as_deref(),
            ) {
                Ok((row, true)) => ok(format!(
                    "now watching {} ({}) — snapshot {} @ {}",
                    row.name,
                    row.symbol,
                    row.snapshot_date.as_deref().unwrap_or("-"),
                    row.snapshot_price
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".into())
                )),
                Ok((row, false)) => ok(format!(
                    "already watching {} ({}) since {} — unchanged",
                    row.name, row.symbol, row.watched_at
                )),
                Err(e) => err(format!("track_asset failed: {e}")),
            },
        )
    }

    #[tool(description = "Stop watching an instrument (only `watching` entries can be removed)")]
    async fn untrack_asset(
        &self,
        Parameters(p): Parameters<UntrackAssetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let Some(symbol) = normalize_symbol(&p.symbol) else {
            return Ok(err(format!("untrack_asset: unknown symbol '{}'", p.symbol)));
        };
        Ok(match self.assets.untrack(&symbol) {
            Ok(DeleteAssetOutcome::Deleted) => ok(format!("stopped watching {symbol}")),
            Ok(DeleteAssetOutcome::NotFound) => err(format!("not watching {symbol}")),
            Ok(DeleteAssetOutcome::NotWatching) => err(format!(
                "{symbol} is a ledger entry (holding/sold), not a watch — cannot remove"
            )),
            Err(e) => err(format!("untrack_asset failed: {e}")),
        })
    }

    #[tool(
        description = "Record that the user HOLDS an instrument (progressive ledger). Only call \
                       after the user confirmed in conversation ('要记下来吗？'); partial data \
                       (no quantity/cost) is fine — fields fill in over time, never demanded."
    )]
    async fn record_position(
        &self,
        Parameters(p): Parameters<RecordPositionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let Some(symbol) = normalize_symbol(&p.symbol) else {
            return Ok(err(format!(
                "record_position: unknown symbol '{}'",
                p.symbol
            )));
        };
        let kind = match p.kind.as_deref() {
            Some("fund") => "fund",
            _ => "stock",
        };
        Ok(
            match self.assets.record_position(
                &symbol,
                &p.name,
                "cn-a",
                kind,
                p.quantity,
                p.cost,
                p.account.as_deref(),
            ) {
                Ok(row) => ok(format!(
                    "recorded holding {} ({}) — quantity {}, cost {}",
                    row.name,
                    row.symbol,
                    p.quantity
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".into()),
                    p.cost.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
                )),
                Err(e) => err(format!("record_position failed: {e}")),
            },
        )
    }

    #[tool(
        description = "Record a transaction (buy/sell/dividend) the user described. Only call \
                       after the user confirmed; a buy moves a watched instrument to holding."
    )]
    async fn record_txn(
        &self,
        Parameters(p): Parameters<RecordTxnParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let Some(symbol) = normalize_symbol(&p.symbol) else {
            return Ok(err(format!("record_txn: unknown symbol '{}'", p.symbol)));
        };
        let kind = match p.kind.as_str() {
            "buy" | "sell" | "dividend" => p.kind.as_str(),
            other => return Ok(err(format!("record_txn: unknown kind '{other}'"))),
        };
        Ok(
            match self.assets.record_txn(
                &symbol,
                &p.name,
                kind,
                p.quantity,
                p.price,
                p.fee,
                p.note.as_deref(),
            ) {
                Ok(()) => ok(format!("recorded {kind} on {symbol}")),
                Err(e) => err(format!("record_txn failed: {e}")),
            },
        )
    }

    #[tool(description = "List the user's tracked assets (watch list and, later, holdings)")]
    async fn list_assets(
        &self,
        Parameters(p): Parameters<ListAssetsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(match self.assets.list(p.state.as_deref()) {
            Ok(rows) => {
                let out: Vec<AssetOut> = rows.into_iter().map(to_out).collect();
                ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".into()))
            }
            Err(e) => err(format!("list_assets failed: {e}")),
        })
    }
}

#[tool_handler]
impl ServerHandler for AssetsServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Masters assets server: the user's asset lifecycle (watching → holding → sold). \
             Track instruments the user shows interest in; tracking is silent and revocable."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}
