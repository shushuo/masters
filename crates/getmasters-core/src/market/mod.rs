//! **Market data** — the investing vertical's quote layer (ADR-0017).
//!
//! Lean-core constraints (ADR-0015): this module owns the unified schema, the symbol
//! normalization, and the cache-or-fetch policy over [`Store`]'s `price_cache` — but **no HTTP**.
//! The outbound fetch is a [`MarketFetcher`] trait injected from `getmasters-server` (the
//! `EmailTransport` seam precedent), so the core stays dependency-free and headless-testable.
//! Correctness rule ("never a wrong figure", NFR-INV-1/2): the cache only holds what an adapter
//! actually returned, every value carries provenance (source + fetched-at + validation), and a
//! missing quote is an explicit error — never a fabricated number.

pub mod server;

use std::sync::Arc;

use async_trait::async_trait;

use crate::store::{PriceRow, Store};

pub use server::MarketDataServer;

/// How long a cached quote is served without re-fetching (EOD data — an hour is generous).
pub const QUOTE_TTL_MS: i64 = 60 * 60 * 1000;

/// Current wall-clock in epoch milliseconds (the one impure edge; the policy math takes `now`).
fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A quote as returned by an upstream adapter (already normalized to canonical units).
#[derive(Clone, Debug)]
pub struct FetchedQuote {
    /// Canonical symbol, e.g. `sh600519`.
    pub symbol: String,
    pub name: Option<String>,
    /// Market id, e.g. `cn-a`.
    pub market: String,
    /// `YYYY-MM-DD` the quote is for.
    pub trade_date: String,
    pub close: Option<f64>,
    pub prev_close: Option<f64>,
    pub change_pct: Option<f64>,
}

/// A symbol-search hit.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SymbolHit {
    pub symbol: String,
    pub name: String,
    pub market: String,
    /// `"stock"` | `"fund"`.
    pub kind: String,
}

/// The injected upstream adapter (implemented in `getmasters-server`; ADR-0017 makes adapters
/// catalog-hot-updatable content in a later slice). Errors are strings — the policy layer decides
/// how to degrade, the adapter only reports.
#[async_trait]
pub trait MarketFetcher: Send + Sync {
    /// Stable adapter id recorded as the cache row's `source` (e.g. `"eastmoney"`).
    fn source_id(&self) -> &'static str;
    /// Fetch the latest EOD quote for a canonical symbol.
    async fn fetch_quote(&self, symbol: &str) -> Result<FetchedQuote, String>;
    /// Search instruments by code / name / pinyin.
    async fn search(&self, query: &str) -> Result<Vec<SymbolHit>, String>;
}

/// Normalize a user- or model-supplied symbol to the canonical lowercase `sh`/`sz` + 6-digit
/// form (`sh600519`). Accepts `SH600519`, `600519.SH`, `600519.ss`, bare `600519` (exchange
/// inferred from the leading digit: 6→sh, 0/3→sz), and `1.600519`/`0.000001` secid forms.
pub fn normalize_symbol(input: &str) -> Option<String> {
    let s = input.trim().to_ascii_lowercase();
    // prefix form: sh600519 / sz000001
    if let Some(code) = s.strip_prefix("sh").or_else(|| s.strip_prefix("sz")) {
        if code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("{}{}", &s[..2], code));
        }
    }
    // suffix form: 600519.sh / 600519.ss / 000001.sz
    if let Some((code, ex)) = s.split_once('.') {
        if code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()) {
            let ex = match ex {
                "sh" | "ss" => "sh",
                "sz" => "sz",
                _ => return None,
            };
            return Some(format!("{ex}{code}"));
        }
        // secid form: 1.600519 / 0.000001
        if code.len() == 1 && ex.len() == 6 && ex.chars().all(|c| c.is_ascii_digit()) {
            let exch = match code {
                "1" => "sh",
                "0" => "sz",
                _ => return None,
            };
            return Some(format!("{exch}{ex}"));
        }
    }
    // bare 6-digit code: infer exchange from the leading digit.
    if s.len() == 6 && s.chars().all(|c| c.is_ascii_digit()) {
        let ex = match s.as_bytes()[0] {
            b'6' | b'5' => "sh",
            b'0' | b'3' | b'1' => "sz",
            _ => return None,
        };
        return Some(format!("{ex}{s}"));
    }
    None
}

