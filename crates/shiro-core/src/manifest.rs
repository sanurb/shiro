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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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

    /// Check whether transitioning from `self` to `to` is valid.
    ///
    /// Valid transitions per `docs/ARCHITECTURE.md`:
    /// - `STAGED → INDEXING → READY`
    /// - `INDEXING → FAILED`
    /// - `FAILED → STAGED` (retry)
    /// - `any → DELETED` (tombstone)
    pub fn can_transition_to(self, to: DocState) -> bool {
        matches!(
            (self, to),
            (Self::Staged, Self::Indexing)
                | (Self::Indexing, Self::Ready)
                | (Self::Indexing, Self::Failed)
                | (Self::Failed, Self::Staged)
                | (_, Self::Deleted)
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_transitions() {
        // Normal pipeline
        assert!(DocState::Staged.can_transition_to(DocState::Indexing));
        assert!(DocState::Indexing.can_transition_to(DocState::Ready));

        // Failure path
        assert!(DocState::Indexing.can_transition_to(DocState::Failed));

        // Retry from failure
        assert!(DocState::Failed.can_transition_to(DocState::Staged));

        // Tombstone from any state
        assert!(DocState::Staged.can_transition_to(DocState::Deleted));
        assert!(DocState::Indexing.can_transition_to(DocState::Deleted));
        assert!(DocState::Ready.can_transition_to(DocState::Deleted));
        assert!(DocState::Failed.can_transition_to(DocState::Deleted));
        assert!(DocState::Deleted.can_transition_to(DocState::Deleted));
    }

    #[test]
    fn invalid_transitions() {
        // Cannot skip stages
        assert!(!DocState::Staged.can_transition_to(DocState::Ready));
        assert!(!DocState::Staged.can_transition_to(DocState::Failed));

        // Cannot go backwards (except retry)
        assert!(!DocState::Ready.can_transition_to(DocState::Staged));
        assert!(!DocState::Ready.can_transition_to(DocState::Indexing));
        assert!(!DocState::Indexing.can_transition_to(DocState::Staged));

        // Cannot un-delete
        assert!(!DocState::Deleted.can_transition_to(DocState::Staged));
        assert!(!DocState::Deleted.can_transition_to(DocState::Ready));

        // Self-transitions (except Deleted) are invalid
        assert!(!DocState::Staged.can_transition_to(DocState::Staged));
        assert!(!DocState::Ready.can_transition_to(DocState::Ready));
    }
}
