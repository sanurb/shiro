//! Port traits for adapter boundaries.
//!
//! Per `docs/ARCHITECTURE.md`: "The core does not 'know' about specific
//! parsers or model providers. Everything external is behind traits."
//!
//! Storage (SQLite) and FTS (Tantivy) are internal infrastructure and use
//! concrete types — only truly external/pluggable adapters get traits.

use crate::error::ShiroError;
use crate::ir::Document;

/// Parse raw content into a structured [`Document`].
///
/// Implementations: plain-text, markdown, PDF baseline, premium (subprocess).
pub trait Parser {
    /// Human-readable name for logging/fingerprinting.
    fn name(&self) -> &str;

    /// Parse raw bytes into a Document.
    ///
    /// `source_uri` is the original path or URL (for metadata).
    fn parse(&self, source_uri: &str, content: &[u8]) -> Result<Document, ShiroError>;
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
/// TODO: implement with an in-process vector index.
/// Acceptance: approximate nearest-neighbor search with cosine similarity.
pub trait VectorIndex {
    fn upsert(&self, id: &crate::id::SegmentId, embedding: &[f32]) -> Result<(), ShiroError>;
    fn search(
        &self,
        query: &[f32],
        limit: usize,
    ) -> Result<Vec<(crate::id::SegmentId, f32)>, ShiroError>;
}
