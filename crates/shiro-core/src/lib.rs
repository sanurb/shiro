//! `shiro-core` — domain types, errors, and port traits for the shiro
//! document knowledge engine.
//!
//! This crate defines the core domain: every other crate in the workspace
//! depends on it, but it depends on no adapter.

pub mod config;
pub mod enrichment;
pub mod error;
pub mod fingerprint;
pub mod generation;
pub mod id;
pub mod ir;
pub mod lock;
pub mod manifest;
pub mod ports;
pub mod span;
pub mod taxonomy;

// Convenience re-exports.
pub use config::{ShiroConfig, ShiroHome};
pub use enrichment::EnrichmentResult;
pub use error::{ErrorCode, ShiroError};
pub use fingerprint::{EmbeddingFingerprint, ProcessingFingerprint};
pub use generation::{GenerationId, IndexGeneration};
pub use id::{DocId, RunId, SegmentId, VersionId};
pub use ir::{Block, BlockGraph, BlockIdx, BlockKind, Document, Edge, Metadata, Relation, Segment};
pub use lock::WriteLock;
pub use manifest::{DocEntry, DocState, RunManifest, RunState};
pub use ports::{EmbeddingMeta, RerankResult, Reranker, VectorHit};
pub use span::Span;
pub use taxonomy::{Concept, ConceptId, ConceptRelation, SkosRelation};
