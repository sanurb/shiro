//! Document intermediate representation.
//!
//! The core data model per `docs/ARCHITECTURE.md`:
//!
//! - **Document**: canonical text + metadata + optional block graph.
//! - **BlockGraph**: arena of blocks + edges + deterministic reading order.
//! - **Segment**: flat, indexable text chunk derived from blocks/text.
//!
//! Pipeline: `Parser → Document → Segmenter → Vec<Segment>`.

use serde::{Deserialize, Serialize};

use crate::id::{DocId, SegmentId};
use crate::span::Span;

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

/// Source metadata for a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// Human-readable title (extracted or inferred).
    pub title: Option<String>,
    /// Original source URI (path or URL).
    pub source_uri: String,
    /// Blake3 hash of the raw source bytes.
    pub source_hash: String,
}

/// A parsed document. The `canonical_text` field is the single coordinate
/// space; all [`Span`]s in blocks and segments are byte offsets into it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    pub canonical_text: String,
    pub metadata: Metadata,
    /// Structural representation. `None` for formats that only produce flat
    /// text (e.g. plain-text parser).
    pub blocks: Option<BlockGraph>,
}

// ---------------------------------------------------------------------------
// Segment (flat, indexable)
// ---------------------------------------------------------------------------

/// A contiguous chunk of content within a document, suitable for indexing.
///
/// Segments are **derived** from the document by the segmenter — they are
/// not stored inside `Document`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id: SegmentId,
    pub doc_id: DocId,
    /// Zero-based index of this segment within the document.
    pub index: usize,
    /// Byte span within `canonical_text`.
    pub span: Span,
    /// The textual content of this segment.
    pub body: String,
}

// ---------------------------------------------------------------------------
// Block arena + graph (structural)
// ---------------------------------------------------------------------------

/// Zero-based index into [`BlockGraph::blocks`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockIdx(pub usize);

/// Structural classification of a content block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockKind {
    Paragraph,
    Heading,
    ListItem,
    TableCell,
    Code,
    Caption,
    Footnote,
}

/// A structural block within a document.
///
/// Provenance model per `docs/ARCHITECTURE.md`:
/// - `canonical_text`: source-faithful representation (archival).
/// - `rendered_text`: normalized form for display/indexing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Source-faithful text (archival).
    pub canonical_text: String,
    /// Normalized text for display/indexing. `None` until normalization runs.
    pub rendered_text: Option<String>,
    pub kind: BlockKind,
    /// Byte span within the document's `canonical_text`.
    pub span: Span,
}

/// Semantic relation between two blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Relation {
    /// Block `from` should be read before block `to`.
    ReadsBefore,
    /// Block `from` is a caption of block `to`.
    CaptionOf,
    /// Block `from` is a footnote referenced by block `to`.
    FootnoteOf,
    /// Block `from` references block `to` (citation, cross-reference).
    RefersTo,
}

/// A directed edge between two blocks in a [`BlockGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: BlockIdx,
    pub to: BlockIdx,
    pub relation: Relation,
}

/// Arena-based document structure preserving layout topology.
///
/// `reading_order` is the authoritative linearization used for retrieval
/// and expansion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockGraph {
    pub blocks: Vec<Block>,
    pub edges: Vec<Edge>,
    /// Deterministic reading order as indices into `blocks`.
    pub reading_order: Vec<BlockIdx>,
}
