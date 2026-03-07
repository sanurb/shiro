//! Plain-text parser and document segmenter.
//!
//! Implements the [`Parser`] trait for plain-text content and provides
//! a standalone [`segment_document`] function that splits a [`Document`]
//! into indexable [`Segment`]s.

use shiro_core::ir::{
    Block, BlockGraph, BlockIdx, BlockKind, Document, Edge, Metadata, Relation, Segment,
};
use shiro_core::ports::Parser;
use shiro_core::{DocId, SegmentId, ShiroError, Span};

pub mod markdown;
pub use markdown::MarkdownParser;
pub mod pdf;
pub use pdf::PdfParser;

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// A parser that treats the entire input as plain UTF-8 text.
#[derive(Debug, Clone, Copy)]
pub struct PlainTextParser;

impl Parser for PlainTextParser {
    fn name(&self) -> &str {
        "plaintext"
    }

    fn parse(&self, source_uri: &str, content: &[u8]) -> Result<Document, ShiroError> {
        let text = std::str::from_utf8(content).map_err(|e| ShiroError::ParseMd {
            message: format!("invalid UTF-8: {e}"),
        })?;

        let id = DocId::from_content(content);
        let source_hash = blake3::hash(content).to_hex().to_string();
        let blocks = build_paragraph_block_graph(text);

        Ok(Document {
            id,
            canonical_text: text.to_string(),
            rendered_text: None,
            metadata: Metadata {
                title: extract_title(text),
                source_uri: source_uri.to_string(),
                source_hash,
            },
            blocks,
            losses: Vec::new(),
        })
    }
}

/// Return the first non-empty line trimmed, or `None` if there are none.
fn extract_title(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(String::from)
}

// ---------------------------------------------------------------------------
// Segmenter
// ---------------------------------------------------------------------------

/// Segment a document into indexable chunks.
///
/// Uses `reading_order` to derive segments from the block arena. Every
/// parser now produces a `BlockGraph`, so there is no text-split fallback.
pub fn segment_document(doc: &Document) -> Result<Vec<Segment>, ShiroError> {
    segment_from_blocks(doc, &doc.blocks)
}

fn segment_from_blocks(
    doc: &Document,
    graph: &shiro_core::BlockGraph,
) -> Result<Vec<Segment>, ShiroError> {
    let mut segments = Vec::new();
    let mut seg_idx = 0usize;

    for block_idx in &graph.reading_order {
        let BlockIdx(idx) = *block_idx;
        let block = graph.blocks.get(idx).ok_or_else(|| ShiroError::InvalidIr {
            message: format!(
                "block index {idx} out of range (len={})",
                graph.blocks.len()
            ),
        })?;

        let trimmed = block.canonical_text.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Use the block's span directly — it already references canonical_text.
        segments.push(Segment {
            id: SegmentId::new(&doc.id, seg_idx),
            doc_id: doc.id.clone(),
            index: seg_idx,
            span: block.span,
            body: trimmed.to_string(),
        });
        seg_idx += 1;
    }

    Ok(segments)
}

