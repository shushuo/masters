//! Investing-vertical routes (docs/11 slice 1): the workspace bootstrap, the asset list, the
//! UI's untrack, and the batch quote endpoint.
//!
//! The quote endpoint goes through the same `market::MarketData` cache-or-fetch path as the
//! `market.get_quote` MCP tool — the number an expert cites and the number the Watch page shows
//! are the same. `DELETE /projects/{id}/assets/{symbol}` is management-screen semantics (a
//! direct store call, like `PUT /projects/{id}/instructions`), not the gated tool path — it is
//! the user's own one-click revocation of a silent track (D8), lifecycle-guarded server-side.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_core::store::DeleteAssetOutcome;
use getmasters_proto::{
    AssetDto, BriefingDto, InvestingWorkspaceDto, PortfolioDto, PortfolioPositionDto, QuoteDto,
};

use crate::state::{AppError, AppState};

/// Cap on briefings returned by the feed.
const MAX_BRIEFINGS: usize = 50;

/// Cap on symbols per quote batch (the Watch page requests one batch per mount).
const MAX_QUOTE_SYMBOLS: usize = 30;

#[utoipa::path(
    post,
    path = "/investing/workspace",
    operation_id = "ensure_investing_workspace",
    responses((status = 200, description = "The (idempotently seeded) investing workspace", body = InvestingWorkspaceDto)),
    tag = "investing"
)]
pub async fn ensure_workspace(
    State(state): State<AppState>,
) -> Result<Json<InvestingWorkspaceDto>, AppError> {
    crate::investing::ensure_workspace(&state)
        .map(Json)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))
}

fn to_asset_dto(row: getmasters_core::store::AssetRow) -> AssetDto {
    AssetDto {
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

#[utoipa::path(
    get,
    path = "/projects/{id}/assets",
    operation_id = "list_project_assets",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "The project's tracked assets (newest interest first)", body = [AssetDto])),
    tag = "investing"
)]
pub async fn list_assets(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AssetDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    let assets = store
        .list_assets(&id, None)?
        .into_iter()
        .map(to_asset_dto)
        .collect();
    Ok(Json(assets))
}

#[utoipa::path(
    delete,
    path = "/projects/{id}/assets/{symbol}",
    operation_id = "untrack_project_asset",
    params(
        ("id" = String, Path, description = "Project id"),
        ("symbol" = String, Path, description = "Canonical symbol (e.g. sh600519)")
    ),
    responses(
        (status = 204, description = "No longer watching"),
        (status = 404, description = "Not watching this symbol"),
        (status = 409, description = "Lifecycle-guarded: the asset is a holding/sold ledger entry")
    ),
    tag = "investing"
)]
pub async fn untrack_asset(
    State(state): State<AppState>,
    Path((id, symbol)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    match store.delete_asset(&id, &symbol)? {
        DeleteAssetOutcome::Deleted => Ok(StatusCode::NO_CONTENT),
        DeleteAssetOutcome::NotFound => Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("not watching {symbol}"),
        )),
        DeleteAssetOutcome::NotWatching => Err(AppError::new(
            StatusCode::CONFLICT,
            format!("{symbol} is a ledger entry (holding/sold), not a watch"),
        )),
    }
}

#[utoipa::path(
    get,
    path = "/projects/{id}/briefings",
    operation_id = "list_project_briefings",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "Delivered proactive-touch briefings, newest first (silent NO_ALERT runs hidden)", body = [BriefingDto])),
    tag = "investing"
)]
pub async fn list_briefings(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<BriefingDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    let recipe_store =
        crate::recipe::RecipeStore::new(state.project_dir(&id), id.clone(), store.clone());

    let mut out = Vec::new();
    for run in store.list_project_runs(&id, MAX_BRIEFINGS)? {
        // Only successful, non-silent runs are briefings. The full body is the run session's
        // final assistant message (the 200-char run summary is just history metadata).
        if run.status != "ok" {
            continue;
        }
        let Some(session_id) = run.session_id else {
            continue;
        };
        let body = store
            .list_messages(&session_id)
            .ok()
            .and_then(|msgs| {
                msgs.into_iter()
                    .rev()
                    .find(|m| m.role == "assistant")
                    .map(|m| m.content)
            })
            .unwrap_or_default();
        if crate::investing::is_silent(&body) {
            continue;
        }
        let title = recipe_store
            .load(&run.recipe_name)
            .ok()
            .flatten()
            .map(|r| r.title)
            .unwrap_or_else(|| run.recipe_name.clone());
        out.push(BriefingDto {
            started_at: run.started_at,
            recipe_name: run.recipe_name,
            title,
            session_id: Some(session_id),
            body,
        });
    }
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/projects/{id}/portfolio",
    operation_id = "get_project_portfolio",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "Deterministic portfolio overview over recorded holdings (unvalued positions reported, never estimated)", body = PortfolioDto)),
    tag = "investing"
)]
pub async fn get_portfolio(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PortfolioDto>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    let market = state.market_data(store.clone());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let o = getmasters_core::fincalc::overview(store, &market, &id, now).await?;
    Ok(Json(PortfolioDto {
        total_value: o.total_value,
        hhi: o.hhi,
        top3_share: o.top3_share,
        unvalued_count: o.unvalued_count as i64,
        positions: o
            .positions
            .into_iter()
            .map(|p| PortfolioPositionDto {
                symbol: p.symbol,
                name: p.name,
                quantity: p.quantity,
                cost: p.cost,
                close: p.close,
                value: p.value,
                weight: p.weight,
                trade_date: p.trade_date,
                source: p.source,
                stale: p.stale,
            })
            .collect(),
    }))
}

#[derive(serde::Deserialize, utoipa::IntoParams)]
pub struct QuotesQuery {
    /// Comma-separated symbols (canonical or common forms), capped at 30.
    pub symbols: String,
}

#[utoipa::path(
    get,
    path = "/projects/{id}/quotes",
    operation_id = "list_project_quotes",
    params(
        ("id" = String, Path, description = "Project id"),
        QuotesQuery
    ),
    responses((status = 200, description = "Latest EOD quotes with provenance; unavailable symbols are omitted", body = [QuoteDto])),
    tag = "investing"
)]
pub async fn list_quotes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<QuotesQuery>,
) -> Result<Json<Vec<QuoteDto>>, AppError> {
    let store = state.agent.store();
    store.get_project(&id)?; // 404 if unknown
    let market = state.market_data(store.clone());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let mut out = Vec::new();
    for sym in q
        .symbols
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(MAX_QUOTE_SYMBOLS)
    {
        // Per-symbol failure degrades to omission — never 500 the whole batch. The Watch page
        // renders a missing quote as an explicit "no data" state.
        match market.quote(sym, now).await {
            Ok(v) => out.push(QuoteDto {
                symbol: v.row.symbol,
                name: v.row.name,
                trade_date: v.row.trade_date,
                close: v.row.close,
                prev_close: v.row.prev_close,
                change_pct: v.row.change_pct,
                source: v.row.source,
                fetched_at: v.row.fetched_at,
                validation: v.row.validation,
                stale: v.stale,
            }),
            Err(e) => tracing::debug!(symbol = %sym, "quote unavailable: {e}"),
        }
    }
    Ok(Json(out))
}
