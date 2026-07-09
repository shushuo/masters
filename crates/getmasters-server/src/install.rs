//! First-run install detection + anonymous upload.
//!
//! On startup the daemon reports a **single anonymous install event** to the cloud so installs
//! can be counted by platform/version. The report is:
//!
//! - **anonymous** — a random per-data-home UUID ([`Uuid::new_v4`]); no hardware id, no user data;
//! - **once per install** — gated on the `install_reported_at` setting (set only after a 2xx), so a
//!   failed upload is retried on the next launch (at-least-once) and a success never repeats;
//! - **opt-out** — suppressed by the `GETMASTERS_NO_TELEMETRY` env var or the `telemetry_enabled`
//!   setting (see [`telemetry_disabled`]). On by default (docs/06 privacy boundary).
//!
//! It is spawned non-blocking from `main` so an unreachable backend never delays daemon readiness.

use getmasters_core::store::Store;
use serde::Serialize;
use uuid::Uuid;

/// Default cloud base URL the install event is POSTed to (`{base}/api/installs`).
const DEFAULT_TELEMETRY_BASE: &str = "https://getmasters.app";
/// Env override for the telemetry base URL (used by dev/e2e to point at a local server).
const TELEMETRY_URL_ENV: &str = "GETMASTERS_TELEMETRY_URL";
/// Env opt-out: any non-empty value disables install reporting.
const NO_TELEMETRY_ENV: &str = "GETMASTERS_NO_TELEMETRY";

/// Setting keys (reuse the existing key/value `settings` table — no migration needed).
const KEY_INSTALL_ID: &str = "install_id";
const KEY_FIRST_SEEN: &str = "install_first_seen";
const KEY_REPORTED_AT: &str = "install_reported_at";
const KEY_TELEMETRY_ENABLED: &str = "telemetry_enabled";

/// The coarse platform bucket for an install, derived at compile time.
pub fn os_type() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

/// Whether install reporting is disabled — by env opt-out or the `telemetry_enabled` setting.
pub fn telemetry_disabled(store: &Store) -> bool {
    if std::env::var_os(NO_TELEMETRY_ENV).is_some_and(|v| !v.is_empty()) {
        return true;
    }
    matches!(store.get_setting(KEY_TELEMETRY_ENABLED), Ok(Some(v)) if v == "false")
}

/// Resolve the telemetry base URL (env override → default), trimming any trailing slash.
fn telemetry_base() -> String {
    std::env::var(TELEMETRY_URL_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TELEMETRY_BASE.to_string())
        .trim_end_matches('/')
        .to_string()
}

#[derive(Serialize)]
struct InstallEvent {
    install_id: String,
    /// Coarse bucket: `mac` | `windows` | `linux` | `unknown`.
    platform: &'static str,
    /// The Rust target OS (`macos` | `windows` | `linux` | …), informational.
    os: &'static str,
    app_version: String,
}

/// Report the install event once, honoring the opt-out and the once-per-install gate.
///
/// Never returns an error: any failure is logged and left unreported so the next launch retries.
pub async fn report_install(store: Store, version: String) {
    if telemetry_disabled(&store) {
        tracing::debug!("install telemetry disabled; skipping report");
        return;
    }

    // Ensure a stable anonymous id + first-seen stamp on the very first run.
    let install_id = match store.get_setting(KEY_INSTALL_ID) {
        Ok(Some(id)) => id,
        _ => {
            let id = Uuid::new_v4().to_string();
            if let Err(e) = store.set_setting(KEY_INSTALL_ID, &id) {
                tracing::warn!("could not persist install_id: {e}");
                return;
            }
            let _ = store.set_setting(KEY_FIRST_SEEN, &now_ms().to_string());
            id
        }
    };

    // Report exactly once per install — only marked after a successful upload.
    if matches!(store.get_setting(KEY_REPORTED_AT), Ok(Some(_))) {
        return;
    }

    let event = InstallEvent {
        install_id,
        platform: os_type(),
        os: std::env::consts::OS,
        app_version: version,
    };
    let url = format!("{}/api/installs", telemetry_base());

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("could not build telemetry client: {e}");
            return;
        }
    };

    match client.post(&url).json(&event).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Err(e) = store.set_setting(KEY_REPORTED_AT, &now_ms().to_string()) {
                tracing::warn!("install reported but could not persist marker: {e}");
            } else {
                tracing::info!(%url, "reported install event");
            }
        }
        Ok(resp) => {
            tracing::warn!(status = %resp.status(), "install report rejected; will retry next launch");
        }
        Err(e) => {
            tracing::warn!("install report failed: {e}; will retry next launch");
        }
    }
}

/// Current Unix time in milliseconds.
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_type_is_known() {
        assert!(["mac", "windows", "linux", "unknown"].contains(&os_type()));
    }

    #[test]
    fn opt_out_via_setting_short_circuits() {
        let store = Store::open_in_memory().expect("in-memory store");
        store.set_setting(KEY_TELEMETRY_ENABLED, "false").unwrap();
        assert!(telemetry_disabled(&store));

        // report_install must not write an id/marker when disabled.
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(report_install(store.clone(), "0.0.0-test".into()));
        assert!(store.get_setting(KEY_INSTALL_ID).unwrap().is_none());
        assert!(store.get_setting(KEY_REPORTED_AT).unwrap().is_none());
    }
}
