//! **Masters** run path (Phase 4a, FR-39/46; ADR-0010/0013) — hands a brief to a single master,
//! which runs as an isolated, gated subagent on **its own persona + model + tool allow-list**.
//!
//! Two backends produce the **same** `AgentEvent` stream (Phase 4i, ADR-0014), so single-run, team,
//! and group-chat consume a master uniformly via [`run_master_stream`]:
//! - **internal** — the persona-over-model loop ([`AgentService`]): persona injected
//!   ([`AgentService::with_persona`]), provider-qualified `default_model` dispatched
//!   ([`AgentService::with_model_provider`]), `allowed_tools` restricting the run.
//! - **acp** — an external ACP coding harness driven over stdio ([`crate::acp`]); its file/permission
//!   callbacks route through the same Permission & Audit gate.
//!
//! The master file is the source of truth; [`getmasters_core::masters::MasterStore`] loads it. Both
//! backends run grant-bounded + audited (the grant boundary is the security line).

use std::path::PathBuf;
use std::pin::Pin;

use futures::{Stream, StreamExt};

use getmasters_core::agent::AgentEvent;
use getmasters_core::masters::{Master, MasterStore};
use getmasters_core::permission::{AutoApprover, GrantSet};
use getmasters_core::provider::resolve_provider;
use getmasters_core::store::Store;
use getmasters_proto::{MasterRunResult, MessageDto};

use crate::acp::{run_acp_master, AcpRunContext};
use crate::state::AppState;

/// The project's master store (files under the project data dir + the DB index).
pub fn master_store(state: &AppState, project_id: &str) -> MasterStore {
    MasterStore::new(
        state.project_dir(project_id),
        project_id.to_string(),
        state.agent.store().clone(),
    )
}

/// Load a master by slug from the project store, falling back to the standalone (global) store.
/// This lets a global master (managed from the Masters sidebar, with no project of its own) run
/// through every existing dispatch path — single run, team, group chat, quick chat — unchanged.
pub fn load_master_any(
    state: &AppState,
    project_id: &str,
    slug: &str,
) -> Result<Option<Master>, String> {
    if let Some(master) = master_store(state, project_id)
        .load(slug)
        .map_err(|e| e.to_string())?
    {
        return Ok(Some(master));
    }
    state
        .global_master_store()
        .load(slug)
        .map_err(|e| e.to_string())
}

