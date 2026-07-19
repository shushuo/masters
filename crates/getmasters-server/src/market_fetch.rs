//! The **Eastmoney** market-data adapter — the injected [`MarketFetcher`] implementation
//! (ADR-0017: the core stays HTTP-free; the adapter lives server-side and is the slot that
//! becomes catalog-hot-updatable content in a later slice).
//!
//! Source posture (ADR-0017/D11): daily/EOD public-republication data only, single source in
//! slice 1 — every cached row is marked `unverified` until dual-source cross-validation lands
//! (Tencent `qt.gtimg.cn` is the documented second-source slot; it is GBK plain-text, hence not
//! first). Parsing is split into pure functions unit-tested on canned JSON; **no automated test
//! touches the network.**

use async_trait::async_trait;
use chrono::{FixedOffset, TimeZone};
use serde_json::Value;
use std::sync::Arc;

use getmasters_core::market::{
    normalize_symbol, Announcement, FetchedQuote, MarketFetcher, SymbolHit,
};

const QUOTE_BASE: &str = "https://push2.eastmoney.com/api/qt/stock/get";
const SUGGEST_BASE: &str = "https://searchapi.eastmoney.com/api/suggest/get";
/// Disclosure announcements come from the **statutory channel** (cninfo, D11) — a different
/// upstream than quotes, deliberately: filings are what the channel exists to publish.
const CNINFO_QUERY: &str = "http://www.cninfo.com.cn/new/hisAnnouncement/query";
const CNINFO_STATIC: &str = "https://static.cninfo.com.cn/";
const ANNOUNCEMENT_SOURCE: &str = "cninfo";
/// The push2 fields we request: f43 latest, f57 code, f58 name, f60 prev close,
/// f86 quote timestamp (epoch seconds), f170 change pct.
const QUOTE_FIELDS: &str = "f43,f57,f58,f60,f86,f170";

/// Eastmoney push2 (UTF-8 JSON) adapter.
pub struct EastmoneyFetcher {
    client: reqwest::Client,
}

impl EastmoneyFetcher {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (getmasters-desktop)")
            .build()
            .expect("reqwest client");
        Self { client }
    }
}

impl Default for EastmoneyFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// The default live adapter (injected into `AppState`; tests inject a core `FixtureFetcher`).
pub fn default_fetcher() -> Arc<dyn MarketFetcher> {
    Arc::new(EastmoneyFetcher::new())
}

/// The **Tencent** (`qt.gtimg.cn`) adapter — the documented second source for dual-source
/// cross-validation (ADR-0017). GBK plain-text (`~`-delimited), so parsing decodes bytes as
/// GBK-ish latin fallback for the numeric fields we need (name may be mojibake, but we only compare
/// closes). Wired **opt-in** (`GETMASTERS_MARKET_DUAL_SOURCE`) until validated against the live
/// feed — an unverified parser must never silently flag real quotes `disputed`.
pub struct TencentFetcher {
    client: reqwest::Client,
}

impl TencentFetcher {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (getmasters-desktop)")
            .build()
            .expect("reqwest client");
        Self { client }
    }
}

impl Default for TencentFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// The opt-in second source, if `GETMASTERS_MARKET_DUAL_SOURCE` is set to a non-empty value.
pub fn secondary_fetcher() -> Option<Arc<dyn MarketFetcher>> {
    match std::env::var("GETMASTERS_MARKET_DUAL_SOURCE") {
        Ok(v) if !v.is_empty() => Some(Arc::new(TencentFetcher::new())),
        _ => None,
    }
}

/// Pure parser for a `qt.gtimg.cn` quote line: `v_sh600519="1~名称~600519~1700.00~1690.00~…";`.
/// Field indices (gtimg convention): 1 name, 3 current/close, 4 prev close, 30 datetime
/// (`YYYYMMDDHHMMSS`), 32 change %. Honesty rule: no usable price → error (never invented).
pub fn parse_tencent(symbol: &str, body: &str) -> Result<FetchedQuote, String> {
    // Take the quoted payload between the first '="' and the trailing '"'.
    let start = body
        .find("=\"")
        .map(|i| i + 2)
        .ok_or_else(|| format!("bad tencent body for {symbol}"))?;
    let rest = &body[start..];
    let end = rest
        .find('"')
        .ok_or_else(|| format!("unterminated tencent body for {symbol}"))?;
    let fields: Vec<&str> = rest[..end].split('~').collect();
    let get = |i: usize| fields.get(i).map(|s| s.trim()).filter(|s| !s.is_empty());
    let num = |i: usize| get(i).and_then(|s| s.parse::<f64>().ok());

    let close = num(3);
    let prev_close = num(4);
    if close.is_none() && prev_close.is_none() {
        return Err(format!("tencent quote for {symbol} has no usable price"));
    }
    // Datetime like "20240715150000" → first 8 digits are YYYYMMDD.
    let trade_date = get(30).and_then(|dt| {
        let digits: String = dt.chars().filter(|c| c.is_ascii_digit()).collect();
        (digits.len() >= 8)
            .then(|| format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8]))
    });
    let trade_date =
        trade_date.ok_or_else(|| format!("tencent quote for {symbol} has no timestamp"))?;

    Ok(FetchedQuote {
        symbol: symbol.to_string(),
        name: get(1).map(str::to_string),
        market: "cn-a".into(),
        trade_date,
        close,
        prev_close,
        change_pct: num(32),
    })
}

