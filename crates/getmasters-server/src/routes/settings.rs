//! Settings + secrets management (docs/06 §4). Non-secret settings live in the DB; API keys
//! live in the OS keychain and are never returned — only their presence is reported.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use getmasters_core::config::{is_local_base, Config, ProviderKind};
use getmasters_core::provider::{build_provider, catalog, ChatMessage, ChatRequest, ProviderError};
use getmasters_proto::{
    ConfigCheckDto, ConfigCheckItem, EmailSettingsDto, EmailSettingsUpdate, EnvironmentDto,
    ProviderStateDto, ProvidersDto, SecretUpdate, SettingsDto, SettingsUpdate,
};

use crate::delivery::SECRET_SMTP_PASSWORD;
use crate::state::{AppError, AppState};

fn provider_label(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAi => "openai",
    }
}

/// Label for the effective provider, which is `None` when no usable credentials are configured.
fn effective_label(kind: Option<ProviderKind>) -> &'static str {
    kind.map(provider_label).unwrap_or("unconfigured")
}

fn current(state: &AppState) -> SettingsDto {
    let cfg = Config::resolve(state.agent.store(), state.secrets.as_ref());
    SettingsDto {
        provider: cfg.active_id().to_string(),
        model: cfg.model,
        openai_base_url: cfg.openai_base_url,
        anthropic_key_set: cfg.anthropic_api_key.is_some(),
        openai_key_set: cfg.openai_api_key.is_some(),
    }
}

#[utoipa::path(
    get,
    path = "/settings",
    operation_id = "get_settings",
    responses((status = 200, description = "Current effective settings", body = SettingsDto)),
    tag = "settings"
)]
pub async fn get(State(state): State<AppState>) -> Json<SettingsDto> {
    Json(current(&state))
}

#[utoipa::path(
    put,
    path = "/settings",
    operation_id = "update_settings",
    request_body = SettingsUpdate,
    responses((status = 200, description = "Updated settings", body = SettingsDto)),
    tag = "settings"
)]
pub async fn update(
    State(state): State<AppState>,
    Json(body): Json<SettingsUpdate>,
) -> Result<Json<SettingsDto>, AppError> {
    let store = state.agent.store();
    if let Some(p) = &body.provider {
        if catalog::find(p).is_none() {
            return Err(AppError::new(
                StatusCode::BAD_REQUEST,
                format!("unknown provider '{p}'"),
            ));
        }
        store.set_setting("provider", p)?;
    }
    if let Some(m) = &body.model {
        store.set_setting("model", m)?;
    }
    if let Some(b) = &body.openai_base_url {
        store.set_setting("openai_base_url", b)?;
    }
    if let Some(bases) = &body.provider_bases {
        for (id, base) in bases {
            if catalog::find(id).is_none() {
                return Err(AppError::new(
                    StatusCode::BAD_REQUEST,
                    format!("unknown provider '{id}'"),
                ));
            }
            store.set_setting(&catalog::base_setting_key(id), base)?;
        }
    }
    // Provider/model changes take effect for new daemon launches; active sessions keep the
    // provider resolved at startup (live provider swap is a later refinement).
    Ok(Json(current(&state)))
}

#[utoipa::path(
    get,
    path = "/settings/providers",
    operation_id = "get_providers",
    responses((status = 200, description = "The configurable provider catalog + active default", body = ProvidersDto)),
    tag = "settings"
)]
pub async fn providers(State(state): State<AppState>) -> Json<ProvidersDto> {
    let cfg = Config::resolve(state.agent.store(), state.secrets.as_ref());
    let providers = catalog::CATALOG
        .iter()
        .map(|e| ProviderStateDto {
            id: e.id.to_string(),
            label: e.label.to_string(),
            transport: e.transport.label().to_string(),
            default_base: e.default_base.map(str::to_string),
            base_url: cfg.provider_bases.get(e.id).cloned(),
            docs_url: e.docs_url.to_string(),
            is_local: e.is_local,
            custom: e.custom,
            key_set: cfg.key_for(e.id).is_some(),
        })
        .collect();
    Json(ProvidersDto {
        active: cfg.active_id().to_string(),
        providers,
    })
}

