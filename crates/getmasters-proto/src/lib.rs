//! Shared wire types for Masters.
//!
//! These DTOs are the **single source of truth** for the client⇄daemon contract
//! (docs/02-architecture.md §4): the daemon's OpenAPI description is derived from them
//! (via `utoipa::ToSchema`), and the desktop's TypeScript client is generated from that
//! OpenAPI in turn. Keep this crate free of runtime deps (no axum/tokio) so both the
//! server and any client tooling can depend on it cheaply.

mod dto;
mod perm;
mod ws;

pub use dto::*;
pub use perm::*;
pub use ws::*;
