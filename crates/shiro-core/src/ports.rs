//! Port traits (hexagonal architecture boundaries).
//!
//! These are the interfaces adapters must implement. `shiro-core` defines
//! the shapes; `shiro-parse`, `shiro-store`, `shiro-index` provide impls.
//!
//! All trait methods are synchronous and infallible-or-`ShiroError`.
//! Async adapters should be introduced behind a feature gate when needed.

use camino::Utf8Path;

use crate::error::ShiroError;
use crate::id::{DocId, RunId, SegmentId};
use crate::ir::{Document, Segment};
use crate::manifest::RunManifest;

// ---------------------------------------------------------------------------
// Parsing + normalization
// ---------------------------------------------------------------------------

/// Parse raw content at a given path into a structured [`Document`].
pub trait Parser {
    fn parse(&self, path: &Utf8Path, content: &[u8]) -> Result<Document, ShiroError>;
}

/// Normalize text for display, comparison, and indexing.
///
/// Operates on a single text fragment (block or segment content).
pub trait Normalizer {
    fn normalize(&self, text: &str) -> Result<String, ShiroError>;
}

/// Split a [`Document`] into indexable [`Segment`]s.
///
/// Segmentation is **structure-first**: it follows block boundaries,
/// not token-count heuristics.
pub trait Segmenter {
    fn segment(&self, doc: &Document) -> Result<Vec<Segment>, ShiroError>;
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Persist and retrieve parsed documents.
pub trait DocumentStore {
    fn put(&self, doc: &Document) -> Result<(), ShiroError>;
    fn get(&self, id: &DocId) -> Result<Document, ShiroError>;
    fn list(&self) -> Result<Vec<DocId>, ShiroError>;
}

/// Persist and retrieve run manifests (staging/promote lifecycle).
///
/// TODO: implement SQLite-backed ManifestStore.
/// Acceptance: manifests survive process crash; incomplete staging
/// dirs are cleaned on next start.
pub trait ManifestStore {
    fn save(&self, manifest: &RunManifest) -> Result<(), ShiroError>;
    fn load(&self, run_id: &RunId) -> Result<RunManifest, ShiroError>;
    fn list_runs(&self) -> Result<Vec<RunId>, ShiroError>;
}

// ---------------------------------------------------------------------------
// Indexing + retrieval
// ---------------------------------------------------------------------------

/// Full-text search index (BM25 or similar).
///
/// TODO: implement tantivy or SQLite FTS5 backend.
/// Acceptance: ranked results with term highlighting metadata.
pub trait FtsIndex {
    fn index(&self, segments: &[Segment]) -> Result<(), ShiroError>;
    fn search(&self, query: &str, limit: usize) -> Result<Vec<Segment>, ShiroError>;
}

/// Generate vector embeddings from text.
///
/// TODO: implement with a local model (e.g. ONNX runtime).
/// Acceptance: deterministic output for identical input; dimensions() is stable.
pub trait Embedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, ShiroError>;
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, ShiroError>;
    fn dimensions(&self) -> usize;
}

/// Store and query vector embeddings for semantic retrieval.
///
/// TODO: implement with an in-process vector index (usearch, hnsw, etc.).
/// Acceptance: approximate nearest-neighbor search with cosine similarity.
pub trait VectorStore {
    fn upsert(&self, id: &SegmentId, embedding: &[f32]) -> Result<(), ShiroError>;
    fn search(&self, query: &[f32], limit: usize) -> Result<Vec<(SegmentId, f32)>, ShiroError>;
}
