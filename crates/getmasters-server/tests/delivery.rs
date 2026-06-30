//! Outbound delivery (Phase 3e, FR-27): drive `run_due` for a schedule that opted into email, with
//! a capturing transport injected, and assert the routine output is sent + audited — never touching
//! a real SMTP server. A second case asserts the notify channel is audited; a third asserts an
//! email-opted schedule with no config is skipped + audited `denied`.

use std::sync::{Arc, Mutex};

use getmasters_core::agent::AgentService;
use getmasters_core::config::Config;
use getmasters_core::provider::MockProvider;
use getmasters_core::store::Store;
use getmasters_server::delivery::{
    EmailConfig, EmailTransport, OutboundEmail, SECRET_SMTP_PASSWORD,
};
use getmasters_server::{scheduler, AppState};

/// Records every message it's asked to "send" instead of hitting SMTP.
#[derive(Clone, Default)]
struct CapturingTransport {
    sent: Arc<Mutex<Vec<OutboundEmail>>>,
}

impl EmailTransport for CapturingTransport {
    fn send(&self, _cfg: &EmailConfig, _pw: &str, msg: &OutboundEmail) -> Result<(), String> {
        self.sent.lock().unwrap().push(msg.clone());
        Ok(())
    }
}

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("getmasters-deliv-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// Build a state whose project agent runs the mock provider, with a capturing email transport.
fn setup(
    store: &Store,
    dir: &std::path::Path,
    transport: CapturingTransport,
) -> (AppState, String) {
    let pid = store.create_project("automation", None).unwrap();
    let agent = AgentService::new(store.clone(), Arc::new(MockProvider::new()), "mock");
    let cfg = Config {
        db_path: dir.join("getmasters.db"),
        ..Default::default()
    };
    let state = AppState::new(agent, "t".to_string())
        .with_config(cfg)
        .with_email_transport(Arc::new(transport));
    (state, pid)
}

fn save_echo_recipe(state: &AppState, store: &Store, pid: &str, name: &str, prompt: &str) {
    getmasters_server::recipe::RecipeStore::new(
        state.project_dir(pid),
        pid.to_string(),
        store.clone(),
    )
    .save(&getmasters_proto::RecipeDto {
        name: name.into(),
        title: name.into(),
        description: String::new(),
        parameters: vec![],
        prompt: prompt.into(),
        extensions: vec![],
    })
    .unwrap();
}

/// Audit rows for a tool name across the whole log (decision-tagged).
fn audit_decisions(store: &Store, session: &str, tool: &str) -> Vec<String> {
    store
        .list_audit(session)
        .unwrap()
        .into_iter()
        .filter(|(t, _, _)| t == tool)
        .map(|(_, d, _)| d)
        .collect()
}

#[tokio::test]
async fn scheduled_email_is_sent_and_audited() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let transport = CapturingTransport::default();
    let (state, pid) = setup(&store, &dir, transport.clone());

    // Configure + enable email delivery.
    store.set_setting("email_enabled", "true").unwrap();
    store.set_setting("smtp_host", "smtp.example.com").unwrap();
    store.set_setting("email_from", "bot@example.com").unwrap();
    store.set_setting("email_to", "me@example.com").unwrap();
    state.secrets.set(SECRET_SMTP_PASSWORD, "hunter2").unwrap();

    // The mock provider echoes the prompt back as the assistant message → that's the run output.
    save_echo_recipe(&state, &store, &pid, "digest", "weekly summary body");

    let sid = store
        .create_schedule(&pid, "digest", "{}", "once", None, Some(1_000), false, true)
        .unwrap();
    scheduler::run_due(&state, 2_000_000_000_000).await;

    // Exactly one email captured, addressed + subject-tagged with the routine name.
    let sent = transport.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "one email should be sent");
    assert_eq!(sent[0].to, "me@example.com");
    assert_eq!(sent[0].from, "bot@example.com");
    assert!(
        sent[0].subject.starts_with("digest — "),
        "subject: {}",
        sent[0].subject
    );
    assert!(
        sent[0].body.contains("weekly summary body"),
        "body: {}",
        sent[0].body
    );

    // The send is audited `approved` against the run's session.
    let run = &store.list_scheduled_runs(&sid).unwrap()[0];
    let session = run.session_id.clone().unwrap();
    assert_eq!(
        audit_decisions(&store, &session, "email.send"),
        vec!["approved"]
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn notify_only_is_audited_on_device() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let transport = CapturingTransport::default();
    let (state, pid) = setup(&store, &dir, transport.clone());

    save_echo_recipe(&state, &store, &pid, "ping", "ping body");
    let sid = store
        .create_schedule(&pid, "ping", "{}", "once", None, Some(1_000), true, false)
        .unwrap();
    scheduler::run_due(&state, 2_000_000_000_000).await;

    // No email (flag off), and the notification is recorded `auto` (on-device).
    assert!(transport.sent.lock().unwrap().is_empty());
    let session = store.list_scheduled_runs(&sid).unwrap()[0]
        .session_id
        .clone()
        .unwrap();
    assert_eq!(
        audit_decisions(&store, &session, "notify.send"),
        vec!["auto"]
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn email_opted_without_config_is_denied() {
    let dir = temp_dir();
    let store = Store::open_in_memory().unwrap();
    let transport = CapturingTransport::default();
    let (state, pid) = setup(&store, &dir, transport.clone());

    // Email flag set, but delivery is NOT configured/enabled.
    save_echo_recipe(&state, &store, &pid, "digest", "body");
    let sid = store
        .create_schedule(&pid, "digest", "{}", "once", None, Some(1_000), false, true)
        .unwrap();
    scheduler::run_due(&state, 2_000_000_000_000).await;

    assert!(
        transport.sent.lock().unwrap().is_empty(),
        "nothing leaves when unconfigured"
    );
    let session = store.list_scheduled_runs(&sid).unwrap()[0]
        .session_id
        .clone()
        .unwrap();
    assert_eq!(
        audit_decisions(&store, &session, "email.send"),
        vec!["denied"]
    );

    std::fs::remove_dir_all(&dir).ok();
}