/// `sh600519` → push2 `secid` `1.600519` (sh→1, sz→0).
fn secid(symbol: &str) -> Result<String, String> {
    let (ex, code) = symbol.split_at(2);
    let mkt = match ex {
        "sh" => "1",
        "sz" => "0",
        _ => return Err(format!("unsupported exchange in '{symbol}'")),
    };
    Ok(format!("{mkt}.{code}"))
}

/// A push2 numeric field: an integer scaled by 100 for A-share prices/percentages; `"-"` (or
/// anything non-numeric) means "no value" (e.g. suspended).
fn scaled(v: Option<&Value>, div: f64) -> Option<f64> {
    v.and_then(Value::as_f64).map(|n| n / div)
}

/// Quote timestamp (epoch seconds) → the trading day in Asia/Shanghai (`YYYY-MM-DD`).
/// A-share quotes are exchange-local; deriving the date in +08:00 is exact for this market.
fn trade_date_of(epoch_secs: i64) -> Option<String> {
    if epoch_secs <= 0 {
        return None;
    }
    let tz = FixedOffset::east_opt(8 * 3600)?;
    let dt = tz.timestamp_opt(epoch_secs, 0).single()?;
    Some(dt.format("%Y-%m-%d").to_string())
}

/// Pure parser for the push2 quote body. Honesty rule: a missing/zero timestamp or an absent
/// `data` object is an error — we never invent a trade date or a price.
pub fn parse_quote(symbol: &str, body: &str) -> Result<FetchedQuote, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("bad quote JSON: {e}"))?;
    let data = v
        .get("data")
        .filter(|d| d.is_object())
        .ok_or_else(|| format!("no quote data for {symbol}"))?;
    let trade_date = trade_date_of(data.get("f86").and_then(Value::as_i64).unwrap_or(0))
        .ok_or_else(|| format!("quote for {symbol} has no timestamp"))?;
    let close = scaled(data.get("f43"), 100.0);
    let prev_close = scaled(data.get("f60"), 100.0);
    if close.is_none() && prev_close.is_none() {
        return Err(format!("quote for {symbol} has no usable price"));
    }
    Ok(FetchedQuote {
        symbol: symbol.to_string(),
        name: data.get("f58").and_then(Value::as_str).map(str::to_string),
        market: "cn-a".into(),
        trade_date,
        close,
        prev_close,
        change_pct: scaled(data.get("f170"), 100.0),
    })
}

/// Pure parser for the cninfo announcement-query body. Entries without an id/title/time are
/// skipped (never invented); `ann_time` is epoch **ms** upstream; the document URL joins the
/// static host with `adjunctUrl`.
pub fn parse_cninfo_announcements(symbol: &str, body: &str) -> Result<Vec<Announcement>, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("bad announcements JSON: {e}"))?;
    let list = v
        .get("announcements")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(list
        .iter()
        .filter_map(|a| {
            let ann_id = match a.get("announcementId") {
                Some(Value::String(s)) if !s.is_empty() => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => return None,
            };
            let title = a.get("announcementTitle").and_then(Value::as_str)?;
            let ann_time = a.get("announcementTime").and_then(Value::as_i64)?;
            let ann_date = trade_date_of(ann_time / 1000)?;
            let url = a
                .get("adjunctUrl")
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())
                .map(|u| format!("{CNINFO_STATIC}{u}"));
            Some(Announcement {
                ann_id,
                symbol: symbol.to_string(),
                title: title.to_string(),
                ann_date,
                ann_time,
                url,
                source: ANNOUNCEMENT_SOURCE.into(),
            })
        })
        .collect())
}

