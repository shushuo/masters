//! **External ACP master agents** (Phase 4i, ADR-0014) — drive a pre-installed, ACP-compatible
//! coding CLI (Claude Code, Codex, OpenCode, Gemini CLI) as a first-class master.
//!
//! Masters plays the ACP **client**: it spawns the harness as a subprocess (stdio JSON-RPC via the
//! `agent-client-protocol` crate), runs `initialize` → `session/new` → `session/prompt`, and maps the
//! agent's streaming `session/update` notifications onto Masters's own [`AgentEvent`] stream — so the
//! existing single-run, team, and group-chat plumbing consume an ACP master exactly like an internal
//! one. The security crux: the harness runs *its own* tool loop, so every file read/write and
//! permission request it makes is an ACP **callback** that we route through Masters's Permission &
//! Audit gate ([`getmasters_core::permission`]) before honoring (ADR-0008).
//!
//! Unlike MCP connectors (narrow tools, fully env-stripped), an ACP coding harness is a user-installed,
//! trusted local agent that needs a real environment (PATH/HOME/`npx`) to run; it therefore inherits
//! the daemon environment plus the master's configured `acp_env`, and the gate governs its side
//! effects. Remote transports, terminals, and the agent's own MCP servers are deferred.

pub mod registry;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::Stream;
use serde_json::json;

use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PermissionOption, PermissionOptionKind,
    PromptRequest, ReadTextFileRequest, ReadTextFileResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionNotification, SessionUpdate, TextContent, ToolCallStatus, ToolKind,
    WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};

use getmasters_core::agent::AgentEvent;
use getmasters_core::masters::AcpLaunch;
use getmasters_core::permission::{Approver, Authorized, GrantSet, PermissionGate};
use getmasters_core::store::Store;

/// Everything the ACP driver needs to run one turn: where to persist, what to spawn, the gate inputs.
pub struct AcpRunContext {
    pub store: Store,
    pub grants: Arc<GrantSet>,
    pub approver: Arc<dyn Approver>,
    /// Session the final reply is persisted into (an `master:<slug>` or group scratch session).
    pub session_id: String,
    /// Author the reply is attributed to (the master slug) — for group-chat transcripts.
    pub author: String,
    /// Working directory for the agent (a granted project folder; absolute per ACP).
    pub cwd: PathBuf,
    pub launch: AcpLaunch,
    /// The user text handed to the agent as its prompt.
    pub brief: String,
}

/// Overall bound on one ACP run (`GETMASTERS_ACP_TIMEOUT_SECS`, default 600s) — a wedged harness
/// becomes a turn error instead of hanging the dispatch forever.
fn acp_timeout() -> std::time::Duration {
    std::env::var("GETMASTERS_ACP_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .map(std::time::Duration::from_secs)
        .unwrap_or(std::time::Duration::from_secs(600))
}

/// Run a brief through an external ACP master, yielding the same [`AgentEvent`] stream the internal
/// master path produces. The whole ACP connection runs on a spawned task feeding an mpsc channel,
/// bounded by [`acp_timeout`] (dropping the connection kills the child process).
pub fn run_acp_master(ctx: AcpRunContext) -> impl Stream<Item = AgentEvent> + Send + 'static {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
    tokio::spawn(async move {
        let timeout = acp_timeout();
        match tokio::time::timeout(timeout, drive(ctx, tx.clone())).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = tx.send(AgentEvent::Error(e));
            }
            Err(_) => {
                let _ = tx.send(AgentEvent::Error(format!(
                    "ACP agent timed out after {}s",
                    timeout.as_secs()
                )));
            }
        }
    });
    futures::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

/// Pick the permission option matching the gate's verdict: allow → prefer *allow-once* (never
/// silently escalate to an "always" grant), deny → prefer *reject-once*. `None` = no matching
/// option offered → answer `Cancelled`.
fn pick_option(options: &[PermissionOption], allow: bool) -> Option<PermissionOption> {
    let preference: &[PermissionOptionKind] = if allow {
        &[
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::AllowAlways,
        ]
    } else {
        &[
            PermissionOptionKind::RejectOnce,
            PermissionOptionKind::RejectAlways,
        ]
    };
    preference
        .iter()
        .find_map(|kind| options.iter().find(|o| o.kind == *kind).cloned())
}

