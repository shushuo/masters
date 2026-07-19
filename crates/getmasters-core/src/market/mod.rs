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

/// Relative tolerance for dual-source close agreement (ADR-0017). Within this → `verified`,
/// beyond → `disputed` (an explicit ⚠ degrade, never silently picking one).
pub const VALIDATION_TOLERANCE: f64 = 0.005; // 0.5%

/// Cross-validate two sources' closes into a `validation` verdict (ADR-0017). Pure: both present
/// and within [`VALIDATION_TOLERANCE`] → `"verified"`; both present but apart → `"disputed"`;
/// otherwise (no second source, or a missing close) → `"unverified"`.
pub fn cross_validate(primary: Option<f64>, secondary: Option<f64>, tol: f64) -> &'static str {
    match (primary, secondary) {
        (Some(a), Some(b)) if a != 0.0 => {
            if ((a - b) / a).abs() <= tol {
                "verified"
            } else {
                "disputed"
            }
        }
        _ => "unverified",
    }
}

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

/// A disclosure announcement as returned by an upstream adapter (the statutory channel, D11).
#[derive(Clone, Debug, serde::Serialize)]
pub struct Announcement {
    /// Upstream announcement id (dedup key together with the adapter's announcement source).
    pub ann_id: String,
    pub symbol: String,
    pub title: String,
    /// `YYYY-MM-DD` the announcement is dated.
    pub ann_date: String,
    /// Epoch ms of publication.
    pub ann_time: i64,
    pub url: Option<String>,
    /// e.g. `"cninfo"` — announcements may come from a different channel than quotes.
    pub source: String,
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
    /// Recent disclosure announcements for a symbol (the earnings sentinel's data face).
    /// Default: unsupported — sources without a disclosure channel degrade gracefully.
    async fn recent_announcements(
        &self,
        _symbol: &str,
        _days: u32,
    ) -> Result<Vec<Announcement>, String> {
        Err("announcements not supported by this source".into())
    }
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
    /// Optional second source for cross-validation (ADR-0017). Absent → single-source `unverified`.
    secondary: Option<Arc<dyn MarketFetcher>>,
}

impl MarketData {
    pub fn new(store: Store, fetcher: Arc<dyn MarketFetcher>) -> Self {
        Self {
            store,
            fetcher,
            secondary: None,
        }
    }

