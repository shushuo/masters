//! `getmastersd` — the Masters local daemon.
//!
//! Binds loopback on an ephemeral port, generates a per-launch bearer token, and emits a
//! single machine-readable handshake line on **stdout** for the desktop to parse:
//!
//! ```text
//! GETMASTERSD_READY {"port":54321,"token":"…"}
//! ```
//!
//! All human logs go to **stderr** (tracing) so they never corrupt that handshake.

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::permission::ApprovalRegistry;
use getmasters_core::provider::{build_provider, UnconfiguredProvider};
use getmasters_core::store::Store;
use getmasters_server::{build_app, AppState};
use uuid::Uuid;

/// Generate a per-launch bearer token (two v4 UUIDs ≈ 244 bits of entropy).
fn generate_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    // Resolve the data home (`~/.getmasters` by default; `GETMASTERS_HOME`/`GETMASTERS_DB_PATH` override) and
    // create it on first run, so an installed app never writes to an unpredictable working dir.
    let db_path = getmasters_server::home::resolve_db_path()?;
    // Open the store first so we can resolve persisted settings; secrets come from the OS
    // keychain (falling back to memory if unavailable).
    let store = Store::open(&db_path)?;
    let secrets = getmasters_core::secrets::default_secret_store();
    let mut cfg = Config::resolve(&store, secrets.as_ref());
    // Root project data dirs under the resolved data home (Config::resolve only knows env defaults).
    cfg.db_path = db_path.clone();
    // Start even without a usable LLM provider: fall back to a placeholder that fails every model
    // call clearly (no offline inference — ADR-0008), so the desktop can come up and the user can
    // configure a provider in Settings/Onboarding. `/health` reports `configured = false` in this
    // state, which auto-opens the setup wizard. A key can also come from ANTHROPIC_API_KEY /
    // OPENAI_API_KEY or any per-provider catalog key.
    let provider: Arc<dyn getmasters_core::provider::Provider> = match build_provider(&cfg) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                "starting unconfigured: {e}. Set a provider key in Settings to enable chat."
            );
            Arc::new(UnconfiguredProvider)
        }
    };
    tracing::info!(provider = provider.name(), model = %cfg.model, db = ?db_path, "starting getmastersd");

    let agent = AgentService::new(store, provider, cfg.model.clone())
        .with_approval_registry(Arc::new(ApprovalRegistry::new()));
    let token = generate_token();
    let state = AppState::new(agent, token.clone())
        .with_secrets(secrets)
        .with_config(cfg);

    // Fire scheduled recipes while the daemon is alive (FR-17; docs/02 §5).
    getmasters_server::scheduler::spawn(state.clone());

    let app = build_app(state);

    // Loopback only, ephemeral port (docs/06 §3).
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();

    // Handshake line on stdout for the desktop. Keep it the ONLY thing on stdout.
    let handshake = serde_json::json!({ "port": port, "token": token });
    println!("GETMASTERSD_READY {handshake}");
    use std::io::Write;
    std::io::stdout().flush().ok();
    tracing::info!(port, "getmastersd ready");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Resolve on Ctrl-C or SIGTERM so the desktop can drain and stop the daemon cleanly.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
