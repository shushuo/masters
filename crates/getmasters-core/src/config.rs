//! Runtime configuration, resolved from the environment.
//!
//! Phase 1 reads everything from env vars; Phase 1b moves provider/model selection into
//! Settings persisted in the DB and secrets into the OS keychain (docs/06 §4).

use std::collections::HashMap;
use std::path::PathBuf;

use crate::provider::catalog;
use crate::secrets::SecretStore;
use crate::store::Store;

/// Keychain entry names for the two legacy provider API keys (== `catalog::secret_name`).
pub const SECRET_ANTHROPIC: &str = "anthropic_api_key";
pub const SECRET_OPENAI: &str = "openai_api_key";

/// The wire/routing kind of a provider (its transport). Each catalog vendor maps to one of these
/// (ADR-0013) — Anthropic-native or OpenAI-compatible — so `Provider` impls stay vendor-agnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    /// Real Anthropic/Claude over HTTPS.
    Anthropic,
    /// Real OpenAI-compatible endpoint over HTTPS.
    OpenAi,
}

/// Effective configuration for a daemon/CLI launch.
///
/// `provider_id` is the active catalog vendor (e.g. `"deepseek"`); `provider` is its routing
/// transport. `provider_keys`/`provider_bases` hold every configured vendor's key + base override.
/// The legacy named fields (`anthropic_api_key`/`openai_api_key`/`openai_base_url`) are kept as
/// aliases so existing callers and struct-literal constructions keep working — prefer the
/// [`Config::key_for`]/[`Config::base_for`] accessors, which consult both.
#[derive(Clone, Debug)]
pub struct Config {
    pub provider: ProviderKind,
    /// Active catalog provider id (e.g. `"anthropic"`, `"openai"`, `"deepseek"`).
    pub provider_id: String,
    /// Per-provider API keys keyed by catalog id (secret store + env).
    pub provider_keys: HashMap<String, String>,
    /// Per-provider base-URL overrides keyed by catalog id (settings + env).
    pub provider_bases: HashMap<String, String>,
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    /// Override for the OpenAI-compatible base URL (default `https://api.openai.com`).
    pub openai_base_url: Option<String>,
    pub model: String,
    pub db_path: PathBuf,
}

/// Default Claude model for Phase 1 (ADR-0003 / docs/03 §LLM).
pub const DEFAULT_MODEL: &str = "claude-opus-4-8";
/// Default OpenAI-compatible base URL.
pub const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com";

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// Whether a base URL is a local/loopback endpoint (e.g. Ollama) that needs no API key.
pub fn is_local_base(base: &str) -> bool {
    let b = base.to_ascii_lowercase();
    b.contains("localhost")
        || b.contains("127.0.0.1")
        || b.contains("0.0.0.0")
        || b.contains("[::1]")
}

impl Config {
    /// Build config from environment variables:
    /// - `GETMASTERS_PROVIDER` = `anthropic` (default) | `openai`
    /// - `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` — when absent for the selected provider (and
    ///   the base isn't local), [`Config::effective_provider`] resolves to `None` (the daemon
    ///   then refuses to start; there is no offline fallback)
    /// - `OPENAI_BASE_URL` — override the OpenAI-compatible endpoint
    /// - `GETMASTERS_MODEL` — defaults to [`DEFAULT_MODEL`]
    /// - `GETMASTERS_DB_PATH` — defaults to `getmasters.db`
    pub fn from_env() -> Self {
        let provider_id = std::env::var("GETMASTERS_PROVIDER")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "anthropic".to_string());

        let mut provider_keys = HashMap::new();
        if let Some(k) = env_nonempty("ANTHROPIC_API_KEY") {
            provider_keys.insert("anthropic".to_string(), k);
        }
        if let Some(k) = env_nonempty("OPENAI_API_KEY") {
            provider_keys.insert("openai".to_string(), k);
        }
        let mut provider_bases = HashMap::new();
        if let Some(b) = env_nonempty("OPENAI_BASE_URL") {
            provider_bases.insert("openai".to_string(), b);
        }

