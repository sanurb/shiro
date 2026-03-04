//! Typed error hierarchy for the shiro workspace.
//!
//! Every adapter maps its internal failures into these variants.
//! The CLI maps each variant to a stable error code string and deterministic
//! exit code per `docs/CLI.md`.

use std::fmt;

use crate::id::DocId;

/// Top-level error type for all shiro operations.
///
/// Each variant maps 1:1 to a stable [`ErrorCode`] — adding a variant
/// without extending [`ErrorCode::from_error`] is a compile error.
#[derive(Debug, thiserror::Error)]
pub enum ShiroError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("PDF parse error: {message}")]
    ParsePdf { message: String },

    #[error("Markdown parse error: {message}")]
    ParseMd { message: String },

    #[error("invalid IR: {message}")]
    InvalidIr { message: String },

    #[error("store corruption: {message}")]
    StoreCorrupt { message: String },

    #[error("FTS index build failed: {message}")]
    IndexBuildFts { message: String },

    #[error("vector index build failed: {message}")]
    IndexBuildVec { message: String },

    #[error("embedding failed: {message}")]
    EmbedFail { message: String },

    #[error("enrichment failed: {message}")]
    EnrichFail { message: String },

    #[error("taxonomy cycle detected: {message}")]
    TaxonomyCycle { message: String },

    #[error("lock busy: {message}")]
    LockBusy { message: String },

    // --- Extension codes (not in CLI.md stable list but needed internally) ---
    #[error("not found: {0}")]
    NotFound(DocId),

    #[error("not found: {message}")]
    NotFoundMsg { message: String },

    #[error("invalid input: {message}")]
    InvalidInput { message: String },

    #[error("config error: {message}")]
    Config { message: String },

    #[error("search failed: {message}")]
    SearchFailed { message: String },
}

/// Stable, machine-readable error codes for the JSON envelope.
///
/// The `E_*` codes match `docs/CLI.md` exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // Stable codes per CLI.md
    EParsePdf,
    EParseMd,
    EInvalidIr,
    EStoreCorrupt,
    EIndexBuildFts,
    EIndexBuildVec,
    EEmbedFail,
    EEnrichFail,
    ETaxonomyCycle,
    ELockBusy,
    // Extension codes
    EIo,
    ENotFound,
    EInvalidInput,
    EConfig,
    ESearchFailed,
}

impl ErrorCode {
    /// Map a domain error to its stable code. Exhaustive by construction.
    pub fn from_error(err: &ShiroError) -> Self {
        match err {
            ShiroError::Io(_) => Self::EIo,
            ShiroError::ParsePdf { .. } => Self::EParsePdf,
            ShiroError::ParseMd { .. } => Self::EParseMd,
            ShiroError::InvalidIr { .. } => Self::EInvalidIr,
            ShiroError::StoreCorrupt { .. } => Self::EStoreCorrupt,
            ShiroError::IndexBuildFts { .. } => Self::EIndexBuildFts,
            ShiroError::IndexBuildVec { .. } => Self::EIndexBuildVec,
            ShiroError::EmbedFail { .. } => Self::EEmbedFail,
            ShiroError::EnrichFail { .. } => Self::EEnrichFail,
            ShiroError::TaxonomyCycle { .. } => Self::ETaxonomyCycle,
            ShiroError::LockBusy { .. } => Self::ELockBusy,
            ShiroError::NotFound(_) | ShiroError::NotFoundMsg { .. } => Self::ENotFound,
            ShiroError::InvalidInput { .. } => Self::EInvalidInput,
            ShiroError::Config { .. } => Self::EConfig,
            ShiroError::SearchFailed { .. } => Self::ESearchFailed,
        }
    }

