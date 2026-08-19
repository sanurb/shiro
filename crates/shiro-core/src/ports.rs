//! Port traits for adapter boundaries.
//!
//! Per `docs/ARCHITECTURE.md`: "The core does not 'know' about specific
//! parsers or model providers. Everything external is behind traits."
//!
//! Storage (SQLite) and FTS (Tantivy) are internal infrastructure and use
//! concrete types — only truly external/pluggable adapters get traits.

use crate::error::ShiroError;
use crate::fingerprint::EmbeddingFingerprint;
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
///
/// Per ADR-012, every implementation MUST expose an [`EmbeddingFingerprint`]
/// that uniquely identifies the embedding configuration. A fingerprint
/// mismatch between the active embedder and a stored index is a hard error.
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

    /// Return the embedding fingerprint for this configuration (ADR-012).
    ///
    /// The fingerprint uniquely identifies the provider + model + dimensions +
    /// normalization + truncation + chunk policy. A mismatch against a stored
    /// index fingerprint is a hard error — the index must be rebuilt.
    fn fingerprint(&self) -> EmbeddingFingerprint;
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
    /// Embedding provider (e.g. `"http"`, `"fastembed"`).
    pub provider: String,
    pub model_name: String,
    pub dimensions: usize,
}

/// Store and query vector embeddings for semantic retrieval.
///
/// Implementations must be:
/// - Deterministic: same embeddings → same search results (modulo ANN approximation)
/// - Idempotent: upsert with same ID replaces previous embedding
/// - Thread-safe: `&self` methods must be safe to call concurrently
pub trait VectorIndex: Send + Sync {
    /// Return the immutable generation represented by this handle.
    ///
    /// Generation zero is the legacy/default generation for adapters that do
    /// not yet persist generation metadata.
    fn generation_id(&self) -> u64 {
        0
    }

    /// Return the embedding fingerprint that defines this index's vector space.
    ///
    /// Vector-capable reads and writes must reject missing or incompatible
    /// fingerprints before using the index (ADR-012).
    fn embedding_fingerprint(&self) -> Result<Option<EmbeddingFingerprint>, ShiroError> {
        Ok(None)
    }

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

const DEFAULT_RERANK_CANDIDATE_COUNT: usize = 50;
const MAX_RERANK_CANDIDATE_COUNT: usize = 200;

/// Validated number of fused candidates supplied to a reranker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RerankCandidateLimit(usize);

impl RerankCandidateLimit {
    /// Validate a rerank candidate limit against the local resource bound.
    pub fn new(candidate_count: usize) -> Result<Self, ShiroError> {
        if !(1..=MAX_RERANK_CANDIDATE_COUNT).contains(&candidate_count) {
            return Err(ShiroError::InvalidInput {
                message: format!(
                    "rerank candidate limit must be between 1 and {MAX_RERANK_CANDIDATE_COUNT}, got {candidate_count}"
                ),
            });
        }
        Ok(Self(candidate_count))
    }

    /// Return the validated number of candidates to rerank.
    pub fn candidate_count(self) -> usize {
        self.0
    }
}

impl Default for RerankCandidateLimit {
    fn default() -> Self {
        Self(DEFAULT_RERANK_CANDIDATE_COUNT)
    }
}

/// Result from a reranker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankResult {
    /// Index of the document in the input list.
    pub index: usize,
    /// Relevance score from the reranker (higher = more relevant).
    pub score: f32,
}

/// Rerank a set of documents against a query.
///
/// Implementations MUST be deterministic: identical inputs produce identical
/// output ordering and scores.
pub trait Reranker: Send + Sync {
    /// Maximum fused candidate count this reranker is configured to score.
    fn rerank_candidate_limit(&self) -> RerankCandidateLimit {
        RerankCandidateLimit::default()
    }

    /// Rerank documents against a query, returning top_n results.
    fn rerank(
        &self,
        query: &str,
        documents: &[&str],
        top_n: usize,
    ) -> Result<Vec<RerankResult>, ShiroError>;

    /// Human-readable model name for logging/explain output.
    fn model_name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rerank_candidate_limit_enforces_resource_bounds() {
        assert_eq!(RerankCandidateLimit::new(1).unwrap().candidate_count(), 1);
        assert_eq!(
            RerankCandidateLimit::new(MAX_RERANK_CANDIDATE_COUNT)
                .unwrap()
                .candidate_count(),
            MAX_RERANK_CANDIDATE_COUNT
        );
        assert!(matches!(
            RerankCandidateLimit::new(0),
            Err(ShiroError::InvalidInput { .. })
        ));
        assert!(matches!(
            RerankCandidateLimit::new(MAX_RERANK_CANDIDATE_COUNT + 1),
            Err(ShiroError::InvalidInput { .. })
        ));
    }

    #[test]
    fn rerank_candidate_limit_defaults_to_fifty() {
        assert_eq!(RerankCandidateLimit::default().candidate_count(), 50);
    }
}
