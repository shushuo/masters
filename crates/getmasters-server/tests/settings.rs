//! Settings + secrets endpoints over the daemon (mock provider, in-memory secret store).

use std::sync::Arc;

use getmasters_core::agent::AgentService;
use getmasters_core::provider::MockProvider;
use getmasters_core::secrets::MemoryStore;
use getmasters_core::store::Store;
use getmasters_proto::{ConfigCheckDto, EnvironmentDto, ProvidersDto, SettingsDto};
use getmasters_server::{build_app, AppState};

const TOKEN: &str = "settings-token";

async fn spawn() -> u16 {
    let store = Store::open_in_memory().unwrap();
    let agent = AgentService::new(store, Arc::new(MockProvider::new()), "mock");
    let state = AppState::new(agent, TOKEN.to_string()).with_secrets(Arc::new(MemoryStore::new()));
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

#[tokio::test]
async fn settings_update_and_secret_presence() {
    let port = spawn().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    // Initial: no keys set.
    let s: SettingsDto = client
        .get(format!("{base}/settings"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!s.openai_key_set);

    // Update provider/model.
    let s: SettingsDto = client
        .put(format!("{base}/settings"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "provider": "openai", "model": "gpt-x" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s.provider, "openai");
    assert_eq!(s.model, "gpt-x");

    // Set the OpenAI key (stored in the secret store, never returned).
    let r = client
        .put(format!("{base}/settings/secret"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "name": "openai_api_key", "value": "sk-secret" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::NO_CONTENT);

    let s: SettingsDto = client
        .get(format!("{base}/settings"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(s.openai_key_set, "key should now be reported present");

    // The value is never exposed by the API.
    let raw = client
        .get(format!("{base}/settings"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!raw.contains("sk-secret"));

    // A catalog provider's key secret is accepted.
    let r = client
        .put(format!("{base}/settings/secret"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "name": "deepseek_api_key", "value": "dsk" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::NO_CONTENT);

    // An unknown secret name is rejected.
    let r = client
        .put(format!("{base}/settings/secret"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "name": "evil", "value": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn lists_provider_catalog_with_state() {
    let port = spawn().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let p: ProvidersDto = client
        .get(format!("{base}/settings/providers"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(p.active, "anthropic"); // default
    for id in [
        "anthropic",
        "openai",
        "deepseek",
        "gemini",
        "ollama",
        "custom",
    ] {
        assert!(p.providers.iter().any(|e| e.id == id), "missing {id}");
    }
    assert!(
        !p.providers
            .iter()
            .find(|e| e.id == "deepseek")
            .unwrap()
            .key_set
    );

    // Set the DeepSeek key + base, make it the active default.
    client
        .put(format!("{base}/settings/secret"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "name": "deepseek_api_key", "value": "dsk-secret" }))
        .send()
        .await
        .unwrap();
    client
        .put(format!("{base}/settings"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({
            "provider": "deepseek",
            "provider_bases": { "deepseek": "https://ds.example.com" }
        }))
        .send()
        .await
        .unwrap();

    let raw = client
        .get(format!("{base}/settings/providers"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !raw.contains("dsk-secret"),
        "key value must never be exposed"
    );
    let p: ProvidersDto = serde_json::from_str(&raw).unwrap();
    assert_eq!(p.active, "deepseek");
    let ds = p.providers.iter().find(|e| e.id == "deepseek").unwrap();
    assert!(ds.key_set);
    assert_eq!(ds.base_url.as_deref(), Some("https://ds.example.com"));
    assert_eq!(ds.transport, "openai_compatible");

    // The active id is also reflected by /settings.
    let s: SettingsDto = client
        .get(format!("{base}/settings"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s.provider, "deepseek");
}

#[tokio::test]
async fn rejects_unknown_provider() {
    let port = spawn().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let r = client
        .put(format!("{base}/settings"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "provider": "not-a-provider" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn environment_reports_resolved_config_and_sources() {
    let port = spawn().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    // Pin provider + model in settings so the report is independent of ambient env keys.
    client
        .put(format!("{base}/settings"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "provider": "openai", "model": "gpt-x" }))
        .send()
        .await
        .unwrap();

    let env: EnvironmentDto = client
        .get(format!("{base}/settings/environment"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(env.configured_provider, "openai");
    // No key configured → no usable provider (the daemon would refuse to start).
    assert_eq!(env.effective_provider, "unconfigured");
    assert_eq!(env.model, "gpt-x");
    // Values pinned in the DB resolve from settings (vs env / default).
    assert_eq!(env.provider_source, "settings");
    assert_eq!(env.model_source, "settings");
    assert!(!env.data_home.is_empty());
    assert!(env.db_path.ends_with("getmasters.db"));
}

#[tokio::test]
async fn config_check_errors_without_credentials() {
    let port = spawn().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    // No API key configured → no usable provider, so the check fails (no offline fallback).
    client
        .put(format!("{base}/settings"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "provider": "anthropic" }))
        .send()
        .await
        .unwrap();

    let check: ConfigCheckDto = client
        .post(format!("{base}/settings/check"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(!check.ok, "no credentials → config check fails");
    assert_eq!(check.effective_provider, "unconfigured");
    // The provider diagnostic flags the missing credentials as an error.
    let provider = check
        .checks
        .iter()
        .find(|c| c.name == "provider")
        .expect("a provider diagnostic is present");
    assert_eq!(provider.status, "error");
}

#[tokio::test]
async fn settings_require_token() {
    let port = spawn().await;
    let r = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/settings"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::UNAUTHORIZED);
}