/// Build a [`BlockGraph`] from plain text by splitting on paragraph
/// boundaries (`\n\n`). Exported for reuse by other baseline parsers (e.g. PDF).
pub fn build_paragraph_block_graph(text: &str) -> BlockGraph {
    let mut blocks: Vec<Block> = Vec::new();

    for part in text.split("\n\n") {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Pointer arithmetic: offset of `part` within `text`.
        let offset = part.as_ptr() as usize - text.as_ptr() as usize;
        // Where the trimmed content starts within `part`.
        let trim_start = trimmed.as_ptr() as usize - part.as_ptr() as usize;
        let start = offset + trim_start;
        let end = start + trimmed.len();

        if let Ok(span) = Span::new(start, end) {
            blocks.push(Block {
                canonical_text: trimmed.to_string(),
                rendered_text: None,
                kind: BlockKind::Paragraph,
                span,
            });
        }
    }

    let reading_order: Vec<BlockIdx> = (0..blocks.len()).map(BlockIdx).collect();

    let mut edges = Vec::new();
    for window in reading_order.windows(2) {
        edges.push(Edge {
            from: window[0],
            to: window[1],
            relation: Relation::ReadsBefore,
        });
    }

    BlockGraph {
        blocks,
        edges,
        reading_order,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let parser = PlainTextParser;
        let content = b"Hello World\n\nThis is a test.";
        let doc = parser.parse("file:///test.txt", content).unwrap();

        assert!(doc.id.as_str().starts_with("doc_"));
        assert_eq!(doc.canonical_text, "Hello World\n\nThis is a test.");
        assert_eq!(doc.metadata.source_uri, "file:///test.txt");
        assert_eq!(doc.metadata.title.as_deref(), Some("Hello World"));
        assert!(!doc.metadata.source_hash.is_empty());
        assert_eq!(doc.blocks.blocks.len(), 2);
        assert_eq!(doc.blocks.reading_order.len(), 2);
    }

    #[test]
    fn test_parse_empty() {
        let parser = PlainTextParser;
        let doc = parser.parse("file:///empty.txt", b"").unwrap();

        assert_eq!(doc.canonical_text, "");
        assert!(doc.metadata.title.is_none());
    }

    #[test]
    fn test_parse_non_utf8() {
        let parser = PlainTextParser;
        let bad = &[0xFF, 0xFE, 0x00];
        let result = parser.parse("file:///bad.bin", bad);
        assert!(result.is_err());
    }

    #[test]
    fn test_segment_paragraphs() {
        let parser = PlainTextParser;
        let content = b"First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let doc = parser.parse("file:///test.txt", content).unwrap();
        let segments = segment_document(&doc).unwrap();

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].body, "First paragraph.");
        assert_eq!(segments[1].body, "Second paragraph.");
        assert_eq!(segments[2].body, "Third paragraph.");
        assert_eq!(segments[0].index, 0);
        assert_eq!(segments[1].index, 1);
        assert_eq!(segments[2].index, 2);
    }

    #[test]
    fn test_segment_preserves_spans() {
        let text = "Hello\n\nWorld\n\nFoo";
        let parser = PlainTextParser;
        let doc = parser.parse("file:///test.txt", text.as_bytes()).unwrap();
        let segments = segment_document(&doc).unwrap();

        assert_eq!(segments.len(), 3);
        for seg in &segments {
            let extracted = &doc.canonical_text[seg.span.start()..seg.span.end()];
            assert_eq!(
                extracted, seg.body,
                "span must index correctly into canonical_text"
            );
        }
    }

    #[test]
    fn test_segment_empty_document() {
        let parser = PlainTextParser;
        let doc = parser.parse("file:///empty.txt", b"").unwrap();
        let segments = segment_document(&doc).unwrap();
        assert!(segments.is_empty());
    }

    #[test]
    fn test_segment_single_paragraph() {
        let parser = PlainTextParser;
        let doc = parser
            .parse("file:///one.txt", b"Just one paragraph.")
            .unwrap();
        let segments = segment_document(&doc).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].body, "Just one paragraph.");
    }

    #[test]
    fn test_segment_whitespace_only() {
        let parser = PlainTextParser;
        let doc = parser
            .parse("file:///ws.txt", b"   \n\n   \n\n   ")
            .unwrap();
        let segments = segment_document(&doc).unwrap();
        assert!(segments.is_empty());
    }

    #[test]
    fn test_title_extraction() {
        assert_eq!(
            extract_title("First line\nSecond"),
            Some("First line".to_string())
        );
        assert_eq!(
            extract_title("\n\n  Title  \nBody"),
            Some("Title".to_string())
        );
        assert_eq!(extract_title(""), None);
        assert_eq!(extract_title("   \n   \n   "), None);
    }

    #[test]
    fn test_reads_before_edges_multi_paragraph() {
        let text = "Alpha\n\nBravo\n\nCharlie";
        let graph = build_paragraph_block_graph(text);
        assert!(graph.validate(text.len()).is_empty(), "graph must be valid");

        assert_eq!(graph.blocks.len(), 3);
        assert_eq!(graph.edges.len(), 2);

        assert_eq!(graph.edges[0].from, BlockIdx(0));
        assert_eq!(graph.edges[0].to, BlockIdx(1));
        assert!(matches!(graph.edges[0].relation, Relation::ReadsBefore));

        assert_eq!(graph.edges[1].from, BlockIdx(1));
        assert_eq!(graph.edges[1].to, BlockIdx(2));
        assert!(matches!(graph.edges[1].relation, Relation::ReadsBefore));
    }

    #[test]
    fn test_reads_before_edges_single_paragraph() {
        let text = "Only one paragraph";
        let graph = build_paragraph_block_graph(text);
        assert!(graph.validate(text.len()).is_empty(), "graph must be valid");

        assert_eq!(graph.blocks.len(), 1);
        assert_eq!(graph.edges.len(), 0);
    }

    #[test]
    fn test_reads_before_edges_empty() {
        let text = "";
        let graph = build_paragraph_block_graph(text);
        assert!(graph.validate(text.len()).is_empty(), "graph must be valid");

        assert_eq!(graph.blocks.len(), 0);
        assert_eq!(graph.edges.len(), 0);
    }

    #[test]
    fn test_reads_before_edge_count_matches_blocks() {
        let text = "A\n\nB\n\nC\n\nD\n\nE";
        let graph = build_paragraph_block_graph(text);
        assert!(graph.validate(text.len()).is_empty(), "graph must be valid");

        assert_eq!(graph.blocks.len(), 5);
        // n blocks => n-1 ReadsBefore edges
        assert_eq!(graph.edges.len(), graph.blocks.len() - 1);
    }
}
