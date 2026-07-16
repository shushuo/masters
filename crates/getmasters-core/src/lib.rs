//! Masters agent core (Phase 0).
//!
//! This crate is the reusable heart of Masters: the [`provider`] abstraction over LLM
//! backends, the SQLite [`store`], and the [`agent`] loop that drives a turn. It is
//! consumed both by the `getmastersd` daemon (`getmasters-server`) and the `getmasters` CLI, which is
//! the concrete realization of ADR-0001's "one shared agent core across desktop + CLI".
//!
//! It must compile and test with **no network and no Tauri**.

pub mod agent;
pub mod assets;
pub mod config;
pub mod error;
pub mod extensions;
pub mod fincalc;
pub mod knowledge;
pub mod market;
pub mod masters;
pub mod memory;
pub mod permission;
pub mod prompt;
pub mod provider;
pub mod revision;
pub mod secrets;
pub mod skills;
pub mod store;
pub mod study;

pub use error::CoreError;
