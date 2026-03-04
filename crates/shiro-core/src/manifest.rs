//! Document lifecycle state machine and run manifests.
//!
//! State transitions per `docs/ARCHITECTURE.md`:
//!
//! ```text
//! STAGED → INDEXING → READY
//!              ↓
//!           FAILED (requires repair/retry)
//!
//! Any → DELETED (tombstone)
//! ```

use serde::{Deserialize, Serialize};

use crate::id::{DocId, RunId};

/// Document lifecycle state per `docs/ARCHITECTURE.md`.
///
/// Documents are searchable **only** when `Ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocState {
    /// Queued for processing.
    Staged,
    /// Parse + index in progress.
    Indexing,
    /// Fully committed and searchable.
    Ready,
    /// Processing failed (requires repair/retry).
    Failed,
    /// Tombstoned (logically removed).
    Deleted,
}

impl DocState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "STAGED",
            Self::Indexing => "INDEXING",
            Self::Ready => "READY",
            Self::Failed => "FAILED",
            Self::Deleted => "DELETED",
        }
    }
}

impl std::fmt::Display for DocState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Processing run lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunState {
    Pending,
    Running,
    Completed,
    Failed,
}

/// Tracks the state of a single document within a processing run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocEntry {
    pub doc_id: DocId,
    pub state: DocState,
    pub error: Option<String>,
}

/// Manifest for a single processing run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub run_id: RunId,
    pub state: RunState,
    pub docs: Vec<DocEntry>,
}
