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
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ReadTextFileRequest,
    ReadTextFileResponse, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    TextContent, WriteTextFileRequest, WriteTextFileResponse,
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

/// Run a brief through an external ACP master, yielding the same [`AgentEvent`] stream the internal
/// master path produces. The whole ACP connection runs on a spawned task feeding an mpsc channel.
pub fn run_acp_master(ctx: AcpRunContext) -> impl Stream<Item = AgentEvent> + Send + 'static {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
    tokio::spawn(async move {
        if let Err(e) = drive(ctx, tx.clone()).await {
            let _ = tx.send(AgentEvent::Error(e));
        }
    });
    futures::stream::poll_fn(move |cx| rx.poll_recv(cx))
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

    let write_gate = gate.clone();
    let read_gate = gate.clone();
    let perm_gate = gate.clone();

    agent_client_protocol::Client
        .builder()
        .name("getmasters")
        // Streaming: forward the agent's text chunks as Deltas (and accumulate for persistence).
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                if let SessionUpdate::AgentMessageChunk(chunk) = notification.update {
                    if let ContentBlock::Text(TextContent { text, .. }) = chunk.content {
                        notif_accum.lock().unwrap().push_str(&text);
                        let _ = notif_tx.send(AgentEvent::Delta(text));
                    }
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
        // Gated callback: the agent asks the user to approve one of its tool calls.
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _conn| {
                let gate = perm_gate.clone();
                let args = serde_json::to_value(&request.tool_call).unwrap_or_else(|_| json!({}));
                match gate.authorize("acp.request_permission", &args).await {
                    // Affirmative: select the first offered option.
                    Authorized::Allowed => {
                        match request.options.first().map(|o| o.option_id.clone()) {
                            Some(id) => responder.respond(RequestPermissionResponse::new(
                                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                    id,
                                )),
                            )),
                            None => responder.respond(RequestPermissionResponse::new(
                                RequestPermissionOutcome::Cancelled,
                            )),
                        }
                    }
                    Authorized::Denied(_) => responder.respond(RequestPermissionResponse::new(
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