/// Where an effective setting resolves from: a persisted DB value wins, else a non-empty env var,
/// else the built-in default (mirrors [`Config::resolve`]'s precedence).
fn setting_source(state: &AppState, db_key: &str, env_key: &str) -> &'static str {
    if state
        .agent
        .store()
        .get_setting(db_key)
        .ok()
        .flatten()
        .is_some()
    {
        "settings"
    } else if std::env::var(env_key)
        .ok()
        .is_some_and(|v| !v.trim().is_empty())
    {
        "env"
    } else {
        "default"
    }
}

/// The Masters/provider env vars surfaced (by name only) in the environment report.
const REPORTED_ENV_VARS: &[&str] = &[
    "GETMASTERS_HOME",
    "GETMASTERS_DB_PATH",
    "GETMASTERS_PROVIDER",
    "GETMASTERS_MODEL",
    "OPENAI_BASE_URL",
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
];

#[utoipa::path(
    get,
    path = "/settings/environment",
    operation_id = "get_environment",
    responses((status = 200, description = "Resolved runtime environment", body = EnvironmentDto)),
    tag = "settings"
)]
pub async fn environment(State(state): State<AppState>) -> Json<EnvironmentDto> {
    let cfg = Config::resolve(state.agent.store(), state.secrets.as_ref());
    let env_overrides = REPORTED_ENV_VARS
        .iter()
        .filter(|k| std::env::var(k).ok().is_some_and(|v| !v.trim().is_empty()))
        .map(|k| k.to_string())
        .collect();
    Json(EnvironmentDto {
        data_home: crate::home::data_home().display().to_string(),
        db_path: crate::home::db_path().display().to_string(),
        configured_provider: cfg.active_id().to_string(),
        effective_provider: effective_label(cfg.effective_provider()).to_string(),
        openai_base_url: cfg.openai_base_url.clone(),
        anthropic_key_set: cfg.anthropic_api_key.is_some(),
        openai_key_set: cfg.openai_api_key.is_some(),
        provider_source: setting_source(&state, "provider", "GETMASTERS_PROVIDER").to_string(),
        model_source: setting_source(&state, "model", "GETMASTERS_MODEL").to_string(),
        base_url_source: setting_source(&state, "openai_base_url", "OPENAI_BASE_URL").to_string(),
        model: cfg.model,
        env_overrides,
    })
}

fn check_item(name: &str, status: &str, detail: impl Into<String>) -> ConfigCheckItem {
    ConfigCheckItem {
        name: name.to_string(),
        status: status.to_string(),
        detail: detail.into(),
    }
}

#[utoipa::path(
    post,
    path = "/settings/check",
    operation_id = "check_config",
    responses((status = 200, description = "Config validation result", body = ConfigCheckDto)),
    tag = "settings"
)]
pub async fn check(State(state): State<AppState>) -> Json<ConfigCheckDto> {
    let cfg = Config::resolve(state.agent.store(), state.secrets.as_ref());
    let effective = cfg.effective_provider();
    let mut checks: Vec<ConfigCheckItem> = Vec::new();

    // 1. Effective provider — error when the configured provider has no usable credentials
    //    (there is no offline fallback; the daemon refuses to start in this state).
    match effective {
        Some(kind) => checks.push(check_item(
            "provider",
            "ok",
            format!("using provider '{}'", provider_label(kind)),
        )),
        None => checks.push(check_item(
            "provider",
            "error",
            format!(
                "configured provider '{}' has no usable credentials",
                provider_label(cfg.provider)
            ),
        )),
    }

    // 2. Key presence / base-URL coherence for the configured provider.
    match cfg.provider {
        ProviderKind::Anthropic => checks.push(if cfg.anthropic_api_key.is_some() {
            check_item("anthropic_api_key", "ok", "API key is set")
        } else {
            check_item(
                "anthropic_api_key",
                "error",
                "no Anthropic API key configured",
            )
        }),
        ProviderKind::OpenAi => {
            let base = cfg.openai_base();
            if cfg.openai_api_key.is_some() {
                checks.push(check_item("openai_api_key", "ok", "API key is set"));
            } else if is_local_base(&base) {
                checks.push(check_item(
                    "openai_api_key",
                    "ok",
                    format!("local base URL '{base}' needs no API key"),
                ));
            } else {
                checks.push(check_item(
                    "openai_api_key",
                    "error",
                    format!("no OpenAI API key configured for base URL '{base}'"),
                ));
            }
        }
    }

    // 3. Live test call — a tiny chat round-trip (only when a provider is usable).
    match build_provider(&cfg) {
        Ok(provider) => {
            let mut req = ChatRequest::new(cfg.model.clone(), vec![ChatMessage::user("ping")]);
            req.max_tokens = 16;
            match provider.chat(req).await {
                Ok(_) => checks.push(check_item(
                    "live_call",
                    "ok",
                    format!("{} responded to a test request", provider.name()),
                )),
                Err(ProviderError::Auth) => checks.push(check_item(
                    "live_call",
                    "error",
                    "authentication failed — check the API key",
                )),
                Err(e) => checks.push(check_item(
                    "live_call",
                    "error",
                    format!("provider error: {e}"),
                )),
            }
        }
        Err(e) => checks.push(check_item(
            "live_call",
            "error",
            format!("no usable provider — {e}"),
        )),
    }

    let ok = !checks.iter().any(|c| c.status == "error");
    Json(ConfigCheckDto {
        ok,
        effective_provider: effective_label(effective).to_string(),
        checks,
    })
}

