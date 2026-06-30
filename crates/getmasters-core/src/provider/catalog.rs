//! The provider catalog — the set of LLM vendors the user can configure (ADR-0003/0013).
//!
//! Each [`CatalogEntry`] maps a vendor to one of two **transports**: Anthropic-native or an
//! OpenAI-compatible HTTP endpoint. We deliberately do *not* add a [`ProviderKind`] variant per
//! vendor — every OpenAI-compatible vendor (DeepSeek, Gemini, OpenRouter, Ollama, DashScope, …)
//! reuses the same [`OpenAiProvider`](super::OpenAiProvider) with its own base URL + key, keeping
//! the lean core free of per-vendor code.
//!
//! Per-provider config reuses existing seams: the API key lives in the OS keychain under
//! [`secret_name`] (`{id}_api_key`), and an optional base-URL override lives in the generic
//! `settings` KV table under [`base_setting_key`] (`provider.{id}.base_url`). The active default
//! provider is the `provider` setting (any catalog id).

use crate::config::ProviderKind;

/// How a provider speaks on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderTransport {
    /// Native Anthropic/Claude API.
    Anthropic,
    /// OpenAI-compatible `/v1/chat/completions` endpoint.
    OpenAiCompatible,
}

impl ProviderTransport {
    /// The routing [`ProviderKind`] this transport dispatches through.
    pub fn kind(self) -> ProviderKind {
        match self {
            ProviderTransport::Anthropic => ProviderKind::Anthropic,
            ProviderTransport::OpenAiCompatible => ProviderKind::OpenAi,
        }
    }

    /// Stable wire label used by the proto DTOs.
    pub fn label(self) -> &'static str {
        match self {
            ProviderTransport::Anthropic => "anthropic",
            ProviderTransport::OpenAiCompatible => "openai_compatible",
        }
    }
}

/// One configurable provider in the catalog.
#[derive(Clone, Copy, Debug)]
pub struct CatalogEntry {
    /// Stable id (also the secret-name + settings-key prefix), e.g. `"deepseek"`.
    pub id: &'static str,
    /// Human-readable label for the UI.
    pub label: &'static str,
    /// Transport / routing kind.
    pub transport: ProviderTransport,
    /// Default base URL (OpenAI-compatible vendors only; `None` for Anthropic + the custom slot).
    pub default_base: Option<&'static str>,
    /// Documentation / API-key page.
    pub docs_url: &'static str,
    /// A local/loopback endpoint (e.g. Ollama) that needs no API key.
    pub is_local: bool,
    /// The generic "Custom OpenAI-compatible" slot — the user supplies the base URL.
    pub custom: bool,
}

/// The configurable providers (ADR-0013). OpenAI-compatible vendors share [`OpenAiProvider`].
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "anthropic",
        label: "Anthropic (Claude)",
        transport: ProviderTransport::Anthropic,
        default_base: None,
        docs_url: "https://console.anthropic.com/settings/keys",
        is_local: false,
        custom: false,
    },
    CatalogEntry {
        id: "openai",
        label: "OpenAI",
        transport: ProviderTransport::OpenAiCompatible,
        default_base: Some("https://api.openai.com"),
        docs_url: "https://platform.openai.com/api-keys",
        is_local: false,
        custom: false,
    },
    CatalogEntry {
        id: "deepseek",
        label: "DeepSeek",
        transport: ProviderTransport::OpenAiCompatible,
        default_base: Some("https://api.deepseek.com"),
        docs_url: "https://platform.deepseek.com/api_keys",
        is_local: false,
        custom: false,
    },
    CatalogEntry {
        id: "gemini",
        label: "Google Gemini",
        transport: ProviderTransport::OpenAiCompatible,
        default_base: Some("https://generativelanguage.googleapis.com/v1beta/openai"),
        docs_url: "https://aistudio.google.com/app/apikey",
        is_local: false,
        custom: false,
    },
    CatalogEntry {
        id: "openrouter",
        label: "OpenRouter",
        transport: ProviderTransport::OpenAiCompatible,
        default_base: Some("https://openrouter.ai/api"),
        docs_url: "https://openrouter.ai/keys",
        is_local: false,
        custom: false,
    },
    CatalogEntry {
        id: "ollama",
        label: "Ollama (local)",
        transport: ProviderTransport::OpenAiCompatible,
        default_base: Some("http://localhost:11434"),
        docs_url: "https://ollama.com",
        is_local: true,
        custom: false,
    },
    CatalogEntry {
        id: "dashscope",
        label: "DashScope (Qwen)",
        transport: ProviderTransport::OpenAiCompatible,
        default_base: Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
        docs_url: "https://bailian.console.aliyun.com/?apiKey=1",
        is_local: false,
        custom: false,
    },
    CatalogEntry {
        id: "glm",
        label: "GLM (Z.AI / Zhipu)",
        transport: ProviderTransport::OpenAiCompatible,
        default_base: Some("https://open.bigmodel.cn/api/paas/v4"),
        docs_url: "https://open.bigmodel.cn/usercenter/apikeys",
        is_local: false,
        custom: false,
    },
    CatalogEntry {
        id: "moonshot",
        label: "Moonshot (Kimi)",
        transport: ProviderTransport::OpenAiCompatible,
        default_base: Some("https://api.moonshot.cn/v1"),
        docs_url: "https://platform.moonshot.cn/console/api-keys",
        is_local: false,
        custom: false,
    },
    CatalogEntry {
        id: "minimax",
        label: "MiniMax",
        transport: ProviderTransport::OpenAiCompatible,
        default_base: Some("https://api.minimaxi.com/v1"),
        docs_url: "https://platform.minimaxi.com/user-center/basic-information/interface-key",
        is_local: false,
        custom: false,
    },
    CatalogEntry {
        id: "custom",
        label: "Custom (OpenAI-compatible)",
        transport: ProviderTransport::OpenAiCompatible,
        default_base: None,
        docs_url: "",
        is_local: false,
        custom: true,
    },
];

/// Look up a catalog entry by id.
pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

/// The keychain secret name for a provider id (`{id}_api_key`).
pub fn secret_name(id: &str) -> String {
    format!("{id}_api_key")
}

/// The `settings` KV key holding a provider's base-URL override (`provider.{id}.base_url`).
pub fn base_setting_key(id: &str) -> String {
    format!("provider.{id}.base_url")
}

/// Whether `name` is a valid provider-key secret name for some catalog entry.
pub fn is_valid_secret_name(name: &str) -> bool {
    CATALOG.iter().any(|e| secret_name(e.id) == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_helpers_round_trip() {
        let e = find("deepseek").expect("deepseek in catalog");
        assert_eq!(e.transport, ProviderTransport::OpenAiCompatible);
        assert_eq!(e.transport.kind(), ProviderKind::OpenAi);
        assert_eq!(secret_name("deepseek"), "deepseek_api_key");
        assert_eq!(base_setting_key("deepseek"), "provider.deepseek.base_url");
        assert!(is_valid_secret_name("deepseek_api_key"));
        assert!(is_valid_secret_name("anthropic_api_key"));
        assert!(!is_valid_secret_name("smtp_password"));
        assert!(!is_valid_secret_name("nope_api_key"));
    }

    #[test]
    fn legacy_ids_present_and_anthropic_native() {
        assert!(find("anthropic").unwrap().transport == ProviderTransport::Anthropic);
        assert!(find("openai").unwrap().transport == ProviderTransport::OpenAiCompatible);
        assert!(find("ollama").unwrap().is_local);
        assert!(find("custom").unwrap().custom);
    }
}