/// Pure parser for the suggest body. Hits with an unusable `QuoteID` are skipped.
pub fn parse_suggest(body: &str) -> Result<Vec<SymbolHit>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("bad suggest JSON: {e}"))?;
    let data = v
        .pointer("/QuotationCodeTable/Data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(data
        .iter()
        .filter_map(|hit| {
            let quote_id = hit.get("QuoteID").and_then(Value::as_str)?;
            let symbol = normalize_symbol(quote_id)?;
            let name = hit.get("Name").and_then(Value::as_str)?.to_string();
            let type_name = hit
                .get("SecurityTypeName")
                .and_then(Value::as_str)
                .unwrap_or("");
            let kind = if type_name.contains("基金") || type_name.contains("ETF") {
                "fund"
            } else {
                "stock"
            };
            Some(SymbolHit {
                symbol,
                name,
                market: "cn-a".into(),
                kind: kind.into(),
            })
        })
        .collect())
}

#[async_trait]
impl MarketFetcher for EastmoneyFetcher {
    fn source_id(&self) -> &'static str {
        "eastmoney"
    }

    async fn fetch_quote(&self, symbol: &str) -> Result<FetchedQuote, String> {
        let secid = secid(symbol)?;
        let url = format!("{QUOTE_BASE}?secid={secid}&fields={QUOTE_FIELDS}");
        let body = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("quote fetch failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("quote body read failed: {e}"))?;
        parse_quote(symbol, &body)
    }

    async fn search(&self, query: &str) -> Result<Vec<SymbolHit>, String> {
        let url = format!("{SUGGEST_BASE}?input={}&type=14", urlencode(query));
        let body = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("suggest fetch failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("suggest body read failed: {e}"))?;
        parse_suggest(&body)
    }

    async fn recent_announcements(
        &self,
        symbol: &str,
        days: u32,
    ) -> Result<Vec<Announcement>, String> {
        // cninfo's history query takes the bare 6-digit code and a date range. Dates derive
        // via the same Shanghai-local helper as quotes (chrono is built without `clock`).
        let code = &symbol[2..];
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let today = trade_date_of(now_secs).ok_or("clock unavailable")?;
        let from = trade_date_of(now_secs - (days as i64) * 86_400).ok_or("clock unavailable")?;
        // Hand-rolled urlencoded body (this reqwest build is trimmed below `.form()`).
        let form = [
            ("pageNum", "1".to_string()),
            ("pageSize", "30".to_string()),
            ("tabName", "fulltext".to_string()),
            ("stock", code.to_string()),
            ("seDate", format!("{from}~{today}")),
            ("sortName", "time".to_string()),
            ("sortType", "desc".to_string()),
            ("isHLtitle", "false".to_string()),
        ];
        let encoded = form
            .iter()
            .map(|(k, v)| format!("{k}={}", urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let body = self
            .client
            .post(CNINFO_QUERY)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(encoded)
            .send()
            .await
            .map_err(|e| format!("announcements fetch failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("announcements body read failed: {e}"))?;
        parse_cninfo_announcements(symbol, &body)
    }
}

#[async_trait]
impl MarketFetcher for TencentFetcher {
    fn source_id(&self) -> &'static str {
        "tencent"
    }

    async fn fetch_quote(&self, symbol: &str) -> Result<FetchedQuote, String> {
        // gtimg keys on the same `sh`/`sz` + code form we already use.
        let url = format!("https://qt.gtimg.cn/q={symbol}");
        let bytes = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("tencent fetch failed: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("tencent body read failed: {e}"))?;
        // GBK payload; we only need the ASCII numeric fields, so a lossy latin decode suffices for
        // the digits (the CJK name may be mojibake, but closes/dates are ASCII).
        let body: String = bytes.iter().map(|&b| b as char).collect();
        parse_tencent(symbol, &body)
    }

    async fn search(&self, _query: &str) -> Result<Vec<SymbolHit>, String> {
        Err("tencent adapter is quote-only (second source)".into())
    }
}

/// Minimal percent-encoding for the query param (UTF-8 bytes; keeps unreserved ASCII).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_push2_quote() {
        // 1700.00 close / 1690.00 prev / +0.59% at 2024-07-15 15:00 +08:00 (1721026800).
        let body = r#"{"rc":0,"data":{"f43":170000,"f57":"600519","f58":"贵州茅台",
                       "f60":169000,"f86":1721026800,"f170":59}}"#;
        let q = parse_quote("sh600519", body).unwrap();
        assert_eq!(q.symbol, "sh600519");
        assert_eq!(q.name.as_deref(), Some("贵州茅台"));
        assert_eq!(q.close, Some(1700.0));
        assert_eq!(q.prev_close, Some(1690.0));
        assert_eq!(q.change_pct, Some(0.59));
        assert_eq!(q.trade_date, "2024-07-15");
    }

    #[test]
    fn suspended_dash_fields_become_none_and_all_missing_errs() {
        // Suspended: latest is "-", prev close still numeric.
        let body =
            r#"{"data":{"f43":"-","f58":"某停牌股","f60":123450,"f86":1721026800,"f170":"-"}}"#;
        let q = parse_quote("sz000001", body).unwrap();
        assert_eq!(q.close, None);
        assert_eq!(q.prev_close, Some(1234.5));
        assert_eq!(q.change_pct, None);
        // No usable price at all → error, never a fabricated number.
        let body = r#"{"data":{"f43":"-","f60":"-","f86":1721026800}}"#;
        assert!(parse_quote("sz000001", body).is_err());
        // Null data (unknown secid) → error.
        assert!(parse_quote("sh999999", r#"{"rc":0,"data":null}"#).is_err());
        // Zero/missing timestamp → error (no invented trade date).
        assert!(parse_quote("sh600519", r#"{"data":{"f43":100,"f86":0}}"#).is_err());
    }

    #[test]
    fn trade_date_is_derived_in_shanghai_time() {
        // 2024-07-15 23:30 UTC = 2024-07-16 07:30 +08:00 — the CN date, not the UTC one.
        assert_eq!(trade_date_of(1721086200).as_deref(), Some("2024-07-16"));
        assert_eq!(trade_date_of(0), None);
    }

    #[test]
    fn parses_suggest_hits_with_kinds() {
        let body = r#"{"QuotationCodeTable":{"Data":[
            {"Code":"600519","Name":"贵州茅台","QuoteID":"1.600519","SecurityTypeName":"沪A"},
            {"Code":"510300","Name":"沪深300ETF","QuoteID":"1.510300","SecurityTypeName":"ETF基金"},
            {"Code":"XXX","Name":"bad","QuoteID":"weird"}
        ]}}"#;
        let hits = parse_suggest(body).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].symbol, "sh600519");
        assert_eq!(hits[0].kind, "stock");
        assert_eq!(hits[1].symbol, "sh510300");
        assert_eq!(hits[1].kind, "fund");
        // Null Data → empty, not an error.
        assert!(parse_suggest(r#"{"QuotationCodeTable":{"Data":null}}"#)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn parses_cninfo_announcements() {
        // announcementTime is epoch ms; 1721026800000 = 2024-07-15 15:00 +08:00.
        let body = r#"{"announcements":[
            {"announcementId":"1220112233","announcementTitle":"贵州茅台：2024年半年度报告",
             "announcementTime":1721026800000,"adjunctUrl":"finalpage/2024-07-15/1220112233.PDF"},
            {"announcementId":9988,"announcementTitle":"临时公告","announcementTime":1721026800000},
            {"announcementTitle":"缺 id 的坏条目","announcementTime":1721026800000}
        ]}"#;
        let anns = parse_cninfo_announcements("sh600519", body).unwrap();
        assert_eq!(anns.len(), 2);
        assert_eq!(anns[0].ann_id, "1220112233");
        assert_eq!(anns[0].ann_date, "2024-07-15");
        assert_eq!(
            anns[0].url.as_deref(),
            Some("https://static.cninfo.com.cn/finalpage/2024-07-15/1220112233.PDF")
        );
        assert_eq!(anns[0].source, "cninfo");
        assert_eq!(anns[1].ann_id, "9988");
        assert_eq!(anns[1].url, None);
        // Null/absent list → empty, not an error.
        assert!(
            parse_cninfo_announcements("sh600519", r#"{"announcements":null}"#)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn secid_derivation() {
        assert_eq!(secid("sh600519").unwrap(), "1.600519");
        assert_eq!(secid("sz000001").unwrap(), "0.000001");
        assert!(secid("of110011").is_err());
    }

    #[test]
    fn parses_a_tencent_quote() {
        // gtimg line: fields[1]=name, [3]=close, [4]=prev, [30]=datetime, [32]=change%.
        let mut f = vec!["0"; 40];
        f[1] = "贵州茅台";
        f[3] = "1700.00";
        f[4] = "1690.00";
        f[30] = "20240715150000";
        f[32] = "0.59";
        let body = format!("v_sh600519=\"{}\";", f.join("~"));
        let q = parse_tencent("sh600519", &body).unwrap();
        assert_eq!(q.symbol, "sh600519");
        assert_eq!(q.close, Some(1700.0));
        assert_eq!(q.prev_close, Some(1690.0));
        assert_eq!(q.change_pct, Some(0.59));
        assert_eq!(q.trade_date, "2024-07-15");
    }

    #[test]
    fn tencent_no_price_errs_and_bad_body_errs() {
        let mut f = vec!["0"; 40];
        f[3] = "";
        f[4] = "";
        f[30] = "20240715150000";
        let body = format!("v_sh600519=\"{}\";", f.join("~"));
        assert!(parse_tencent("sh600519", &body).is_err());
        assert!(parse_tencent("sh600519", "garbage without quotes").is_err());
    }
}
