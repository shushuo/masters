//! **Outbound delivery** (Phase 3e, FR-27; ADR-0009) — pushes a routine's output off the agent
//! loop after it completes, over the two channels ADR-0009 puts in scope:
//!
//! 1. **OS notification** — on-device, no privacy boundary crossed.
//! 2. **Opt-in email digest** — user-configured SMTP, **off by default**, a `send` side-effect.
//!
//! Delivery is a *server-level component invoked by the Scheduler after a run* (docs/02 §1:
//! `Sched → Deliver → Perm`), **not** an MCP tool: recipes run headless with approvals cleared
//! (`without_approval`), so an in-loop tool would auto-approve and violate docs/06's rule that
//! `send` never sends silently. Instead the opt-in *is* the configuration — turning email on and
//! toggling a schedule's `deliver_email` flag is the standing per-target approval — and every send
//! is written to the audit log (the gate's record path). The SMTP wire + email config live here
//! (server-only `lettre` dep); the lean core only persists the per-schedule flags.

use std::sync::Arc;

use getmasters_core::permission::audit::redact_args;
use getmasters_core::secrets::SecretStore;
use getmasters_core::store::{AuditEntry, Store};

use crate::state::AppState;

/// Keychain entry holding the SMTP password.
pub const SECRET_SMTP_PASSWORD: &str = "smtp_password";

/// Resolved SMTP/email configuration (non-secret bits from the `settings` table; the password
/// comes from the keychain). Present only when email delivery is fully configured + enabled.
#[derive(Clone, Debug)]
pub struct EmailConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub from: String,
    pub to: String,
}

impl EmailConfig {
    /// Read the raw `settings` values (used by the `/settings/email` GET handler too).
    pub fn enabled(store: &Store) -> bool {
        store.get_setting("email_enabled").ok().flatten().as_deref() == Some("true")
    }

    /// Resolve a *complete, enabled* email config, or `None` if delivery is off or under-configured.
    pub fn resolve(store: &Store) -> Option<EmailConfig> {
        if !Self::enabled(store) {
            return None;
        }
        let get = |k: &str| {
            store
                .get_setting(k)
                .ok()
                .flatten()
                .filter(|v| !v.is_empty())
        };
        let host = get("smtp_host")?;
        let from = get("email_from")?;
        let to = get("email_to")?;
        let port = get("smtp_port")
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(587);
        Some(EmailConfig {
            host,
            port,
            username: get("smtp_username"),
            from,
            to,
        })
    }
}

/// One email to send.
#[derive(Clone, Debug)]
pub struct OutboundEmail {
    pub to: String,
    pub from: String,
    pub subject: String,
    pub body: String,
}

/// The SMTP wire, abstracted so tests inject a capturing fake (mirrors `SecretStore`/`Provider`).
pub trait EmailTransport: Send + Sync {
    /// Send `msg` over SMTP using `cfg` (+ `password`). Returns `Ok(())` on a delivered message.
    fn send(&self, cfg: &EmailConfig, password: &str, msg: &OutboundEmail) -> Result<(), String>;
}

/// The real transport: a blocking `lettre` SMTP client built from the resolved config. Only runs in
/// the live daemon — tests use [`CapturingTransport`], so lettre's TLS path isn't exercised in CI.
#[derive(Clone, Default)]
pub struct LettreTransport;

impl EmailTransport for LettreTransport {
    fn send(&self, cfg: &EmailConfig, password: &str, msg: &OutboundEmail) -> Result<(), String> {
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::{Message, SmtpTransport, Transport};

        let email = Message::builder()
            .from(
                msg.from
                    .parse()
                    .map_err(|e| format!("bad From address: {e}"))?,
            )
            .to(msg.to.parse().map_err(|e| format!("bad To address: {e}"))?)
            .subject(msg.subject.clone())
            .body(msg.body.clone())
            .map_err(|e| format!("build email: {e}"))?;

        let mut builder =
            SmtpTransport::relay(&cfg.host).map_err(|e| format!("smtp relay {}: {e}", cfg.host))?;
        builder = builder.port(cfg.port);
        if let Some(user) = &cfg.username {
            builder = builder.credentials(Credentials::new(user.clone(), password.to_string()));
        }
        builder
            .build()
            .send(&email)
            .map(|_| ())
            .map_err(|e| format!("smtp send: {e}"))
    }
}

