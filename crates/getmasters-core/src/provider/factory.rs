//! Provider selection.
//!
//! Two entry points:
//! - [`build_provider`] — the daemon/CLI default: pick a provider from [`Config`]. Returns an
//!   error when no usable provider is configured (no offline fallback; the daemon then refuses
//!   to start). Under the `testing` feature it yields the offline `MockProvider` instead.
//! - [`resolve_provider`] — resolve a **provider-qualified** model id (`anthropic:…`,
//!   `openai:…`, `groq:…`, …) to a provider + bare model (ADR-0013). Per-master dispatch
//!   reuses this.

use std::sync::Arc;

use crate::config::{is_local_base, Config, ProviderKind};
use crate::provider::catalog::{self, ProviderTransport};

use super::ProviderError;
use super::{AnthropicProvider, OpenAiProvider, Provider};

/// Legacy OpenAI-compatible presets not surfaced in the catalog: `prefix → base_url`. These reuse
/// the `openai` key (best-effort back-compat for older provider-qualified model ids).
const PRESETS: &[(&str, &str)] = &[
    ("groq", "https://api.groq.com/openai"),
    ("together", "https://api.together.xyz"),
];

fn preset_base(prefix: &str) -> Option<&'static str> {
    PRESETS.iter().find(|(p, _)| *p == prefix).map(|(_, b)| *b)
}

/// Build the effective default provider, or an error when none is configured with usable
/// credentials. The error case is unconditional (no offline fallback even under `testing`), so
/// the daemon refuses to start without a key regardless of how it was built — tests that want
/// the offline mock construct [`super::MockProvider`] directly instead of going through here.
pub fn build_provider(cfg: &Config) -> Result<Arc<dyn Provider>, ProviderError> {
    let id = cfg.active_id();
    match cfg.effective_provider() {
        Some(ProviderKind::Anthropic) => Ok(Arc::new(AnthropicProvider::new(
            cfg.key_for(id).expect("anthropic key guaranteed"),
        ))),
        Some(ProviderKind::OpenAi) => {
            // `openai` keeps the dedicated default; other OpenAI-compatible vendors use their
            // catalog base (or override). A local base needs no key.
            let base = if id == "openai" {
                cfg.openai_base()
            } else {
                cfg.base_for(id)
            };
            let key = if is_local_base(&base) {
                None
            } else {
                cfg.key_for(id)
            };
            Ok(Arc::new(OpenAiProvider::new(base, key)))
        }
        None => Err(ProviderError::NotConfigured(
            "no usable LLM provider — set an API key (ANTHROPIC_API_KEY / OPENAI_API_KEY) or a \
             local OpenAI base URL"
                .into(),
        )),
    }
}

/// The default provider for a bare/unknown model id. In production the daemon validated a usable
/// provider at startup, so this succeeds; under the `testing` feature a keyless config falls back
/// to the offline mock (so headless tests that pass a bare model don't panic).
fn default_provider(cfg: &Config) -> Arc<dyn Provider> {
    match build_provider(cfg) {
        Ok(p) => p,
        #[cfg(feature = "testing")]
        Err(_) => Arc::new(super::MockProvider::new()),
        #[cfg(not(feature = "testing"))]
        Err(e) => panic!("no usable provider for a bare model id: {e}"),
    }
}