#[utoipa::path(
    put,
    path = "/settings/secret",
    operation_id = "set_secret",
    request_body = SecretUpdate,
    responses((status = 204, description = "Secret stored")),
    tag = "settings"
)]
pub async fn set_secret(
    State(state): State<AppState>,
    Json(body): Json<SecretUpdate>,
) -> Result<StatusCode, AppError> {
    if !(catalog::is_valid_secret_name(&body.name) || body.name == SECRET_SMTP_PASSWORD) {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            format!("unknown secret '{}'", body.name),
        ));
    }
    state
        .secrets
        .set(&body.name, &body.value)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/settings/secret/{name}",
    operation_id = "delete_secret",
    params(("name" = String, Path, description = "Secret name")),
    responses((status = 204, description = "Secret removed")),
    tag = "settings"
)]
pub async fn delete_secret(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .secrets
        .delete(&name)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::NO_CONTENT)
}

fn current_email(state: &AppState) -> EmailSettingsDto {
    let store = state.agent.store();
    let get = |k: &str| {
        store
            .get_setting(k)
            .ok()
            .flatten()
            .filter(|v| !v.is_empty())
    };
    EmailSettingsDto {
        enabled: get("email_enabled").as_deref() == Some("true"),
        host: get("smtp_host"),
        port: get("smtp_port").and_then(|p| p.parse().ok()),
        username: get("smtp_username"),
        from: get("email_from"),
        to: get("email_to"),
        password_set: state.secrets.has(SECRET_SMTP_PASSWORD),
    }
}

#[utoipa::path(
    get,
    path = "/settings/email",
    operation_id = "get_email_settings",
    responses((status = 200, description = "Current email-delivery settings", body = EmailSettingsDto)),
    tag = "settings"
)]
pub async fn get_email(State(state): State<AppState>) -> Json<EmailSettingsDto> {
    Json(current_email(&state))
}

#[utoipa::path(
    put,
    path = "/settings/email",
    operation_id = "update_email_settings",
    request_body = EmailSettingsUpdate,
    responses((status = 200, description = "Updated email-delivery settings", body = EmailSettingsDto)),
    tag = "settings"
)]
pub async fn update_email(
    State(state): State<AppState>,
    Json(body): Json<EmailSettingsUpdate>,
) -> Result<Json<EmailSettingsDto>, AppError> {
    let store = state.agent.store();
    if let Some(e) = body.enabled {
        store.set_setting("email_enabled", if e { "true" } else { "false" })?;
    }
    if let Some(h) = &body.host {
        store.set_setting("smtp_host", h)?;
    }
    if let Some(p) = body.port {
        store.set_setting("smtp_port", &p.to_string())?;
    }
    if let Some(u) = &body.username {
        store.set_setting("smtp_username", u)?;
    }
    if let Some(f) = &body.from {
        store.set_setting("email_from", f)?;
    }
    if let Some(t) = &body.to {
        store.set_setting("email_to", t)?;
    }
    Ok(Json(current_email(&state)))
}
