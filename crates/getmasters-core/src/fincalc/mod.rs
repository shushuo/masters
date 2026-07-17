//! **FinCalc** — deterministic portfolio math (docs/11 M2; NFR-INV-1: the LLM never
//! mental-maths money). Pure functions over the ledger + the quote cache; the one impure edge
//! (quotes) goes through [`MarketData`]'s shared cache-or-fetch path so these numbers match
//! what the experts and the Watch page cite.
//!
//! Honesty rules: a position values only when BOTH quantity and a close are present — an
//! unvalued position is reported as such, never estimated; weights/HHI compute over the valued
//! subset only and say so.

pub mod server;

use crate::error::Result;
use crate::market::MarketData;
use crate::store::Store;

pub use server::FinCalcServer;

/// One holding, valued where the data allows.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ValuedPosition {
    pub symbol: String,
    pub name: String,
    pub quantity: Option<f64>,
    pub cost: Option<f64>,
    pub close: Option<f64>,
    /// `quantity × close` when both known; `None` = honestly unvalued.
    pub value: Option<f64>,
    /// Share of the valued total (0..1); `None` when unvalued.
    pub weight: Option<f64>,
    /// Quote provenance (present when a quote was found).
    pub trade_date: Option<String>,
    pub source: Option<String>,
    pub stale: bool,
}

/// The portfolio overview: totals + concentration over the valued subset.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PortfolioOverview {
    /// Sum of valued positions; `None` when nothing could be valued.
    pub total_value: Option<f64>,
    /// Herfindahl–Hirschman index over valued weights (1/n..1; higher = more concentrated).
    pub hhi: Option<f64>,
    /// Combined weight of the three largest valued positions.
    pub top3_share: Option<f64>,
    pub positions: Vec<ValuedPosition>,
    /// How many holdings could not be valued (missing quantity or quote).
    pub unvalued_count: usize,
}

/// Pure: HHI over weights (each 0..1). Empty → `None`.
pub fn hhi(weights: &[f64]) -> Option<f64> {
    if weights.is_empty() {
        return None;
    }
    Some(weights.iter().map(|w| w * w).sum())
}

/// Pure: combined share of the top `n` weights.
pub fn top_n_share(weights: &[f64], n: usize) -> Option<f64> {
    if weights.is_empty() {
        return None;
    }
    let mut sorted = weights.to_vec();
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Some(sorted.iter().take(n).sum())
}

/// Compute the portfolio overview for a project's holdings. Quotes come through the shared
/// [`MarketData`] path (cache-or-fetch, provenance carried onto every valued position).
pub async fn overview(
    store: &Store,
    market: &MarketData,
    project_id: &str,
    now_ms: i64,
) -> Result<PortfolioOverview> {
    let holdings = store.holdings(project_id)?;
    let mut positions: Vec<ValuedPosition> = Vec::with_capacity(holdings.len());
    for (asset, pos) in holdings {
        let quantity = pos.as_ref().and_then(|p| p.quantity);
        let cost = pos.as_ref().and_then(|p| p.cost);
        let quote = market.quote(&asset.symbol, now_ms).await.ok();
        let close = quote.as_ref().and_then(|q| q.row.close);
        let value = match (quantity, close) {
            (Some(q), Some(c)) => Some(q * c),
            _ => None, // honestly unvalued — never estimated
        };
        positions.push(ValuedPosition {
            symbol: asset.symbol,
            name: asset.name,
            quantity,
            cost,
            close,
            value,
            weight: None,
            trade_date: quote.as_ref().map(|q| q.row.trade_date.clone()),
            source: quote.as_ref().map(|q| q.row.source.clone()),
            stale: quote.as_ref().map(|q| q.stale).unwrap_or(false),
        });
    }

    let total: f64 = positions.iter().filter_map(|p| p.value).sum();
    let total_value = (total > 0.0).then_some(total);
    if let Some(t) = total_value {
        for p in &mut positions {
            p.weight = p.value.map(|v| v / t);
        }
    }
    let weights: Vec<f64> = positions.iter().filter_map(|p| p.weight).collect();
    let unvalued_count = positions.iter().filter(|p| p.value.is_none()).count();
    Ok(PortfolioOverview {
        total_value,
        hhi: hhi(&weights),
        top3_share: top_n_share(&weights, 3),
        positions,
        unvalued_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hhi_and_top_share() {
        assert_eq!(hhi(&[]), None);
        assert_eq!(hhi(&[1.0]), Some(1.0));
        let w = [0.5, 0.3, 0.1, 0.1];
        assert!((hhi(&w).unwrap() - 0.36).abs() < 1e-9);
        assert!((top_n_share(&w, 3).unwrap() - 0.9).abs() < 1e-9);
        assert_eq!(top_n_share(&[], 3), None);
    }
}

#[cfg(all(test, feature = "testing"))]
mod overview_tests {
    use std::sync::Arc;

    use super::*;
    use crate::assets::AssetsStore;
    use crate::market::testing::FixtureFetcher;

    #[tokio::test]
    async fn overview_values_only_what_it_can() {
        let store = Store::open_in_memory().unwrap();
        let pid = store.create_project("inv", None).unwrap();
        let assets = AssetsStore::new(pid.clone(), store.clone());
        // 100 × 1700 = 170k valued; the second holding has no quantity → unvalued, no guess.
        assets
            .record_position(
                "sh600519",
                "贵州茅台",
                "cn-a",
                "stock",
                Some(100.0),
                Some(1500.0),
                None,
            )
            .unwrap();
        assets
            .record_position("sz000001", "平安银行", "cn-a", "stock", None, None, None)
            .unwrap();

        let market = MarketData::new(
            store.clone(),
            Arc::new(FixtureFetcher::single("sh600519", "贵州茅台", 1700.0)),
        );
        let o = overview(&store, &market, &pid, 1_000).await.unwrap();
        assert_eq!(o.positions.len(), 2);
        assert_eq!(o.total_value, Some(170_000.0));
        assert_eq!(o.unvalued_count, 1);
        assert_eq!(o.hhi, Some(1.0), "one valued position = fully concentrated");
        let valued = o.positions.iter().find(|p| p.symbol == "sh600519").unwrap();
        assert_eq!(valued.weight, Some(1.0));
        assert_eq!(valued.source.as_deref(), Some("fixture"));
        let unvalued = o.positions.iter().find(|p| p.symbol == "sz000001").unwrap();
        assert_eq!(unvalued.value, None);
        assert_eq!(unvalued.weight, None);
    }
}