/// Whether an ACP tool kind only reads (grant-wise); everything else needs write access.
fn kind_is_read(kind: ToolKind) -> bool {
    matches!(kind, ToolKind::Read | ToolKind::Search | ToolKind::Think)
}

/// Authorize an ACP `session/request_permission` through the Permission & Audit gate.
///
/// The crux (ADR-0014): a located file operation is checked **per path** against the folder
/// grants (`files.read` for read kinds, `files.create` for edit/delete/move), so an out-of-grant
/// operation is denied + audited even under headless auto-approval. An un-located call (execute/
/// fetch/other) authorizes as `acp.<kind>` — Write-classified by the default policy, so it rides
/// the approver (auto in headless runs) and is always audited.
async fn authorize_permission(gate: &PermissionGate, request: &RequestPermissionRequest) -> bool {
    let fields = &request.tool_call.fields;
    let kind = fields.kind.unwrap_or_default();
    let locations = fields.locations.clone().unwrap_or_default();

    if !locations.is_empty() {
        let tool = if kind_is_read(kind) {
            "files.read"
        } else {
            "files.create"
        };
        for loc in &locations {
            let path = loc.path.to_string_lossy().to_string();
            if let Authorized::Denied(_) = gate.authorize(tool, &json!({ "path": path })).await {
                return false;
            }
        }
        return true;
    }

    let name = format!("acp.{}", format!("{kind:?}").to_ascii_lowercase());
    let args = json!({
        "title": fields.title,
        "raw_input": fields.raw_input,
    });
    matches!(gate.authorize(&name, &args).await, Authorized::Allowed)
}

/// Build the gate the ACP callbacks authorize against — same construction as the internal loop.
fn gate(ctx: &AcpRunContext) -> Arc<PermissionGate> {
    Arc::new(PermissionGate::new(
        ctx.grants.clone(),
        ctx.approver.clone(),
        ctx.store.clone(),
        Some(ctx.session_id.clone()),
    ))
}

/// Build the ACP transport. The configured env is passed as leading `KEY=value` args (the env the
/// agent receives on top of the inherited environment); the command + args follow.
fn build_agent(launch: &AcpLaunch) -> Result<AcpAgent, String> {
    let mut parts: Vec<String> = launch.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
    parts.push(launch.command.clone());
    parts.extend(launch.args.iter().cloned());
    AcpAgent::from_args(parts).map_err(|e| format!("invalid ACP launch config: {e:?}"))
}

