//! Built-in MCP servers for Masters (ADR-0005).
//!
//! Phase 1a ships the **Files** server ([`files::FilesServer`]), implemented with the official
//! Rust MCP SDK (`rmcp`) and hosted in-process by the Core Extension Manager over a duplex
//! transport. Knowledge/Study/Memory/Skills/Masters/Web follow in later phases. Every tool
//! exposed here is gated by Core's Permission & Audit — adding a tool never bypasses it.

pub mod files;

pub use files::{tool_classes, FilesServer};

/// The set of built-in servers Masters plans to host (Phase 1a implements `files`).
pub const BUILTIN_SERVERS: &[&str] = &[
    "files",
    "knowledge",
    "study",
    "memory",
    "skills",
    "assets",
    "market",
    "masters",
    "web",
];
