//! `acp-echo` — a minimal stdio **ACP agent** used as the headless test fixture for external ACP
//! master agents (Phase 4i, ADR-0014), mirroring the `mcp-echo` connector fixture.
//!
//! It implements just enough of the agent side of ACP to exercise Masters's ACP *client* driver:
//! - `initialize` / `session/new` handshake,
//! - on `session/prompt`: stream one `agent_message_chunk` that echoes the prompt text **and** the
//!   `GREETING` env var (so the test asserts configured env reaches the child), optionally issue one
//!   `fs/write_text_file` request to the path in `ACP_ECHO_WRITE` (so the test exercises the gate),
//!   then return `end_turn`.

use agent_client_protocol::schema::v1::{
    AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
    NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, SessionId,
    SessionNotification, SessionUpdate, StopReason, TextContent, ToolCall, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, WriteTextFileRequest,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Dispatch, Result, Stdio};

#[tokio::main]
async fn main() -> Result<()> {
    Agent
        .builder()
        .name("acp-echo")
        .on_receive_request(
            async move |initialize: InitializeRequest, responder, _connection| {
                responder.respond(
                    InitializeResponse::new(initialize.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_new_session: NewSessionRequest, responder, _connection| {
                responder.respond(NewSessionResponse::new(SessionId::new("acp-echo-session")))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |prompt: PromptRequest, responder, connection: ConnectionTo<Client>| {
                let session_id = prompt.session_id.clone();

                // Echo the prompt text + the configured GREETING env var (proves env passthrough).
                let prompt_text: String = prompt
                    .prompt
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let greeting = std::env::var("GREETING").unwrap_or_default();
                let reply = format!("echo: {prompt_text} GREETING={greeting}");

                // Surface one tool call + its completion (exercises the client's Phase 4g
                // tool-visibility mapping for ACP masters).
                connection.send_notification(SessionNotification::new(
                    session_id.clone(),
                    SessionUpdate::ToolCall(ToolCall::new("fix-1", "probe the workspace")),
                ))?;
                connection.send_notification(SessionNotification::new(
                    session_id.clone(),
                    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                        "fix-1",
                        ToolCallUpdateFields::new()
                            .status(ToolCallStatus::Completed)
                            .title("probe done".to_string()),
                    )),
                ))?;

                connection.send_notification(SessionNotification::new(
                    session_id.clone(),
                    SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                        TextContent::new(reply),
                    ))),
                ))?;

                // Optionally exercise the client's permission gate with a real file write.
                if let Ok(path) = std::env::var("ACP_ECHO_WRITE") {
                    let _ = connection
                        .send_request(WriteTextFileRequest::new(
                            session_id.clone(),
                            std::path::PathBuf::from(path),
                            "written by acp-echo".to_string(),
                        ))
                        .on_receiving_result(async move |_result| Ok(()));
                }

                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_dispatch(
            async move |message: Dispatch, cx: ConnectionTo<Client>| {
                message.respond_with_error(
                    agent_client_protocol::util::internal_error("unhandled message"),
                    cx,
                )
            },
            agent_client_protocol::on_receive_dispatch!(),
        )
        .connect_to(Stdio::new())
        .await
}
