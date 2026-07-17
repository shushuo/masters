//! Cloud daily-snapshot proxy — the desktop's 「本周市场三件事」 heartbeat (D13; ADR-0017).
//!
//! The daemon fetches the cloud's `GET {base}/api/snapshot/daily` (the same host as the
//! catalog — `cloud_base()`), caches it briefly in [`AppState`](crate::state::AppState), and
//! re-serves it to the desktop at `GET /snapshot/daily`. **Best-effort**: on any failure the
//! endpoint returns an empty payload so the UI falls back to its local quote pack — the
//! heartbeat is a nicety, never a hard dependency (matching `catalog`'s posture).

use std::time::Duration;

use serde::Deserialize;

use getmasters_proto::{DailyQuoteDto, DailySnapshotDto, MarketBulletinDto, MarketIndexDto};

use crate::catalog::cloud_base;

/// Cache TTL for the daily payload. The cloud endpoint is itself CDN-cached; this only avoids
/// re-fetching on every empty-state render.
pub const CACHE_TTL_MS: i64 = 30 * 60 * 1000;

// --- The cloud wire shape (masters-cloud `snapshot.ts::getDailyPayload`). --------------------

#[derive(Deserialize)]
struct CloudDaily {
    #[serde(default)]
    snapshot: Option<CloudSnapshot>,
    #[serde(default)]
    bulletin: Option<CloudBulletin>,
    #[serde(default)]
    quotes: Vec<CloudQuote>,
}

#[derive(Deserialize)]
struct CloudSnapshot {
    date: String,
    #[serde(default)]
    indices: Vec<CloudIndex>,
}

#[derive(Deserialize)]
struct CloudIndex {
    symbol: String,
    name: String,
    #[serde(default)]
    close: Option<f64>,
    #[serde(default)]
    change_pct: Option<f64>,
    #[serde(default)]
    trade_date: Option<String>,
}

#[derive(Deserialize)]
struct CloudBulletin {
    slug: String,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    published_at: Option<String>,
}

#[derive(Deserialize)]
struct CloudQuote {
    #[serde(default)]
    text: String,
    #[serde(default)]
    who: String,
}

/// Pure map from the cloud wire shape to the proto DTO (testable without the network).
pub fn map_cloud(body: &str) -> Result<DailySnapshotDto, String> {
    let cloud: CloudDaily = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let (snapshot_date, indices) = match cloud.snapshot {
        Some(s) => {
            let date = s.date;
            let indices = s
                .indices
                .into_iter()
                .map(|i| MarketIndexDto {
                    symbol: i.symbol,
                    name: i.name,
                    close: i.close,
                    change_pct: i.change_pct,
                    trade_date: i.trade_date.unwrap_or_else(|| date.clone()),
                })
                .collect();
            (Some(date), indices)
        }
        None => (None, Vec::new()),
    };
    let bulletin = cloud.bulletin.map(|b| MarketBulletinDto {
        slug: b.slug,
        title: b.title,
        body: b.body,
        published_at: b.published_at,
    });
    let quotes = cloud
        .quotes
        .into_iter()
        .filter(|q| !q.text.trim().is_empty())
        .map(|q| DailyQuoteDto {
            text: q.text,
            who: q.who,
        })
        .collect();
    Ok(DailySnapshotDto {
        snapshot_date,
        indices,
        bulletin,
        quotes,
    })
}

/// Fetch + map the cloud daily payload. Network-bound; callers treat an `Err` as "serve empty".
pub async fn fetch_daily() -> Result<DailySnapshotDto, String> {
    let url = format!("{}/api/snapshot/daily", cloud_base());
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("snapshot fetch failed: {}", resp.status()));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    map_cloud(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_full_payload() {
        let body = r#"{
            "snapshot": { "date": "2026-07-17", "source": "eastmoney",
                "indices": [
                    { "symbol": "sh000001", "name": "上证指数", "close": 3802.62, "change_pct": -2.06, "trade_date": "2026-07-17" },
                    { "symbol": "sz399006", "name": "创业板指", "close": null, "change_pct": null }
                ] },
            "bulletin": { "slug": "wk-29", "title": "本周三件事", "body": "一、…", "published_at": "2026-07-13T00:00:00Z" },
            "quotes": [ { "text": "别人贪婪时恐惧。", "who": "巴菲特" }, { "text": "", "who": "x" } ]
        }"#;
        let d = map_cloud(body).unwrap();
        assert_eq!(d.snapshot_date.as_deref(), Some("2026-07-17"));
        assert_eq!(d.indices.len(), 2);
        // Missing trade_date falls back to the snapshot date.
        assert_eq!(d.indices[1].trade_date, "2026-07-17");
        assert_eq!(d.bulletin.as_ref().unwrap().title, "本周三件事");
        // Empty-text quote is dropped.
        assert_eq!(d.quotes.len(), 1);
    }

    #[test]
    fn maps_empty_payload() {
        let d = map_cloud(r#"{ "snapshot": null, "bulletin": null, "quotes": [] }"#).unwrap();
        assert!(d.snapshot_date.is_none());
        assert!(d.indices.is_empty());
        assert!(d.bulletin.is_none());
        assert!(d.quotes.is_empty());
    }
}