/// Resolve a (possibly provider-qualified) model id to a provider + the bare model string.
///
/// `anthropic:claude-…`, `openai:gpt-…`, `groq:…`, `together:…`, `openrouter:…`,
/// `ollama:llama3`. A bare model (no prefix) uses [`build_provider`] and the config model.
/// Infallible: the bare/unknown branch assumes the daemon validated a usable provider at
/// startup. Under the `testing` feature the `mock:` prefix and a keyless `anthropic:` resolve
/// to the offline mock; in production a keyless `anthropic:` yields a real (empty-key)
/// provider whose calls fail clearly with `Auth`.
pub fn resolve_provider(cfg: &Config, qualified_model: &str) -> (Arc<dyn Provider>, String) {
    let Some((prefix, model)) = qualified_model.split_once(':') else {
        return (default_provider(cfg), qualified_model.to_string());
    };
    let model = model.to_string();

    #[cfg(feature = "testing")]
    if prefix == "mock" {
        return (Arc::new(super::MockProvider::new()), model);
    }

    // A catalog vendor → build with *that vendor's own* key + base (the per-provider-key fix).
    if let Some(entry) = catalog::find(prefix) {
        return match entry.transport {
            ProviderTransport::Anthropic => match cfg.key_for(prefix) {
                Some(key) => (Arc::new(AnthropicProvider::new(key)), model),
                #[cfg(feature = "testing")]
                None => (Arc::new(super::MockProvider::new()), model),
                #[cfg(not(feature = "testing"))]
                None => (Arc::new(AnthropicProvider::new(String::new())), model),
            },
            ProviderTransport::OpenAiCompatible => {
                let base = if prefix == "openai" {
                    cfg.openai_base()
                } else {
                    cfg.base_for(prefix)
                };
                let key = if is_local_base(&base) {
                    None
                } else {
                    cfg.key_for(prefix)
                };
                // Carry the bare model as the embeddings model too (chat uses ChatRequest.model,
                // so this is only consulted by `embed`).
                (
                    Arc::new(OpenAiProvider::new(base, key).with_embed_model(model.clone())),
                    model,
                )
            }
        };
    }

    // Legacy preset (groq/together) not in the catalog → reuse the openai key.
    if let Some(base) = preset_base(prefix) {
        let key = if is_local_base(base) {
            None
        } else {
            cfg.key_for("openai")
        };
        return (
            Arc::new(OpenAiProvider::new(base.to_string(), key).with_embed_model(model.clone())),
            model,
        );
    }

    // Unknown prefix → treat the whole thing as a model on the default provider.
    (default_provider(cfg), qualified_model.to_string())
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn errors_without_usable_provider() {
        // No offline fallback: a config with no usable credentials is an error (the daemon then
        // refuses to start). Tests that want the mock construct it directly.
        assert!(build_provider(&Config::default()).is_err());
    }

    #[test]
    fn anthropic_when_selected_with_key() {
        let cfg = Config {
            provider: ProviderKind::Anthropic,
            anthropic_api_key: Some("sk-test".into()),
            ..Config::default()
        };
        assert_eq!(build_provider(&cfg).unwrap().name(), "anthropic");
    }

    #[test]
    fn openai_when_selected_with_key() {
        let cfg = Config {
            provider: ProviderKind::OpenAi,
            openai_api_key: Some("sk-test".into()),
            ..Config::default()
        };
        assert_eq!(build_provider(&cfg).unwrap().name(), "openai");
    }

    #[test]
    fn openai_keyless_local_base_is_allowed() {
        let cfg = Config {
            provider: ProviderKind::OpenAi,
            openai_base_url: Some("http://localhost:11434".into()),
            ..Config::default()
        };
        assert_eq!(cfg.effective_provider(), Some(ProviderKind::OpenAi));
        assert_eq!(build_provider(&cfg).unwrap().name(), "openai");
    }

    #[test]
    fn resolve_qualified_models() {
        let cfg = Config {
            anthropic_api_key: Some("ak".into()),
            openai_api_key: Some("ok".into()),
            ..Config::default()
        };
        let (p, m) = resolve_provider(&cfg, "anthropic:claude-opus-4-8");
        assert_eq!(p.name(), "anthropic");
        assert_eq!(m, "claude-opus-4-8");

        let (p, m) = resolve_provider(&cfg, "groq:llama-3.1-70b"); // legacy preset
        assert_eq!(p.name(), "openai");
        assert_eq!(m, "llama-3.1-70b");

        let (p, _) = resolve_provider(&cfg, "ollama:llama3"); // catalog, keyless local
        assert_eq!(p.name(), "openai");

        let (p, m) = resolve_provider(&cfg, "bare-model");
        assert_eq!(p.name(), "anthropic"); // bare model → default provider (has anthropic key)
        assert_eq!(m, "bare-model");
    }

    #[test]
    fn resolve_uses_per_provider_key_and_base() {
        // Regression for the shared-`openai_api_key` bug: a catalog vendor must dispatch with its
        // OWN key + base, independent of the default provider's openai key. The factory only
        // exposes `name()` through the trait object, so we assert routing here and verify the
        // per-provider key/base plumbing via the `Config` accessors it consumes.
        let mut keys = std::collections::HashMap::new();
        keys.insert("deepseek".to_string(), "dsk".to_string());
        keys.insert("openai".to_string(), "ok".to_string());
        let cfg = Config {
            provider_keys: keys,
            ..Config::default()
        };

        let (p, m) = resolve_provider(&cfg, "deepseek:deepseek-chat");
        assert_eq!(p.name(), "openai"); // OpenAI-compatible transport
        assert_eq!(m, "deepseek-chat");
        // The vendor's own key + base, not the shared openai key.
        assert_eq!(cfg.key_for("deepseek").as_deref(), Some("dsk"));
        assert_eq!(cfg.base_for("deepseek"), "https://api.deepseek.com");
        assert_eq!(
            cfg.base_for("gemini"),
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );

        let (p, _) = resolve_provider(&cfg, "gemini:gemini-2.0-flash");
        assert_eq!(p.name(), "openai");
    }
}