/// The driver lifecycle: connect, initialize, open a session, prompt, stream updates, persist.
async fn drive(
    ctx: AcpRunContext,
    tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), String> {
    let transport = build_agent(&ctx.launch)?;
    let gate = gate(&ctx);

    // Accumulates the agent's streamed text so we can persist one assistant message at the end.
    let accum: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    let cwd = ctx.cwd.clone();
    let brief = ctx.brief.clone();
    let store = ctx.store.clone();
    let session_id = ctx.session_id.clone();
    let author = ctx.author.clone();

    let notif_tx = tx.clone();
    let notif_accum = accum.clone();
    let notif_store = ctx.store.clone();
    let notif_session = ctx.session_id.clone();

    let write_gate = gate.clone();
    let read_gate = gate.clone();
    let perm_gate = gate.clone();

    agent_client_protocol::Client
        .builder()
        .name("getmasters")
        // Streaming: forward the agent's text chunks as Deltas (accumulated for persistence),
        // and surface its tool activity as attributed ToolCallStarted/ToolResult events (Phase 4g
        // visibility now covers ACP masters too), mirrored into the durable event log.
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                match notification.update {
                    SessionUpdate::AgentMessageChunk(chunk) => {
                        if let ContentBlock::Text(TextContent { text, .. }) = chunk.content {
                            notif_accum.lock().unwrap().push_str(&text);
                            let _ = notif_tx.send(AgentEvent::Delta(text));
                        }
                    }
                    SessionUpdate::ToolCall(tc) => {
                        let mut summary = tc.title.clone();
                        if let Some(loc) = tc.locations.first() {
                            summary = format!("{summary} {}", loc.path.display());
                        }
                        let name = format!("acp.{}", format!("{:?}", tc.kind).to_ascii_lowercase());
                        let _ = notif_store.append_event(
                            &notif_session,
                            "tool_call",
                            Some(
                                &json!({ "id": tc.tool_call_id.0.as_ref(), "tool": name, "summary": summary })
                                    .to_string(),
                            ),
                        );
                        let _ = notif_tx.send(AgentEvent::ToolCallStarted {
                            id: tc.tool_call_id.0.to_string(),
                            name,
                            summary,
                        });
                    }
                    SessionUpdate::ToolCallUpdate(up) => {
                        if let Some(status) = up.fields.status {
                            if matches!(status, ToolCallStatus::Completed | ToolCallStatus::Failed)
                            {
                                let summary = up.fields.title.clone().unwrap_or_default();
                                let is_error = matches!(status, ToolCallStatus::Failed);
                                let _ = notif_store.append_event(
                                    &notif_session,
                                    "tool_result",
                                    Some(
                                        &json!({ "id": up.tool_call_id.0.as_ref(), "summary": summary, "is_error": is_error })
                                            .to_string(),
                                    ),
                                );
                                let _ = notif_tx.send(AgentEvent::ToolResult {
                                    id: up.tool_call_id.0.to_string(),
                                    summary,
                                    is_error,
                                });
                            }
                        }
                    }
                    _ => {}
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        // Gated callback: the agent wants to write a file.
        .on_receive_request(
            async move |request: WriteTextFileRequest, responder, _conn| {
                let gate = write_gate.clone();
                let path = request.path.to_string_lossy().to_string();
                match gate
                    .authorize("files.create", &json!({ "path": path }))
                    .await
                {
                    Authorized::Allowed => match std::fs::write(&request.path, &request.content) {
                        Ok(()) => responder.respond(WriteTextFileResponse::new()),
                        Err(e) => {
                            responder.respond_with_internal_error(format!("write failed: {e}"))
                        }
                    },
                    Authorized::Denied(reason) => responder
                        .respond_with_internal_error(format!("permission denied: {reason}")),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // Gated callback: the agent wants to read a file.
        .on_receive_request(
            async move |request: ReadTextFileRequest, responder, _conn| {
                let gate = read_gate.clone();
                let path = request.path.to_string_lossy().to_string();
                match gate.authorize("files.read", &json!({ "path": path })).await {
                    Authorized::Allowed => match std::fs::read_to_string(&request.path) {
                        Ok(content) => responder.respond(ReadTextFileResponse::new(content)),
                        Err(e) => {
                            responder.respond_with_internal_error(format!("read failed: {e}"))
                        }
                    },
                    Authorized::Denied(reason) => responder
                        .respond_with_internal_error(format!("permission denied: {reason}")),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // Gated callback: the agent asks the user to approve one of its tool calls. Located file
        // operations are grant-checked per path; the reply picks the option matching the verdict
        // (allow → allow-once, deny → reject-once — never a silent "always" escalation).
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _conn| {
                let gate = perm_gate.clone();
                let allow = authorize_permission(&gate, &request).await;
                match pick_option(&request.options, allow) {
                    Some(option) => responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                            option.option_id,
                        )),
                    )),
                    None => responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    )),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(
            transport,
            move |connection: ConnectionTo<Agent>| async move {
                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;

                let session = connection
                    .send_request(NewSessionRequest::new(cwd))
                    .block_task()
                    .await?;

                connection
                    .send_request(PromptRequest::new(
                        session.session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new(brief))],
                    ))
                    .block_task()
                    .await?;

                // Turn complete: persist the accumulated reply, attributed, and emit Complete.
                let text = accum.lock().unwrap().clone();
                match store.insert_message_attributed(
                    &session_id,
                    &author,
                    "assistant",
                    &text,
                    None,
                ) {
                    Ok(msg) => {
                        let _ = tx.send(AgentEvent::Complete { message_id: msg.id });
                    }
                    Err(e) => {
                        let _ = tx.send(AgentEvent::Error(e.to_string()));
                    }
                }
                Ok(())
            },
        )
        .await
        .map_err(|e| format!("ACP session failed: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        SessionId, ToolCallLocation, ToolCallUpdate, ToolCallUpdateFields,
    };
    use getmasters_core::permission::AutoApprover;
    use getmasters_proto::FolderAccess;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("getmasters-acpperm-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// A gate with one ReadWrite grant on `dir`, auto-approving (the headless posture).
    fn gate_with_grant(dir: &std::path::Path) -> (PermissionGate, Store, String) {
        let store = Store::open_in_memory().unwrap();
        let session = store.create_session(None, None).unwrap();
        let grant = store
            .create_folder_grant(None, dir.to_str().unwrap(), FolderAccess::ReadWrite)
            .unwrap();
        let gate = PermissionGate::new(
            Arc::new(GrantSet::new(vec![grant])),
            Arc::new(AutoApprover),
            store.clone(),
            Some(session.id.clone()),
        );
        (gate, store, session.id)
    }

    fn perm_request(path: &std::path::Path, kind: ToolKind) -> RequestPermissionRequest {
        RequestPermissionRequest::new(
            SessionId::new("s1"),
            ToolCallUpdate::new(
                "t1",
                ToolCallUpdateFields::new()
                    .kind(kind)
                    .title("write a file".to_string())
                    .locations(vec![ToolCallLocation::new(path)]),
            ),
            vec![
                PermissionOption::new("allow-1", "Allow once", PermissionOptionKind::AllowOnce),
                PermissionOption::new(
                    "allow-all",
                    "Always allow",
                    PermissionOptionKind::AllowAlways,
                ),
                PermissionOption::new("reject-1", "Reject once", PermissionOptionKind::RejectOnce),
            ],
        )
    }

    #[tokio::test]
    async fn located_edit_inside_grant_allows_with_allow_once() {
        let dir = temp_dir();
        let (gate, _store, _sid) = gate_with_grant(&dir);
        let req = perm_request(&dir.join("ok.txt"), ToolKind::Edit);
        assert!(authorize_permission(&gate, &req).await);
        // The reply must pick allow-once, never silently escalate to "always".
        let picked = pick_option(&req.options, true).unwrap();
        assert_eq!(picked.option_id.0.as_ref(), "allow-1");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn located_edit_outside_grant_denies_with_reject_once_and_audits() {
        let dir = temp_dir();
        let outside = temp_dir();
        let (gate, store, sid) = gate_with_grant(&dir);
        let req = perm_request(&outside.join("evil.txt"), ToolKind::Edit);
        assert!(!authorize_permission(&gate, &req).await);
        let picked = pick_option(&req.options, false).unwrap();
        assert_eq!(picked.option_id.0.as_ref(), "reject-1");
        // The denial is audited (files.create / denied).
        let audit = store.list_audit(&sid).unwrap();
        assert!(
            audit
                .iter()
                .any(|(tool, decision, _)| tool == "files.create" && decision == "denied"),
            "expected a denied files.create audit row, got {audit:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[tokio::test]
    async fn read_kind_checks_read_access_only() {
        let dir = temp_dir();
        std::fs::write(dir.join("readable.txt"), "x").unwrap();
        let (gate, _store, _sid) = gate_with_grant(&dir);
        let req = perm_request(&dir.join("readable.txt"), ToolKind::Read);
        assert!(authorize_permission(&gate, &req).await);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_matching_option_yields_none() {
        let only_reject = vec![PermissionOption::new(
            "reject-1",
            "Reject",
            PermissionOptionKind::RejectOnce,
        )];
        assert!(pick_option(&only_reject, true).is_none()); // allow verdict, no allow option
    }
}