/// Deliver a completed routine's `content` over the channels the schedule opted into. Never panics
/// or propagates — failures are logged + audited so one bad send can't stall the scheduler tick.
pub async fn deliver(
    state: &AppState,
    project_id: &str,
    session_id: Option<&str>,
    label: &str,
    content: &str,
    notify: bool,
    email: bool,
) {
    let store = state.agent.store().clone();

    // OS notification — on-device, no privacy boundary. The actual toast is the desktop's job
    // (Tauri notification plugin); the daemon records the intent + leaves a tracing breadcrumb.
    if notify {
        tracing::info!(project = %project_id, routine = %label, "delivery: notify");
        audit(
            &store,
            session_id,
            "notify.send",
            None,
            "auto",
            &format!("notification: {label}"),
        );
    }

    if !email {
        return;
    }

    // Email — a `send` side-effect. The opt-in is the config + the schedule's email flag; every
    // send (or skipped send) is audited. The body is redaction-aware (docs/06 §5).
    let Some(cfg) = EmailConfig::resolve(&store) else {
        audit(
            &store,
            session_id,
            "email.send",
            None,
            "denied",
            "email not configured",
        );
        return;
    };
    let password = state.secrets.get(SECRET_SMTP_PASSWORD).unwrap_or_default();
    let mut body = redacted_body(content);
    // Redaction mode (ADR-0016): mask monetary detail before it leaves the device.
    if redaction_enabled(&store) {
        body = redact_amounts(&body);
    }
    let msg = OutboundEmail {
        to: cfg.to.clone(),
        from: cfg.from.clone(),
        subject: format!("{label} — {}", crate::recipe::today_utc()),
        body,
    };
    // The args we audit: redaction-aware, and never the password (the keychain holds that).
    let args = serde_json::json!({ "to": msg.to, "subject": msg.subject });

    let transport = state.email.clone();
    let cfg_for_send = cfg.clone();
    let msg_for_send = msg.clone();
    let result = tokio::task::spawn_blocking(move || {
        transport.send(&cfg_for_send, &password, &msg_for_send)
    })
    .await
    .unwrap_or_else(|e| Err(format!("delivery task panicked: {e}")));

    match result {
        Ok(()) => {
            tracing::info!(project = %project_id, to = %msg.to, "delivery: emailed");
            audit(
                &store,
                session_id,
                "email.send",
                Some(&args),
                "approved",
                &format!("emailed {}", msg.to),
            );
        }
        Err(e) => {
            tracing::warn!(project = %project_id, error = %e, "delivery: email failed");
            audit(
                &store,
                session_id,
                "email.send",
                Some(&args),
                "approved",
                &format!("email failed: {e}"),
            );
        }
    }
}

/// Redact secret-shaped values from the run output before it leaves the device (docs/06 §5). The
/// content is plain text, so we wrap it as a JSON value and reuse the audit redactor, which scrubs
/// `key`/`token`/`secret`/`password`-shaped object fields if the output happens to be structured.
fn redacted_body(content: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(v @ serde_json::Value::Object(_)) | Ok(v @ serde_json::Value::Array(_)) => {
            redact_args(&v)
        }
        _ => content.to_string(),
    }
}

/// Whether **redaction mode** is on (ADR-0016): mask monetary detail before content leaves the
/// device. Off by default (only an explicit `"true"` enables it).
fn redaction_enabled(store: &Store) -> bool {
    matches!(store.get_setting("redaction_enabled"), Ok(Some(v)) if v == "true")
}

