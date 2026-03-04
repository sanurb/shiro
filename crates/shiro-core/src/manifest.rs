//! Run manifests and document lifecycle state machines.
//!
//! A **run** is an atomic unit of work: ingest N documents, parse, store,
//! index. The manifest tracks which documents are in-flight and their state.
//!
//! ## Directory layout (planned, not yet implemented)
//!
//! ```text
//! <data_dir>/
//!   staging/<run_id>/       # in-progress run artifacts
//!     manifest.json
//!     docs/<doc_id>.json
//!   live/                   # committed, query-ready state
//!     docs/<doc_id>.json
//!     index/
//! ```
//!
//! TODO: implement commit protocol (staging → live promotion).
//! Acceptance criteria:
//! - Atomic promotion: either all docs commit or none do.
//! - Crash recovery: incomplete staging dirs are cleaned up on next start.
//! - Idempotent re-runs: re-ingesting identical content is a no-op.

use serde::{Deserialize, Serialize};

use crate::id::{DocId, RunId};

/// Document lifecycle state within a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocState {
    /// Queued for processing.
    Staged,
    /// Successfully parsed into segments.
    Parsed,
    /// Full-text search index updated.
    IndexedFts,
    /// Vector embeddings indexed.
    IndexedVec,
    /// Fully committed and query-ready.
    Ready,
}

/// Processing run lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunState {
    Pending,
    Running,
    Completed,
    Failed,
}

/// Tracks the state of each document within a processing run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocEntry {
    pub doc_id: DocId,
    pub state: DocState,
    // TODO: last_error: Option<String>, retry_count: u32, segment_count: usize
}

/// Manifest for a single processing run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub run_id: RunId,
    pub state: RunState,
    pub docs: Vec<DocEntry>,
    // TODO: created_at, updated_at timestamps
}
