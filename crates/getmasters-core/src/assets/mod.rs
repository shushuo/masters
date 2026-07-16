//! **Assets** — the investing vertical's asset lifecycle spine (ADR-0016).
//!
//! One `assets` table carries each instrument through `watching → holding → sold`: the
//! watchlist and the (future) ledger are states of the same row, not separate features.
//! Slice 1 implements the `watching` state — silent-but-revocable tracking with a
//! point-in-time snapshot (price/date/reason at first interest, docs/11 D10). Holdings and
//! transactions accumulate progressively in V1. DB-owned structured data behind a gated rmcp
//! server ([`server::AssetsServer`]), the Study precedent.

pub mod server;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::Result;
use crate::store::{AssetRow, DeleteAssetOutcome, Store};

pub use server::AssetsServer;

/// Current wall-clock in epoch milliseconds (the one impure edge).
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Project-scoped asset lifecycle state.
#[derive(Clone)]
pub struct AssetsStore {
    project_id: String,
    store: Store,
}

impl AssetsStore {
    pub fn new(project_id: impl Into<String>, store: Store) -> Self {
        Self {
            project_id: project_id.into(),
            store,
        }
    }

    /// Track an instrument as `watching`. Idempotent (first interest wins — see
    /// [`Store::upsert_asset_watch`]). Returns `(row, newly_created)`.
    #[allow(clippy::too_many_arguments)]
    pub fn track(
        &self,
        symbol: &str,
        name: &str,
        market: &str,
        kind: &str,
        reason: Option<&str>,
        snapshot_price: Option<f64>,
        snapshot_date: Option<&str>,
    ) -> Result<(AssetRow, bool)> {
        self.store.upsert_asset_watch(
            &self.project_id,
            symbol,
            name,
            market,
            kind,
            reason,
            snapshot_price,
            snapshot_date,
            now_ms(),
        )
    }

    /// Untrack — lifecycle-guarded (only `watching` rows delete).
    pub fn untrack(&self, symbol: &str) -> Result<DeleteAssetOutcome> {
        self.store.delete_asset(&self.project_id, symbol)
    }

    /// All tracked assets, optionally filtered by state.
    pub fn list(&self, state: Option<&str>) -> Result<Vec<AssetRow>> {
        self.store.list_assets(&self.project_id, state)
    }
}
