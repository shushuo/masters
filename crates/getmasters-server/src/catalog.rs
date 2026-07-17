//! Cloud catalog sync — pull the public **system** masters + skills from `getmasters.app` into the
//! app's global stores.
//!
//! The catalog is served by the cloud (`GET {base}/api/catalog` → [`CatalogDto`]). Sync installs each
//! master into the [global master store](crate::state::AppState::global_master_store) and each skill
//! into the [global skill store](crate::state::AppState::global_skill_store) — both idempotent
//! (overwrite by slug). It is **version-gated** (skips when the cloud `version` is unchanged, unless
//! forced) and **never clobbers user content** (a same-slug global master authored by the user — any
//! `origin` other than `system`/`builtin` — is left untouched).
//!
//! Runs best-effort on startup (spawned from `main`, like [`crate::install`]) and on demand via
//! `POST /catalog/sync`. Opt-out of the startup path with `GETMASTERS_NO_CATALOG_SYNC`.

use getmasters_core::skills::{slugify, Skill};
use getmasters_proto::{CatalogDto, CatalogStatusDto};

use crate::routes::masters::from_dto;
use crate::state::AppState;

/// Default cloud base URL (catalog at `{base}/api/catalog`).
const DEFAULT_CATALOG_BASE: &str = "https://getmasters.app";
/// Env override for the catalog base URL (dev/e2e point this at a local server).
const CATALOG_URL_ENV: &str = "GETMASTERS_CATALOG_URL";
/// Env opt-out for the startup auto-sync (any non-empty value).
const NO_SYNC_ENV: &str = "GETMASTERS_NO_CATALOG_SYNC";

const KEY_VERSION: &str = "catalog_version";
const KEY_SYNCED_AT: &str = "catalog_synced_at";

/// Whether the startup auto-sync is opted out via env.
pub fn startup_sync_disabled() -> bool {
    std::env::var_os(NO_SYNC_ENV).is_some_and(|v| !v.is_empty())
}

/// The cloud base URL (shared by the catalog + daily-snapshot proxies).
pub(crate) fn cloud_base() -> String {
    std::env::var(CATALOG_URL_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CATALOG_BASE.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Fetch the current catalog from the cloud.
pub async fn fetch_catalog() -> Result<CatalogDto, String> {
    let url = format!("{}/api/catalog", cloud_base());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("catalog fetch failed: {}", resp.status()));
    }
    resp.json::<CatalogDto>().await.map_err(|e| e.to_string())
}

/// Sync the catalog into the global stores. Version-gated unless `force`. Returns the resulting
/// status (counts + last-synced version/time).
pub async fn sync_catalog(state: AppState, force: bool) -> Result<CatalogStatusDto, String> {
    let catalog = fetch_catalog().await?;
    Ok(apply_catalog(&state, catalog, force))
}

/// Apply a fetched catalog to the global stores (version-gated unless `force`). Split from
/// [`sync_catalog`] so it's testable without a live HTTP server.
pub fn apply_catalog(state: &AppState, catalog: CatalogDto, force: bool) -> CatalogStatusDto {
    let store = state.agent.store();

    let current = store.get_setting(KEY_VERSION).ok().flatten();
    if !force && current.as_deref() == Some(catalog.version.as_str()) {
        tracing::debug!(version = %catalog.version, "catalog unchanged; skipping sync");
        return status(state);
    }

    let master_store = state.global_master_store();
    let mut masters_written = 0u32;
    for mut dto in catalog.masters {
        if dto.origin.trim().is_empty() {
            dto.origin = "system".to_string();
        }
        let slug = slugify(&dto.name);
        // Never clobber a user-authored master that happens to share a slug.
        if let Ok(Some(existing)) = master_store.load(&slug) {
            if existing.origin != "system" && existing.origin != "builtin" {
                tracing::info!(%slug, origin = %existing.origin, "skipping catalog master (user-owned slug)");
                continue;
            }
        }
        match master_store.create(&from_dto(dto)) {
            Ok(_) => masters_written += 1,
            Err(e) => tracing::warn!(%slug, "failed to install catalog master: {e}"),
        }
    }

    let skill_store = state.global_skill_store();
    let mut skills_written = 0u32;
    for s in catalog.skills {
        let skill = Skill {
            name: s.name,
            summary: s.summary,
            tags: s.tags,
            body: s.steps,
        };
        match skill_store.create_skill(&skill) {
            Ok(_) => skills_written += 1,
            Err(e) => tracing::warn!(name = %skill.name, "failed to install catalog skill: {e}"),
        }
    }

    store.set_setting(KEY_VERSION, &catalog.version).ok();
    store.set_setting(KEY_SYNCED_AT, &now_ms().to_string()).ok();
    tracing::info!(
        version = %catalog.version,
        masters = masters_written,
        skills = skills_written,
        "synced cloud catalog"
    );
    status(state)
}

/// Current sync status: last-synced version/time + installed counts.
pub fn status(state: &AppState) -> CatalogStatusDto {
    let store = state.agent.store();
    let version = store.get_setting(KEY_VERSION).ok().flatten();
    let synced_at = store.get_setting(KEY_SYNCED_AT).ok().flatten();
    let masters = state
        .global_master_store()
        .list()
        .map(|v| v.len() as u32)
        .unwrap_or(0);
    let skills = state
        .global_skill_store()
        .list()
        .map(|v| v.len() as u32)
        .unwrap_or(0);
    CatalogStatusDto {
        version,
        synced_at,
        masters,
        skills,
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