        Self::assemble(
            provider_id,
            std::env::var("GETMASTERS_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string()),
            provider_keys,
            provider_bases,
            std::env::var("GETMASTERS_DB_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("getmasters.db")),
        )
    }

    /// Build config from persisted settings + the secret store, falling back to env/defaults.
    /// Persisted DB settings win over env; secrets (keychain) win over env keys.
    pub fn resolve(store: &Store, secrets: &dyn SecretStore) -> Self {
        let env = Self::from_env();

        let provider_id = store
            .get_setting("provider")
            .ok()
            .flatten()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(env.provider_id);
        let model = store
            .get_setting("model")
            .ok()
            .flatten()
            .unwrap_or(env.model);

        // Per-provider keys: secret store (keychain) wins over env.
        let mut provider_keys = env.provider_keys;
        for entry in catalog::CATALOG {
            if let Some(k) = secrets.get(&catalog::secret_name(entry.id)) {
                provider_keys.insert(entry.id.to_string(), k);
            }
        }

        // Per-provider base overrides: the `provider.{id}.base_url` setting, plus the legacy
        // `openai_base_url` setting as a fallback for the `openai` id.
        let mut provider_bases = env.provider_bases;
        for entry in catalog::CATALOG {
            if let Some(b) = store
                .get_setting(&catalog::base_setting_key(entry.id))
                .ok()
                .flatten()
                .filter(|v| !v.is_empty())
            {
                provider_bases.insert(entry.id.to_string(), b);
            }
        }
        if !provider_bases.contains_key("openai") {
            if let Some(b) = store
                .get_setting("openai_base_url")
                .ok()
                .flatten()
                .filter(|v| !v.is_empty())
            {
                provider_bases.insert("openai".to_string(), b);
            }
        }

        Self::assemble(
            provider_id,
            model,
            provider_keys,
            provider_bases,
            env.db_path,
        )
    }

    /// Assemble a `Config` from its parts, deriving `provider` (transport) + legacy alias fields.
    fn assemble(
        provider_id: String,
        model: String,
        provider_keys: HashMap<String, String>,
        provider_bases: HashMap<String, String>,
        db_path: PathBuf,
    ) -> Self {
        let provider = catalog::find(&provider_id)
            .map(|e| e.transport.kind())
            .unwrap_or(ProviderKind::Anthropic);
        Self {
            provider,
            anthropic_api_key: provider_keys.get("anthropic").cloned(),
            openai_api_key: provider_keys.get("openai").cloned(),
            openai_base_url: provider_bases.get("openai").cloned(),
            provider_id,
            provider_keys,
            provider_bases,
            model,
            db_path,
        }
    }

    /// The active catalog id, reconciled with `provider`. Robust to struct-literal construction
    /// (tests that set `provider` directly without a matching `provider_id`): if `provider_id`
    /// doesn't name a catalog entry of the active transport, fall back to the canonical id.
    pub fn active_id(&self) -> &str {
        if let Some(e) = catalog::find(&self.provider_id) {
            if e.transport.kind() == self.provider {
                return &self.provider_id;
            }
        }
        match self.provider {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenAi => "openai",
        }
    }

    /// The API key configured for a catalog provider id (map first, then legacy alias fields).
    pub fn key_for(&self, id: &str) -> Option<String> {
        if let Some(k) = self.provider_keys.get(id) {
            return Some(k.clone());
        }
        match id {
            "anthropic" => self.anthropic_api_key.clone(),
            "openai" => self.openai_api_key.clone(),
            _ => None,
        }
    }

    /// The base URL for a catalog provider id: override → catalog default → empty.
    pub fn base_for(&self, id: &str) -> String {
        if let Some(b) = self.provider_bases.get(id) {
            return b.clone();
        }
        if id == "openai" {
            if let Some(b) = &self.openai_base_url {
                return b.clone();
            }
        }
        catalog::find(id)
            .and_then(|e| e.default_base)
            .map(str::to_string)
            .unwrap_or_default()
    }

    /// The OpenAI base URL to use (override or default).
    pub fn openai_base(&self) -> String {
        let base = self.base_for("openai");
        if base.is_empty() {
            DEFAULT_OPENAI_BASE.to_string()
        } else {
            base
        }
    }

    /// The provider that will actually be used, or `None` when the active provider has no usable
    /// credentials (no offline fallback — the daemon refuses to start on `None`).
    pub fn effective_provider(&self) -> Option<ProviderKind> {
        let id = self.active_id();
        match self.provider {
            ProviderKind::Anthropic if self.key_for(id).is_some() => Some(ProviderKind::Anthropic),
            ProviderKind::OpenAi
                if self.key_for(id).is_some() || is_local_base(&self.base_for(id)) =>
            {
                Some(ProviderKind::OpenAi)
            }
            _ => None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Anthropic,
            provider_id: "anthropic".to_string(),
            provider_keys: HashMap::new(),
            provider_bases: HashMap::new(),
            anthropic_api_key: None,
            openai_api_key: None,
            openai_base_url: None,
            model: DEFAULT_MODEL.to_string(),
            db_path: PathBuf::from("getmasters.db"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{MemoryStore, SecretStore};
    use crate::store::Store;

    #[test]
    fn resolve_prefers_settings_and_secrets() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("provider", "openai").unwrap();
        store.set_setting("model", "gpt-x").unwrap();
        store
            .set_setting("openai_base_url", "http://localhost:11434")
            .unwrap();
        let secrets = MemoryStore::new();
        secrets.set(SECRET_OPENAI, "sk-test").unwrap();

        let cfg = Config::resolve(&store, &secrets);
        assert_eq!(cfg.provider, ProviderKind::OpenAi);
        assert_eq!(cfg.model, "gpt-x");
        assert_eq!(cfg.openai_api_key.as_deref(), Some("sk-test"));
        assert_eq!(cfg.effective_provider(), Some(ProviderKind::OpenAi));
    }

    #[test]
    fn resolve_per_provider_catalog_keys_and_bases() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("provider", "deepseek").unwrap();
        store
            .set_setting("provider.deepseek.base_url", "https://ds.example.com")
            .unwrap();
        let secrets = MemoryStore::new();
        secrets.set("deepseek_api_key", "dsk").unwrap();

        let cfg = Config::resolve(&store, &secrets);
        assert_eq!(cfg.provider_id, "deepseek");
        assert_eq!(cfg.provider, ProviderKind::OpenAi); // deepseek transport
        assert_eq!(cfg.key_for("deepseek").as_deref(), Some("dsk"));
        assert_eq!(cfg.base_for("deepseek"), "https://ds.example.com");
        assert_eq!(cfg.effective_provider(), Some(ProviderKind::OpenAi));
    }

    #[test]
    fn keyless_local_ollama_is_usable() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("provider", "ollama").unwrap();
        let secrets = MemoryStore::new();

        let cfg = Config::resolve(&store, &secrets);
        assert_eq!(cfg.provider_id, "ollama");
        assert!(cfg.key_for("ollama").is_none());
        assert_eq!(cfg.base_for("ollama"), "http://localhost:11434");
        assert_eq!(cfg.effective_provider(), Some(ProviderKind::OpenAi));
    }
}