/// Backend-agnostic dispatch: run a turn for `master` in `session_id`, attributed to `author`,
/// returning its `AgentEvent` stream. `brief = Some` seeds a user turn; `None` answers the session's
/// existing transcript (group chat). `participants` is the group-chat roster `(slug, name)`
/// (empty outside group chat) — injected so the master knows its teammates and can hand off with
/// `@slug` mentions (Phase 4f). Internal masters go through [`AgentService`]; ACP masters go
/// through the [`crate::acp`] driver — callers consume the boxed stream identically.
pub async fn run_master_stream(
    state: &AppState,
    project_id: &str,
    session_id: &str,
    author: &str,
    master: &Master,
    brief: Option<&str>,
    participants: &[(String, String)],
) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>, String> {
    if master.is_acp() {
        let launch = master
            .acp
            .clone()
            .ok_or_else(|| "ACP master missing launch config".to_string())?;
        let store = state.agent.store().clone();
        let grants = std::sync::Arc::new(GrantSet::new(
            store
                .list_folder_grants(Some(project_id))
                .map_err(|e| e.to_string())?,
        ));
        // The ACP agent needs an explicit prompt: the brief, else the last user message posted.
        let mut prompt = match brief {
            Some(b) => b.to_string(),
            None => last_user_text(&store, session_id),
        };
        // Group chat: the harness has no system-prompt seam, so the roster rides in the prompt.
        if !participants.is_empty() {
            let list = participants
                .iter()
                .map(|(slug, name)| format!("- @{slug} ({name})"))
                .collect::<Vec<_>>()
                .join("\n");
            prompt = format!(
                "{prompt}\n\nYou are @{author} in a group chat. Teammates you can hand work to \
                 by mentioning @<slug> in your reply:\n{list}"
            );
        }
        let ctx = AcpRunContext {
            cwd: acp_cwd(state, project_id, &grants),
            store,
            grants,
            // Grant-bounded auto-approval (the grant boundary is the security line); interactive
            // approval for ACP single-runs is a deferred refinement.
            approver: std::sync::Arc::new(AutoApprover),
            session_id: session_id.to_string(),
            author: author.to_string(),
            launch,
            brief: prompt,
        };
        return Ok(Box::pin(run_acp_master(ctx)));
    }

    // Internal backend: build the persona/model/tools agent and run it (headless, grant-bounded).
    let qualified = if master.default_model.trim().is_empty() {
        state.cfg.model.clone()
    } else {
        master.default_model.clone()
    };
    let (provider, model) = resolve_provider(&state.cfg, &qualified);
    let mut agent = state
        .project_agent(project_id)
        .await?
        .without_approval()
        .with_model_provider(provider, model)
        .with_persona(master.persona_block())
        .with_author(author);
    if !participants.is_empty() {
        agent = agent.with_participants(participants.to_vec());
    }
    if !master.allowed_tools.is_empty() {
        agent = agent.with_enabled_tools(master.allowed_tools.iter().cloned().collect());
    }
    match brief {
        Some(b) => Ok(Box::pin(agent.run_turn(session_id, b).await)),
        None => Ok(Box::pin(agent.run_answer_turn(session_id))),
    }
}

/// The working directory handed to an ACP agent: the project's first granted folder (absolute paths
/// per ACP), falling back to the per-project data dir when no folder is granted yet.
fn acp_cwd(state: &AppState, project_id: &str, grants: &GrantSet) -> PathBuf {
    grants
        .grants()
        .first()
        .map(|g| PathBuf::from(&g.path))
        .unwrap_or_else(|| state.project_dir(project_id))
}

/// The most recent user message text in a session (the ACP prompt for a group answer turn).
fn last_user_text(store: &Store, session_id: &str) -> String {
    store
        .list_messages(session_id)
        .ok()
        .and_then(|msgs| {
            msgs.into_iter()
                .rev()
                .find(|m| m.role == "user")
                .map(|m| m.content)
        })
        .unwrap_or_default()
}

/// Drain a master's event stream to its final attributed reply (the `complete_turn` equivalent).
async fn drain_to_message(
    store: &Store,
    mut stream: Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
) -> Result<MessageDto, String> {
    let mut message: Option<MessageDto> = None;
    while let Some(ev) = stream.next().await {
        match ev {
            AgentEvent::Complete { message_id } => message = store.get_message(&message_id).ok(),
            AgentEvent::Error(e) => return Err(e),
            _ => {}
        }
    }
    message.ok_or_else(|| "master produced no reply".to_string())
}

/// Run a brief through one master. Loads the master, opens an `master:<slug>` session, drives the
/// chosen backend to completion, and returns the final attributed message.
pub async fn run(
    state: &AppState,
    project_id: &str,
    slug: &str,
    brief: &str,
) -> Result<MasterRunResult, String> {
    let master = load_master_any(state, project_id, slug)?
        .ok_or_else(|| format!("master '{slug}' not found"))?;

    let store = state.agent.store().clone();
    let session = store
        .create_session(Some(project_id), Some(&format!("master:{slug}")))
        .map_err(|e| e.to_string())?;

    let stream = run_master_stream(
        state,
        project_id,
        &session.id,
        slug,
        &master,
        Some(brief),
        &[],
    )
    .await?;
    let message = drain_to_message(&store, stream).await?;
    Ok(MasterRunResult {
        session_id: session.id,
        message,
    })
}
