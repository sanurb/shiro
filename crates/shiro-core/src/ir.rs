//! Document intermediate representation.
//!
//! Two layers:
//! - **Segment**: flat, indexable text chunk (flows to FTS/vector stores).
//! - **Block / BlockGraph**: structural arena preserving layout topology
//!   (reading order, captions, footnotes). Produced by structure-aware parsers.
//!
//! The canonical pipeline: `Parser → BlockGraph → Segmenter → Vec<Segment>`.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::id::{DocId, SegmentId};
use crate::span::Span;

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

/// Source metadata for a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// Human-readable title (extracted or inferred).
    pub title: Option<String>,
    /// Filesystem path the document was ingested from.
    pub source: Utf8PathBuf,
}

// ---------------------------------------------------------------------------
// Segment (flat, indexable)
// ---------------------------------------------------------------------------

/// A parsed document ready for storage and indexing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    pub metadata: Metadata,
    pub segments: Vec<Segment>,
    /// Structural representation. `None` for formats that only produce flat text
    /// (e.g. plain-text parser).
    pub blocks: Option<BlockGraph>,
}

/// A contiguous chunk of content within a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id: SegmentId,
    pub doc_id: DocId,
    /// Byte span of this segment within the original document.
    pub span: Span,
    /// The textual content of this segment.
    pub content: String,
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
/// Provenance model:
/// - `canonical_text`: source-faithful representation preserving original
///   whitespace, ligatures, and encoding. This is the archival record.
/// - `rendered_text`: normalized form suitable for display, comparison,
///   and indexing. Populated by the [`Normalizer`](crate::ports::Normalizer)
///   pass.
///
/// TODO: populate `rendered_text` during normalization.
/// Acceptance: normalized text strips extraneous whitespace, ligatures,
/// and control chars while preserving semantic content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Source-faithful text (archival).
    pub canonical_text: String,
    /// Normalized text for display/indexing. `None` until normalization runs.
    pub rendered_text: Option<String>,
    pub kind: BlockKind,
    /// Byte span within the source document.
    pub span: Span,
}

/// Semantic relation between two blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Relation {
    /// Block `from` should be read before block `to`.
    ReadsBefore,
    /// Block `from` is a caption of block `to` (e.g. figure caption).
    CaptionOf,
    /// Block `from` is a footnote referenced by block `to`.
    FootnoteOf,
    /// Block `from` references block `to` (citation, cross-reference).
    References,
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
/// The `reading_order` field holds a deterministic sequence of `BlockIdx`
/// values representing the intended consumption order.
///
/// TODO: implement reading-order algorithm for PDF layout analysis.
/// Acceptance: ordering respects column layout, headers-before-body,
/// and footnotes-after-reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockGraph {
    pub blocks: Vec<Block>,
    pub edges: Vec<Edge>,
    /// Deterministic reading order as indices into `blocks`.
    pub reading_order: Vec<BlockIdx>,
}