    /// Add a second source: on a fresh fetch, its close is compared with the primary's and the
    /// cached row is marked `verified`/`disputed` accordingly (ADR-0017 dual-source validation).
    pub fn with_secondary(mut self, secondary: Arc<dyn MarketFetcher>) -> Self {
        self.secondary = Some(secondary);
        self
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
                // Dual-source cross-validation (ADR-0017): when a second source is configured,
                // compare closes and record the verdict on the served row. A disputed pair is
                // flagged (⚠), never silently reconciled to one number. The secondary is used only
                // to validate — the primary source remains the served value (a single row per
                // (symbol, date, source) keeps the cache read deterministic).
                let mut validation = "unverified";
                if let Some(sec) = &self.secondary {
                    if let Ok(sq) = sec.fetch_quote(&symbol).await {
                        validation = cross_validate(q.close, sq.close, VALIDATION_TOLERANCE);
                    }
                }
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
                    validation: validation.into(),
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

    /// Recent announcements for a symbol (last `days`): fetch fresh (the sentinel runs daily —
    /// no TTL games), cache what came back, and on fetch failure fall back to the cache window
    /// (`stale: true` semantics are carried by the caller noting the fetch failed). Nothing at
    /// all → empty list is an honest answer for announcements (unlike prices).
    pub async fn announcements(
        &self,
        symbol: &str,
        days: u32,
        now_ms: i64,
    ) -> Result<Vec<crate::store::AnnouncementRow>, String> {
        let symbol =
            normalize_symbol(symbol).ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
        let since = now_ms - (days as i64) * 24 * 60 * 60 * 1000;
        match self.fetcher.recent_announcements(&symbol, days).await {
            Ok(list) => {
                for a in &list {
                    let row = crate::store::AnnouncementRow {
                        symbol: a.symbol.clone(),
                        ann_id: a.ann_id.clone(),
                        title: a.title.clone(),
                        ann_date: a.ann_date.clone(),
                        ann_time: a.ann_time,
                        url: a.url.clone(),
                        source: a.source.clone(),
                        fetched_at: now_ms,
                    };
                    self.store
                        .insert_announcement(&row)
                        .map_err(|e| format!("announcement cache write failed: {e}"))?;
                }
            }
            Err(e) => {
                tracing::debug!(symbol = %symbol, "announcement fetch failed; serving cache: {e}");
            }
        }
        self.store
            .list_announcements(&symbol, since)
            .map_err(|e| format!("announcement cache read failed: {e}"))
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
        announcements: HashMap<String, Vec<Announcement>>,
        pub calls: AtomicUsize,
    }

    impl FixtureFetcher {
        pub fn new(quotes: Vec<FetchedQuote>) -> Self {
            Self {
                quotes: quotes.into_iter().map(|q| (q.symbol.clone(), q)).collect(),
                announcements: HashMap::new(),
                calls: AtomicUsize::new(0),
            }
        }

        /// Add canned announcements for a symbol.
        pub fn with_announcements(mut self, symbol: &str, list: Vec<Announcement>) -> Self {
            self.announcements.insert(symbol.to_string(), list);
            self
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
        async fn recent_announcements(
            &self,
            symbol: &str,
            _days: u32,
        ) -> Result<Vec<Announcement>, String> {
            self.announcements
                .get(symbol)
                .cloned()
                .ok_or_else(|| format!("fixture has no announcements for {symbol}"))
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

    #[test]
    fn cross_validate_verdicts() {
        assert_eq!(cross_validate(Some(100.0), Some(100.2), 0.005), "verified");
        assert_eq!(cross_validate(Some(100.0), Some(101.0), 0.005), "disputed");
        assert_eq!(cross_validate(Some(100.0), None, 0.005), "unverified");
        assert_eq!(cross_validate(None, Some(100.0), 0.005), "unverified");
    }

    #[tokio::test]
    async fn dual_source_marks_verified_and_disputed() {
        // Agreeing sources → verified.
        let store = Store::open_in_memory().unwrap();
        let md = MarketData::new(
            store.clone(),
            Arc::new(FixtureFetcher::single("sh600519", "贵州茅台", 1700.0)),
        )
        .with_secondary(Arc::new(FixtureFetcher::single("sh600519", "贵州茅台", 1700.5)));
        let v = md.quote("sh600519", 1_000).await.unwrap();
        assert_eq!(v.row.validation, "verified");
        assert_eq!(v.row.source, "fixture", "primary source is served");

        // Disagreeing sources (>0.5%) → disputed, flagged not reconciled.
        let store2 = Store::open_in_memory().unwrap();
        let md2 = MarketData::new(
            store2,
            Arc::new(FixtureFetcher::single("sz000001", "平安银行", 100.0)),
        )
        .with_secondary(Arc::new(FixtureFetcher::single("sz000001", "平安银行", 105.0)));
        let v2 = md2.quote("sz000001", 1_000).await.unwrap();
        assert_eq!(v2.row.validation, "disputed");
        assert_eq!(v2.row.close, Some(100.0), "still the primary's number");
    }

    #[tokio::test]
    async fn announcements_cache_and_fall_back() {
        let store = Store::open_in_memory().unwrap();
        let day_ms = 24 * 60 * 60 * 1000;
        let ann = Announcement {
            ann_id: "a1".into(),
            symbol: "sh600519".into(),
            title: "2026年半年度报告".into(),
            ann_date: "2026-07-15".into(),
            ann_time: 9 * day_ms,
            url: Some("https://static.cninfo.com.cn/x.pdf".into()),
            source: "cninfo".into(),
        };
        let fetcher = Arc::new(
            FixtureFetcher::single("sh600519", "贵州茅台", 1700.0)
                .with_announcements("sh600519", vec![ann]),
        );
        let md = MarketData::new(store.clone(), fetcher);

        let now = 10 * day_ms;
        let got = md.announcements("600519", 2, now).await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "2026年半年度报告");
        assert_eq!(got[0].source, "cninfo");

        // Upstream gone → the cache window still serves (graceful fallback)…
        let md_bad = MarketData::new(store.clone(), Arc::new(FailingFetcher));
        let got = md_bad.announcements("sh600519", 2, now).await.unwrap();
        assert_eq!(got.len(), 1);
        // …but an announcement outside the look-back window is not returned.
        let got = md_bad
            .announcements("sh600519", 2, now + 5 * day_ms)
            .await
            .unwrap();
        assert!(got.is_empty());
    }
}
