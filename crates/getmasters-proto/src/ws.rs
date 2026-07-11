//! WebSocket envelopes for a live streaming agent run (docs/02-architecture.md §4).
//!
//! The client opens `GET /sessions/{id}/ws`, sends a [`ClientCommand`], and receives a
//! sequence of [`ServerEvent`]s. Both are `#[serde(tag = "type")]` so they round-trip as
//! discriminated unions in TypeScript.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A command sent from the client (desktop) to the daemon over the WebSocket.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    /// Submit a user turn; the daemon persists it and starts streaming a reply.
    Send {
        content: String,
        /// Group chat only (Phase 4f): optional per-call cap on mention-driven follow-up rounds.
        /// Clamped into `1..=5`; absent → the server default (`MAX_GROUP_ROUNDS`). Ignored for
        /// ordinary single-turn sessions.
        #[serde(default)]
        max_rounds: Option<u32>,
    },
    /// Resolve a pending approval request (answers a `ServerEvent::ApprovalRequest`).
    ApprovalDecision {
        request_id: String,
        /// One of `"allow"` | `"allow_folder"` | `"always_tool"` | `"deny"`.
        decision: String,
    },
    /// Request cancellation of the in-flight run (cancels between chunks / mid-approval).
    Stop,
}

/// An event streamed from the daemon to the client during a run.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// The assistant turn has begun.
    MessageStart,
    /// A chunk of assistant text.
    TokenDelta { text: String },
    /// The agent is about to invoke a tool (after approval, if any).
    ToolCallStarted {
        id: String,
        tool: String,
        summary: String,
    },
    /// A tool finished; `summary` is a short result description.
    ToolResult {
        id: String,
        summary: String,
        is_error: bool,
    },
    /// The agent needs the user to approve a side-effecting tool call. The client replies
    /// with `ClientCommand::ApprovalDecision` carrying the same `request_id`.
    ApprovalRequest {
        request_id: String,
        tool: String,
        summary: String,
        /// Side-effect classes involved (e.g. `["write"]`).
        classes: Vec<String>,
        /// A before/after preview of a proposed file write (write-class tools only). Optional and
        /// `#[serde(default)]` for backward-compat; display-only — see [`FilePreview`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview: Option<FilePreview>,
    },
    /// The assistant turn finished and was persisted under `message_id`.
    MessageComplete { message_id: String },
    /// A terminal error ended the run.
    Error { message: String },

    // --- Multi-master group chat streaming (Phase 4e/4f, ADR-0012) ---
    /// A group **round** began; `addressed` are the master slugs that will reply (in order). `round`
    /// is 0 for the user's turn, then increments for each mention-driven follow-up round (Phase 4f).
    GroupStart { round: u32, addressed: Vec<String> },
    /// A chunk of one master's reply (attributed to `author`, within `round`).
    MasterDelta {
        round: u32,
        author: String,
        text: String,
    },
    /// One master's reply finished and was posted to the group transcript under `message_id`.
    MasterComplete {
        round: u32,
        author: String,
        message_id: String,
    },
    /// One master failed; the others continue (non-terminal).
    MasterError {
        round: u32,
        author: String,
        message: String,
    },
    /// One master is about to invoke a tool (Phase 4g — attributed tool-call visibility).
    MasterToolCall {
        round: u32,
        author: String,
        id: String,
        tool: String,
        summary: String,
    },
    /// A master's tool finished; `summary` is a short result description.
    MasterToolResult {
        round: u32,
        author: String,
        id: String,
        summary: String,
        is_error: bool,
    },
    /// Every round finished — the group turn is done (terminal).
    GroupComplete,
}

/// A before/after preview of a **proposed** (not-yet-applied) file write, attached to an
/// [`ServerEvent::ApprovalRequest`] so the desktop can render a diff before the user allows it.
///
/// Reconstructed in the permission gate from the grant-checked pre-image plus the tool args
/// (`create` → new content; `edit` → `find`/`replace` applied once; `delete` → removal). It is
/// **display-only**: never persisted, never logged, and its computation can never change the
/// authorization verdict. `omitted` is set (with `before`/`after` = `None`) when the target is
/// binary or over the size cap.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct FilePreview {
    pub path: String,
    /// `"create" | "edit" | "delete"`.
    pub op: String,
    /// Prior on-disk content (`None` when the file didn't exist, or was unreadable/binary/omitted).
    pub before: Option<String>,
    /// Proposed content (`None` for `delete` or when omitted).
    pub after: Option<String>,
    pub added: u32,
    pub removed: u32,
    /// True when the preview was skipped (binary or over the size cap); `before`/`after` are `None`.
    pub omitted: bool,
}
