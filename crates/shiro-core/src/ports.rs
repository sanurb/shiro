//! Port traits for adapter boundaries.
//!
//! Per `docs/ARCHITECTURE.md`: "The core does not 'know' about specific
//! parsers or model providers. Everything external is behind traits."
//!
//! Storage (SQLite) and FTS (Tantivy) are internal infrastructure and use
//! concrete types — only truly external/pluggable adapters get traits.

use crate::error::ShiroError;
use crate::ir::Document;

use serde::{Deserialize, Serialize};

/// Parse raw content into a structured [`Document`].
///
/// Implementations: plain-text, markdown, PDF baseline, premium (subprocess).
pub trait Parser {
    /// Human-readable name for logging/fingerprinting.
    fn name(&self) -> &str;

    /// Monotonic version of the parser implementation.
    ///
    /// Must be incremented whenever the parser's output-affecting behavior
    /// changes (ADR-004). Used to build [`ProcessingFingerprint`] for
    /// staleness detection.
    fn version(&self) -> u32;

    /// Parse raw bytes into a Document.
    ///
    /// `source_uri` is the original path or URL (for metadata).
    fn parse(&self, source_uri: &str, content: &[u8]) -> Result<Document, ShiroError>;
}

/// Generate vector embeddings from text.
///
/// Implementations must be deterministic: identical input text must produce
/// identical output vectors. This is required for reproducible retrieval.
pub trait Embedder: Send + Sync {
    /// Embed a single text string.
    fn embed(&self, text: &str) -> Result<Vec<f32>, ShiroError>;

    /// Embed a batch of texts. Default implementation calls `embed` per item.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, ShiroError> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    /// Expected output dimensions.
    fn dimensions(&self) -> usize;

    /// Metadata about this embedding model.
    fn meta(&self) -> EmbeddingMeta;
}

/// A single hit from a vector similarity search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorHit {
    pub segment_id: crate::id::SegmentId,
    pub score: f32,
}

/// Metadata about an embedding model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingMeta {
    pub dimensions: usize,
    pub model_name: String,
}

/// Store and query vector embeddings for semantic retrieval.
///
/// Implementations must be:
/// - Deterministic: same embeddings → same search results (modulo ANN approximation)
/// - Idempotent: upsert with same ID replaces previous embedding
/// - Thread-safe: `&self` methods must be safe to call concurrently
pub trait VectorIndex: Send + Sync {
    /// Insert or replace an embedding for a segment.
    fn upsert(&self, id: &crate::id::SegmentId, embedding: &[f32]) -> Result<(), ShiroError>;

    /// Remove an embedding by segment ID.
    fn delete(&self, id: &crate::id::SegmentId) -> Result<(), ShiroError>;

    /// Remove all embeddings for a given document.
    fn delete_by_doc(&self, doc_id: &crate::id::DocId) -> Result<(), ShiroError>;

    /// Approximate nearest-neighbor search.
    /// Returns [`VectorHit`] results sorted by descending similarity.
    fn search(&self, query: &[f32], limit: usize) -> Result<Vec<VectorHit>, ShiroError>;

    /// Number of indexed embeddings.
    fn count(&self) -> Result<usize, ShiroError>;

    /// Expected embedding dimensions. Used for validation.
    fn dimensions(&self) -> usize;

    /// Persist any buffered writes to durable storage.
    fn flush(&self) -> Result<(), ShiroError>;
}