    /// Stable string representation for JSON envelopes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EParsePdf => "E_PARSE_PDF",
            Self::EParseMd => "E_PARSE_MD",
            Self::EInvalidIr => "E_INVALID_IR",
            Self::EStoreCorrupt => "E_STORE_CORRUPT",
            Self::EIndexBuildFts => "E_INDEX_BUILD_FTS",
            Self::EIndexBuildVec => "E_INDEX_BUILD_VEC",
            Self::EEmbedFail => "E_EMBED_FAIL",
            Self::EEnrichFail => "E_ENRICH_FAIL",
            Self::ETaxonomyCycle => "E_TAXONOMY_CYCLE",
            Self::ELockBusy => "E_LOCK_BUSY",
            Self::EIo => "E_IO",
            Self::ENotFound => "E_NOT_FOUND",
            Self::EInvalidInput => "E_INVALID_INPUT",
            Self::EConfig => "E_CONFIG",
            Self::ESearchFailed => "E_SEARCH_FAILED",
        }
    }

    /// Deterministic process exit code per `docs/CLI.md`.
    ///
    /// | Code | Meaning                     |
    /// |------|-----------------------------|
    /// | 0    | success                     |
    /// | 2    | usage error                 |
    /// | 10   | ingest/parse failure        |
    /// | 11   | index build/activation      |
    /// | 12   | search/query failure        |
    /// | 20   | store corruption detected   |
    /// | 21   | lock busy                   |
    pub fn exit_code(self) -> i32 {
        match self {
            Self::EInvalidInput | Self::EConfig => 2,
            Self::EParsePdf | Self::EParseMd | Self::EInvalidIr | Self::EEmbedFail => 10,
            Self::EIndexBuildFts | Self::EIndexBuildVec => 11,
            Self::ESearchFailed => 12,
            Self::EStoreCorrupt => 20,
            Self::ELockBusy => 21,
            // Unmapped codes default to 1 (generic failure).
            Self::EIo | Self::EEnrichFail | Self::ETaxonomyCycle | Self::ENotFound => 1,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_error_codes_match_docs() {
        assert_eq!(ErrorCode::EParsePdf.as_str(), "E_PARSE_PDF");
        assert_eq!(ErrorCode::EParseMd.as_str(), "E_PARSE_MD");
        assert_eq!(ErrorCode::EInvalidIr.as_str(), "E_INVALID_IR");
        assert_eq!(ErrorCode::EStoreCorrupt.as_str(), "E_STORE_CORRUPT");
        assert_eq!(ErrorCode::EIndexBuildFts.as_str(), "E_INDEX_BUILD_FTS");
        assert_eq!(ErrorCode::EIndexBuildVec.as_str(), "E_INDEX_BUILD_VEC");
        assert_eq!(ErrorCode::EEmbedFail.as_str(), "E_EMBED_FAIL");
        assert_eq!(ErrorCode::EEnrichFail.as_str(), "E_ENRICH_FAIL");
        assert_eq!(ErrorCode::ETaxonomyCycle.as_str(), "E_TAXONOMY_CYCLE");
        assert_eq!(ErrorCode::ELockBusy.as_str(), "E_LOCK_BUSY");
    }

    #[test]
    fn exit_codes_match_docs() {
        assert_eq!(ErrorCode::EInvalidInput.exit_code(), 2);
        assert_eq!(ErrorCode::EParsePdf.exit_code(), 10);
        assert_eq!(ErrorCode::EIndexBuildFts.exit_code(), 11);
        assert_eq!(ErrorCode::ESearchFailed.exit_code(), 12);
        assert_eq!(ErrorCode::EStoreCorrupt.exit_code(), 20);
        assert_eq!(ErrorCode::ELockBusy.exit_code(), 21);
    }

    #[test]
    fn from_error_exhaustive() {
        // Verify every ShiroError variant maps to an ErrorCode.
        let cases: Vec<ShiroError> = vec![
            ShiroError::Io(std::io::Error::other("x")),
            ShiroError::ParsePdf {
                message: String::new(),
            },
            ShiroError::ParseMd {
                message: String::new(),
            },
            ShiroError::InvalidIr {
                message: String::new(),
            },
            ShiroError::StoreCorrupt {
                message: String::new(),
            },
            ShiroError::IndexBuildFts {
                message: String::new(),
            },
            ShiroError::IndexBuildVec {
                message: String::new(),
            },
            ShiroError::EmbedFail {
                message: String::new(),
            },
            ShiroError::EnrichFail {
                message: String::new(),
            },
            ShiroError::TaxonomyCycle {
                message: String::new(),
            },
            ShiroError::LockBusy {
                message: String::new(),
            },
            ShiroError::NotFound(DocId::from_content(b"x")),
            ShiroError::NotFoundMsg {
                message: String::new(),
            },
            ShiroError::InvalidInput {
                message: String::new(),
            },
            ShiroError::Config {
                message: String::new(),
            },
            ShiroError::SearchFailed {
                message: String::new(),
            },
        ];

        for err in &cases {
            let code = ErrorCode::from_error(err);
            // Just verify it doesn't panic and returns a non-empty string.
            assert!(!code.as_str().is_empty());
        }
    }
}
