//! `shiro-core` — domain types, errors, and port traits for the shiro
//! document knowledge engine.
//!
//! This crate defines the hexagonal core: every other crate in the workspace
//! depends on it, but it depends on no adapter.

pub mod error;
pub mod id;
pub mod ir;
pub mod manifest;
pub mod ports;
pub mod span;

// Convenience re-exports for the most-used types.
pub use error::{ErrorCode, ShiroError};
pub use id::{DocId, RunId, SegmentId};
pub use ir::{Block, BlockGraph, BlockIdx, BlockKind, Document, Edge, Metadata, Relation, Segment};
pub use manifest::{DocEntry, DocState, RunManifest, RunState};
pub use span::Span;
