//! Shared permission/grant leaf types.
//!
//! These live in `getmasters-proto` (no heavy deps) so both `getmasters-core` (the Permission &
//! Audit engine) and `getmasters-mcp` (the Files server's path backstop) can depend on them
//! without a `core ↔ mcp` cycle. They are plain data; the fs-aware `GrantSet` resolution
//! logic lives in `getmasters-core::permission::grant`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The side-effect class of a tool call — the axis the permission policy keys on (docs/06 §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SideEffect {
    /// Reads within a granted folder (auto-allowed by default).
    Read,
    /// Creates/edits/moves a file (prompted by default).
    Write,
    /// Deletes/overwrites (prompted; soft-delete where possible).
    Destructive,
    /// Reaches the network (prompted).
    Network,
    /// External side-effect like email/post (prompted, never silent).
    Send,
}

impl SideEffect {
    /// Lower-case wire label (`"read"`, `"write"`, …).
    pub fn as_str(&self) -> &'static str {
        match self {
            SideEffect::Read => "read",
            SideEffect::Write => "write",
            SideEffect::Destructive => "destructive",
            SideEffect::Network => "network",
            SideEffect::Send => "send",
        }
    }
}

/// Access level a folder grant confers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FolderAccess {
    Read,
    ReadWrite,
}

impl FolderAccess {
    pub fn as_str(&self) -> &'static str {
        match self {
            FolderAccess::Read => "read",
            FolderAccess::ReadWrite => "read_write",
        }
    }

    /// Parse the stored string form; unknown values fall back to the safe `Read`.
    pub fn from_str_lenient(s: &str) -> Self {
        match s {
            "read_write" => FolderAccess::ReadWrite,
            _ => FolderAccess::Read,
        }
    }

    /// Whether this grant permits writes.
    pub fn allows_write(&self) -> bool {
        matches!(self, FolderAccess::ReadWrite)
    }
}

/// A folder the agent is permitted to act within (permission scope; docs/06).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct FolderGrant {
    pub id: String,
    /// Owning project; `None` for an ad-hoc/global grant.
    pub project_id: Option<String>,
    pub path: String,
    pub access: FolderAccess,
    /// Epoch milliseconds.
    pub created_at: i64,
}
