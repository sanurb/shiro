//! Typed error hierarchy for the shiro workspace.
//!
//! Every adapter maps its internal failures into these variants.
//! The CLI maps each variant to a stable [`ErrorCode`] and deterministic exit code.

use std::fmt;

use crate::id::DocId;

/// Top-level error type for all shiro operations.
#[derive(Debug, thiserror::Error)]
pub enum ShiroError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {message}")]
    Config { message: String },

    #[error("parse failed: {message}")]
    Parse { message: String },

    #[error("store error: {message}")]
    Store { message: String },

    #[error("index error: {message}")]
    Index { message: String },

    #[error("invalid state: {message}")]
    InvalidState { message: String },

    #[error("document not found: {0}")]
    NotFound(DocId),

    #[error("unsupported: {message}")]
    Unsupported { message: String },

    #[error("invalid input: {message}")]
    InvalidInput { message: String },
}

/// Stable, machine-readable error codes for the JSON envelope.
///
/// Exhaustive over [`ShiroError`] variants — adding a `ShiroError` variant
/// without extending this mapping is a compile error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    IoError,
    ConfigError,
    ParseFailed,
    StoreError,
    IndexError,
    InvalidState,
    NotFound,
    Unsupported,
    InvalidInput,
}

impl ErrorCode {
    /// Map a domain error to its stable code. Exhaustive by construction.
    pub fn from_error(err: &ShiroError) -> Self {
        match err {
            ShiroError::Io(_) => Self::IoError,
            ShiroError::Config { .. } => Self::ConfigError,
            ShiroError::Parse { .. } => Self::ParseFailed,
            ShiroError::Store { .. } => Self::StoreError,
            ShiroError::Index { .. } => Self::IndexError,
            ShiroError::InvalidState { .. } => Self::InvalidState,
            ShiroError::NotFound(_) => Self::NotFound,
            ShiroError::Unsupported { .. } => Self::Unsupported,
            ShiroError::InvalidInput { .. } => Self::InvalidInput,
        }
    }

    /// Deterministic process exit code for this error class.
    ///
    /// | Code | Meaning       |
    /// |------|---------------|
    /// | 0    | success       |
    /// | 2    | usage / input |
    /// | 3    | not found     |
    /// | 4    | invalid state |
    /// | 5    | internal      |
    pub fn exit_code(self) -> i32 {
        match self {
            Self::InvalidInput | Self::ConfigError | Self::Unsupported => 2,
            Self::NotFound => 3,
            Self::InvalidState => 4,
            Self::IoError | Self::ParseFailed | Self::StoreError | Self::IndexError => 5,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::IoError => "IO_ERROR",
            Self::ConfigError => "CONFIG_ERROR",
            Self::ParseFailed => "PARSE_FAILED",
            Self::StoreError => "STORE_ERROR",
            Self::IndexError => "INDEX_ERROR",
            Self::InvalidState => "INVALID_STATE",
            Self::NotFound => "NOT_FOUND",
            Self::Unsupported => "UNSUPPORTED",
            Self::InvalidInput => "INVALID_INPUT",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
