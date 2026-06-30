//! Core error type.

use crate::provider::ProviderError;

/// Errors surfaced by the core library.
#[derive(thiserror::Error, Debug)]
pub enum CoreError {
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