/// A quote as served to callers: the cached row plus an honesty flag.
#[derive(Clone, Debug)]
pub struct QuoteView {
    pub row: PriceRow,
    /// True when the row is older than [`QUOTE_TTL_MS`] and a refresh attempt failed —
    /// callers must render this, not hide it.
    pub stale: bool,
}

/// The one shared cache-or-fetch path, used by **both** the MCP tool and the HTTP quote
/// endpoint — so the number an expert cites and the number the Watch page shows are the same.
#[derive(Clone)]
pub struct MarketData {
    store: Store,
    fetcher: Arc<dyn MarketFetcher>,
}

impl MarketData {
    pub fn new(store: Store, fetcher: Arc<dyn MarketFetcher>) -> Self {
        Self { store, fetcher }
    }

    /// Serve a quote: fresh cache hit → return it; else fetch + cache; fetch failure → fall
    /// back to any stale cached row (`stale: true`); nothing at all → `Err` (explicit absence,
    /// ADR-0017 §5 — never a fabricated number).
    pub async fn quote(&self, symbol: &str, now_ms: i64) -> Result<QuoteView, String> {
        let symbol =
            normalize_symbol(symbol).ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
        let cached = self
            .store
            .latest_price(&symbol)
            .map_err(|e| format!("price cache read failed: {e}"))?;
        if let Some(row) = &cached {
            if now_ms - row.fetched_at <= QUOTE_TTL_MS {
                return Ok(QuoteView {
                    row: row.clone(),
                    stale: false,
                });
            }
        }
        match self.fetcher.fetch_quote(&symbol).await {
            Ok(q) => {
                let row = PriceRow {
                    symbol: q.symbol,
                    market: q.market,
                    name: q.name,
                    trade_date: q.trade_date,
                    close: q.close,
                    prev_close: q.prev_close,
                    change_pct: q.change_pct,
                    source: self.fetcher.source_id().into(),
                    fetched_at: now_ms,
                    validation: "unverified".into(),
                };
                self.store
                    .insert_price(&row)
                    .map_err(|e| format!("price cache write failed: {e}"))?;
                Ok(QuoteView { row, stale: false })
            }
            Err(fetch_err) => match cached {
                Some(row) => Ok(QuoteView { row, stale: true }),
                None => Err(format!("no quote available for {symbol}: {fetch_err}")),
            },
        }
    }
}

/// Headless test fakes (the `MockProvider` role for market data).
#[cfg(feature = "testing")]
pub mod testing {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Serves canned quotes and counts fetches (for cache-hit assertions).
    pub struct FixtureFetcher {
        quotes: HashMap<String, FetchedQuote>,
        pub calls: AtomicUsize,
    }

    impl FixtureFetcher {
        pub fn new(quotes: Vec<FetchedQuote>) -> Self {
            Self {
                quotes: quotes.into_iter().map(|q| (q.symbol.clone(), q)).collect(),
                calls: AtomicUsize::new(0),
            }
        }

        /// A single-quote fixture for the common case.
        pub fn single(symbol: &str, name: &str, close: f64) -> Self {
            Self::new(vec![FetchedQuote {
                symbol: symbol.into(),
                name: Some(name.into()),
                market: "cn-a".into(),
                trade_date: "2026-07-15".into(),
                close: Some(close),
                prev_close: Some(close - 1.0),
                change_pct: Some(0.5),
            }])
        }

