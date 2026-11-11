//! Errors for the pure domain layer.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid document: {0}")]
    InvalidDocument(String),
    #[error("unsupported schema version {found} (this build supports {supported})")]
    SchemaVersion { found: String, supported: String },
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid time value: {0}")]
    InvalidTime(String),
}

pub type Result<T, E = CoreError> = std::result::Result<T, E>;