/// Redact monetary amounts from text for outbound delivery (ADR-0016 redaction mode). Masks decimal
/// figures (prices/amounts), currency-prefixed numbers, and large (≥5-digit) integers with `▢▢`,
/// while sparing percentages (returns), plain small integers, and dates. Pure + conservative:
/// numbers are the only thing touched, and the surrounding text/units are kept for readability.
pub fn redact_amounts(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // Read a full number token (digits + thousands separators + a decimal point).
        let start = i;
        let mut has_dot = false;
        let mut digits = 0usize;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == ',' || chars[i] == '.') {
            if chars[i] == '.' {
                has_dot = true;
            } else if chars[i].is_ascii_digit() {
                digits += 1;
            }
            i += 1;
        }
        let after = chars.get(i).copied();
        let before = start.checked_sub(1).and_then(|j| chars.get(j).copied());
        let is_pct = matches!(after, Some('%') | Some('％'));
        let currency = matches!(before, Some('¥') | Some('$') | Some('￥'));
        if !is_pct && (has_dot || currency || digits >= 5) {
            out.push('▢');
            out.push('▢');
        } else {
            out.extend(&chars[start..i]);
        }
    }
    out
}

fn audit(
    store: &Store,
    session_id: Option<&str>,
    tool: &str,
    args: Option<&serde_json::Value>,
    decision: &str,
    summary: &str,
) {
    let _ = store.insert_audit(&AuditEntry {
        session_id: session_id.map(|s| s.to_string()),
        tool: tool.to_string(),
        args: args.map(redact_args),
        decision: decision.to_string(),
        result_summary: Some(summary.to_string()),
    });
}

/// A no-op fallback transport (used if a daemon is started without configuring the real one). It
/// always errors so a misconfiguration is visible in the audit trail rather than silently dropping.
#[derive(Clone, Default)]
pub struct NullTransport;

impl EmailTransport for NullTransport {
    fn send(&self, _cfg: &EmailConfig, _pw: &str, _msg: &OutboundEmail) -> Result<(), String> {
        Err("no email transport configured".into())
    }
}

/// Shared helper for `AppState`'s default transport.
pub fn default_transport() -> Arc<dyn EmailTransport> {
    Arc::new(LettreTransport)
}

/// Convenience for reading the email config presence without resolving secrets.
pub fn email_enabled(secrets: &dyn SecretStore) -> bool {
    secrets.has(SECRET_SMTP_PASSWORD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_is_none_when_disabled_or_incomplete() {
        let store = Store::open_in_memory().unwrap();
        // Off by default.
        assert!(EmailConfig::resolve(&store).is_none());

        // Enabled but missing host/from/to → still None.
        store.set_setting("email_enabled", "true").unwrap();
        assert!(EmailConfig::resolve(&store).is_none());

        store.set_setting("smtp_host", "smtp.example.com").unwrap();
        store.set_setting("email_from", "bot@example.com").unwrap();
        store.set_setting("email_to", "me@example.com").unwrap();
        let cfg = EmailConfig::resolve(&store).expect("now fully configured");
        assert_eq!(cfg.host, "smtp.example.com");
        assert_eq!(cfg.port, 587); // default when unset
        assert_eq!(cfg.to, "me@example.com");
    }

    #[test]
    fn redact_amounts_masks_money_not_percentages_or_dates() {
        let s = "现价 1700.00 元，成本 ¥1699.50，持仓市值 120000，收益 +10.5%，日期 2026-07-15。";
        let r = redact_amounts(s);
        assert!(!r.contains("1700.00"), "price masked");
        assert!(!r.contains("1699.50"), "currency amount masked");
        assert!(!r.contains("120000"), "large amount masked");
        assert!(r.contains("+10.5%"), "percentage kept");
        assert!(r.contains("2026-07-15"), "date kept");
        assert!(r.contains('▢'));
    }

    #[test]
    fn body_redacts_structured_secrets() {
        let plain = "Weekly digest: 3 files changed.";
        assert_eq!(redacted_body(plain), plain);

        let structured = r#"{"summary":"ok","api_key":"sk-leak"}"#;
        let out = redacted_body(structured);
        assert!(!out.contains("sk-leak"));
        assert!(out.contains("***"));
    }
}