        pub fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl MarketFetcher for FixtureFetcher {
        fn source_id(&self) -> &'static str {
            "fixture"
        }
        async fn fetch_quote(&self, symbol: &str) -> Result<FetchedQuote, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.quotes
                .get(symbol)
                .cloned()
                .ok_or_else(|| format!("fixture has no quote for {symbol}"))
        }
        async fn search(&self, query: &str) -> Result<Vec<SymbolHit>, String> {
            Ok(self
                .quotes
                .values()
                .filter(|q| {
                    q.symbol.contains(query) || q.name.as_deref().is_some_and(|n| n.contains(query))
                })
                .map(|q| SymbolHit {
                    symbol: q.symbol.clone(),
                    name: q.name.clone().unwrap_or_default(),
                    market: q.market.clone(),
                    kind: "stock".into(),
                })
                .collect())
        }
    }

    /// Always fails — for graceful-absence assertions.
    pub struct FailingFetcher;

    #[async_trait]
    impl MarketFetcher for FailingFetcher {
        fn source_id(&self) -> &'static str {
            "failing"
        }
        async fn fetch_quote(&self, _symbol: &str) -> Result<FetchedQuote, String> {
            Err("upstream unreachable".into())
        }
        async fn search(&self, _query: &str) -> Result<Vec<SymbolHit>, String> {
            Err("upstream unreachable".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_symbol_accepts_common_forms() {
        for (input, want) in [
            ("sh600519", Some("sh600519")),
            ("SH600519", Some("sh600519")),
            ("600519.SH", Some("sh600519")),
            ("600519.ss", Some("sh600519")),
            ("000001.sz", Some("sz000001")),
            ("600519", Some("sh600519")),
            ("000001", Some("sz000001")),
            ("300750", Some("sz300750")),
            ("1.600519", Some("sh600519")),
            ("0.000001", Some("sz000001")),
            (" sh600519 ", Some("sh600519")),
            ("AAPL", None),
            ("60051", None),
            ("600519.xx", None),
        ] {
            assert_eq!(normalize_symbol(input).as_deref(), want, "input={input}");
        }
    }
}

#[cfg(all(test, feature = "testing"))]
mod policy_tests {
    use super::testing::{FailingFetcher, FixtureFetcher};
    use super::*;
    use crate::store::Store;

    fn quote_of(view: &QuoteView) -> (Option<f64>, &str, bool) {
        (view.row.close, view.row.source.as_str(), view.stale)
    }

    #[tokio::test]
    async fn quote_caches_and_serves_within_ttl() {
        let store = Store::open_in_memory().unwrap();
        let fetcher = Arc::new(FixtureFetcher::single("sh600519", "贵州茅台", 1700.0));
        let md = MarketData::new(store, fetcher.clone());

        let v1 = md.quote("600519.SH", 1_000).await.unwrap();
        assert_eq!(quote_of(&v1), (Some(1700.0), "fixture", false));
        // Second call inside the TTL is served from cache — the fixture is hit once.
        let v2 = md.quote("sh600519", 2_000).await.unwrap();
        assert_eq!(quote_of(&v2), (Some(1700.0), "fixture", false));
        assert_eq!(fetcher.call_count(), 1);
        // Past the TTL it re-fetches.
        let _ = md
            .quote("sh600519", 1_000 + QUOTE_TTL_MS + 1)
            .await
            .unwrap();
        assert_eq!(fetcher.call_count(), 2);
    }

    #[tokio::test]
    async fn quote_falls_back_stale_then_errs_honestly() {
        let store = Store::open_in_memory().unwrap();
        // Seed the cache via a working fetcher, then swap in a failing one over the same store.
        let md_ok = MarketData::new(
            store.clone(),
            Arc::new(FixtureFetcher::single("sh600519", "贵州茅台", 1700.0)),
        );
        md_ok.quote("sh600519", 1_000).await.unwrap();

        let md_bad = MarketData::new(store.clone(), Arc::new(FailingFetcher));
        // Past TTL + fetch failure → stale fallback, flagged.
        let v = md_bad
            .quote("sh600519", 1_000 + QUOTE_TTL_MS + 1)
            .await
            .unwrap();
        assert!(v.stale);
        assert_eq!(v.row.close, Some(1700.0));
        // No cache at all → explicit error, never a fabricated number.
        assert!(md_bad.quote("sz000001", 1_000).await.is_err());
    }
}
