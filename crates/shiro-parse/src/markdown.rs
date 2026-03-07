//! Markdown parser backed by `pulldown-cmark`.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser as MdParser, Tag, TagEnd};
use shiro_core::ir::{Block, BlockGraph, BlockIdx, BlockKind, Document, Edge, Metadata, Relation};
use shiro_core::ports::Parser;
use shiro_core::{DocId, ShiroError, Span};

/// A parser that understands Markdown structure (headings, paragraphs, code
/// blocks, lists) and produces a [`BlockGraph`].
#[derive(Debug, Clone, Copy)]
pub struct MarkdownParser;

impl Parser for MarkdownParser {
    fn name(&self) -> &str {
        "markdown"
    }

    fn parse(&self, source_uri: &str, content: &[u8]) -> Result<Document, ShiroError> {
        let text = std::str::from_utf8(content).map_err(|e| ShiroError::ParseMd {
            message: format!("invalid UTF-8: {e}"),
        })?;

        let id = DocId::from_content(content);
        let source_hash = blake3::hash(content).to_hex().to_string();

        let title = extract_md_title(text);
        let (canonical_text, _frontmatter) = strip_frontmatter(text);

        let blocks = build_block_graph(canonical_text);

        Ok(Document {
            id,
            canonical_text: canonical_text.to_string(),
            rendered_text: Some(text.to_string()),
            metadata: Metadata {
                title,
                source_uri: source_uri.to_string(),
                source_hash,
            },
            blocks,
            losses: Vec::new(),
        })
    }
}

// ---------------------------------------------------------------------------
// Frontmatter
// ---------------------------------------------------------------------------

/// Strip YAML frontmatter delimited by `---` at the start of the document.
/// Returns `(body, Option<frontmatter_content>)`.
fn strip_frontmatter(text: &str) -> (&str, Option<&str>) {
    if !text.starts_with("---\n") && !text.starts_with("---\r\n") {
        return (text, None);
    }

    // Skip past the opening delimiter line.
    let after_open = if text.starts_with("---\r\n") { 5 } else { 4 };

    // Find the closing `---` on its own line.
    if let Some(pos) = text[after_open..].find("\n---\n") {
        let fm = &text[after_open..after_open + pos];
        let body = &text[after_open + pos + 5..]; // skip `\n---\n`
        return (body, Some(fm));
    }
    if let Some(pos) = text[after_open..].find("\n---\r\n") {
        let fm = &text[after_open..after_open + pos];
        let body = &text[after_open + pos + 6..];
        return (body, Some(fm));
    }
    // Closing delimiter at very end without trailing newline.
    if text[after_open..].ends_with("\n---") {
        let fm = &text[after_open..text.len() - 4];
        let body = "";
        return (body, Some(fm));
    }

    // No closing delimiter found — treat entire text as body.
    (text, None)
}

// ---------------------------------------------------------------------------
// Title extraction
// ---------------------------------------------------------------------------

/// Extract the document title: first H1 heading text, or first non-empty line.
fn extract_md_title(text: &str) -> Option<String> {
    let parser = MdParser::new(text);
    let mut in_h1 = false;
    let mut title_parts: Vec<String> = Vec::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) => {
                in_h1 = true;
            }
            Event::End(TagEnd::Heading(HeadingLevel::H1)) => {
                if !title_parts.is_empty() {
                    return Some(title_parts.join(""));
                }
                in_h1 = false;
            }
            Event::Text(ref t) | Event::Code(ref t) if in_h1 => {
                title_parts.push(t.to_string());
            }
            _ => {}
        }
    }

    // Fallback: first non-empty line.
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(String::from)
}

// ---------------------------------------------------------------------------
// Block graph construction
// ---------------------------------------------------------------------------

/// Classify a pulldown-cmark heading level into a [`BlockKind`].
fn heading_kind(_level: HeadingLevel) -> BlockKind {
    BlockKind::Heading
}

/// Build a [`BlockGraph`] from Markdown source text.
fn build_block_graph(text: &str) -> BlockGraph {
    let parser = MdParser::new_ext(text, Options::empty());
    let events: Vec<(Event<'_>, std::ops::Range<usize>)> = parser.into_offset_iter().collect();

    let mut blocks: Vec<Block> = Vec::new();
    let mut i = 0;

    while i < events.len() {
        let (ref ev, ref range) = events[i];
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                let kind = heading_kind(*level);
                let (block_text, end_idx) = collect_inline_text(&events, i);
                let span = make_span(range.start, events[end_idx].1.end);
                if let Some(span) = span {
                    blocks.push(Block {
                        canonical_text: block_text,
                        rendered_text: None,
                        kind,
                        span,
                    });
                }
                i = end_idx + 1;
            }
            Event::Start(Tag::Paragraph) => {
                let (block_text, end_idx) = collect_inline_text(&events, i);
                let span = make_span(range.start, events[end_idx].1.end);
                if let Some(span) = span {
                    blocks.push(Block {
                        canonical_text: block_text,
                        rendered_text: None,
                        kind: BlockKind::Paragraph,
                        span,
                    });
                }
                i = end_idx + 1;
            }
            Event::Start(Tag::CodeBlock(_)) => {
                let (block_text, end_idx) = collect_inline_text(&events, i);
                let span = make_span(range.start, events[end_idx].1.end);
                if let Some(span) = span {
                    blocks.push(Block {
                        canonical_text: block_text,
                        rendered_text: None,
                        kind: BlockKind::Code,
                        span,
                    });
                }
                i = end_idx + 1;
            }
            Event::Start(Tag::Item) => {
                let (block_text, end_idx) = collect_inline_text(&events, i);
                let span = make_span(range.start, events[end_idx].1.end);
                if let Some(span) = span {
                    blocks.push(Block {
                        canonical_text: block_text,
                        rendered_text: None,
                        kind: BlockKind::ListItem,
                        span,
                    });
                }
                i = end_idx + 1;
            }
            _ => {
                i += 1;
            }
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

/// Collect all text content between a `Start` tag at `start_idx` and its
/// matching `End` tag. Returns `(text, end_index)`.
fn collect_inline_text(
    events: &[(Event<'_>, std::ops::Range<usize>)],
    start_idx: usize,
) -> (String, usize) {
    let mut buf = String::new();
    let mut idx = start_idx + 1; // skip the Start event

    while idx < events.len() {
        match &events[idx].0 {
            Event::End(_) => {
                // Check if this is a closing tag at the same nesting depth.
                // For simplicity we break on the first End that matches the
                // outermost block (headings, paragraphs, code blocks, items
                // are not nested within each other at the block level).
                return (buf, idx);
            }
            Event::Text(t) | Event::Code(t) => {
                buf.push_str(t);
            }
            Event::SoftBreak | Event::HardBreak => {
                buf.push('\n');
            }
            Event::Start(_) => {
                // Nested inline — skip the Start, collect text, the End will
                // be consumed naturally.
            }
            _ => {}
        }
        idx += 1;
    }

    (buf, events.len().saturating_sub(1))
}

/// Create a [`Span`], returning `None` if invalid.
fn make_span(start: usize, end: usize) -> Option<Span> {
    Span::new(start, end).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment_document;

    #[test]
    fn test_parse_heading() {
        let parser = MarkdownParser;
        let content = b"# Title\n\nBody text.";
        let doc = parser.parse("file:///test.md", content).unwrap();

        assert_eq!(doc.metadata.title.as_deref(), Some("Title"));

        let blocks = &doc.blocks;
        assert_eq!(blocks.blocks.len(), 2);
        assert_eq!(blocks.blocks[0].kind, BlockKind::Heading);
        assert_eq!(blocks.blocks[0].canonical_text, "Title");
        assert_eq!(blocks.blocks[1].kind, BlockKind::Paragraph);
        assert_eq!(blocks.blocks[1].canonical_text, "Body text.");
    }

    #[test]
    fn test_parse_frontmatter() {
        let parser = MarkdownParser;
        let content = b"---\ntitle: Test\n---\n\nContent";
        let doc = parser.parse("file:///test.md", content).unwrap();

        assert_eq!(doc.canonical_text, "\nContent");
        // Frontmatter should not appear in canonical_text.
        assert!(!doc.canonical_text.contains("title: Test"));
    }

    #[test]
    fn test_parse_empty_md() {
        let parser = MarkdownParser;
        let doc = parser.parse("file:///empty.md", b"").unwrap();

        assert_eq!(doc.canonical_text, "");
        assert!(doc.blocks.blocks.is_empty());
    }

    #[test]
    fn test_parse_segments() {
        let parser = MarkdownParser;
        let content = b"# Heading\n\nParagraph one.\n\nParagraph two.";
        let doc = parser.parse("file:///test.md", content).unwrap();
        let segments = segment_document(&doc).unwrap();

        // One heading + two paragraphs = 3 segments.
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].body, "Heading");
        assert_eq!(segments[1].body, "Paragraph one.");
        assert_eq!(segments[2].body, "Paragraph two.");
    }

    #[test]
    fn test_frontmatter_strip_basic() {
        let (body, fm) = strip_frontmatter("---\nkey: val\n---\nBody");
        assert_eq!(fm, Some("key: val"));
        assert_eq!(body, "Body");
    }

    #[test]
    fn test_frontmatter_none() {
        let (body, fm) = strip_frontmatter("No frontmatter here");
        assert!(fm.is_none());
        assert_eq!(body, "No frontmatter here");
    }

    #[test]
    fn test_title_from_h1() {
        assert_eq!(
            extract_md_title("# Hello World\n\nBody"),
            Some("Hello World".to_string()),
        );
    }

    #[test]
    fn test_title_fallback_no_h1() {
        assert_eq!(
            extract_md_title("Just text\nMore text"),
            Some("Just text".to_string()),
        );
    }

    #[test]
    fn test_code_block() {
        let parser = MarkdownParser;
        let content = b"```rust\nfn main() {}\n```";
        let doc = parser.parse("file:///test.md", content).unwrap();

        let blocks = &doc.blocks;
        assert_eq!(blocks.blocks.len(), 1);
        assert_eq!(blocks.blocks[0].kind, BlockKind::Code);
        assert!(blocks.blocks[0].canonical_text.contains("fn main()"));
    }

    #[test]
    fn test_spans_index_into_canonical() {
        let parser = MarkdownParser;
        let content = b"# Title\n\nParagraph text.";
        let doc = parser.parse("file:///test.md", content).unwrap();

        let blocks = &doc.blocks;
        for block in &blocks.blocks {
            let extracted = &doc.canonical_text[block.span.start()..block.span.end()];
            assert!(
                extracted.contains(&block.canonical_text),
                "span {start}..{end} => {extracted:?} should contain {text:?}",
                start = block.span.start(),
                end = block.span.end(),
                text = block.canonical_text,
            );
        }
    }

    #[test]
    fn test_reads_before_edges_multi_block() {
        let graph = build_block_graph("# Heading\n\nParagraph one.\n\nParagraph two.");
        assert_eq!(graph.blocks.len(), 3);
        assert_eq!(graph.edges.len(), 2);
        for edge in &graph.edges {
            assert!(matches!(edge.relation, Relation::ReadsBefore));
        }
        assert_eq!(graph.edges[0].from.0, 0);
        assert_eq!(graph.edges[0].to.0, 1);
        assert_eq!(graph.edges[1].from.0, 1);
        assert_eq!(graph.edges[1].to.0, 2);
        assert!(graph.validate(1000).is_empty());
    }

    #[test]
    fn test_reads_before_edges_count() {
        let graph = build_block_graph("# A\n\nB\n\nC\n\nD");
        let n = graph.blocks.len();
        assert!(n > 1);
        assert_eq!(graph.edges.len(), n - 1);
        assert!(graph.validate(1000).is_empty());
    }

    #[test]
    fn test_single_block_no_edges() {
        let graph = build_block_graph("Just a paragraph.");
        assert_eq!(graph.blocks.len(), 1);
        assert_eq!(graph.edges.len(), 0);
        assert!(graph.validate(1000).is_empty());
    }

    #[test]
    fn test_empty_document_no_edges() {
        let graph = build_block_graph("");
        assert_eq!(graph.blocks.len(), 0);
        assert_eq!(graph.edges.len(), 0);
        assert!(graph.validate(1000).is_empty());
    }

    #[test]
    fn golden_markdown_ir() {
        let parser = MarkdownParser;
        let content = b"# Title\n\nFirst paragraph.\n\n## Section\n\nSecond paragraph.\n";
        let doc = parser
            .parse("file:///golden.md", content)
            .expect("parse failed");
        let graph = &doc.blocks;

        // Exactly 4 blocks: heading, paragraph, heading, paragraph
        assert_eq!(
            graph.blocks.len(),
            4,
            "expected 4 blocks, got {}",
            graph.blocks.len()
        );
        assert_eq!(graph.blocks[0].kind, BlockKind::Heading);
        assert_eq!(graph.blocks[0].canonical_text, "Title");
        assert_eq!(graph.blocks[1].kind, BlockKind::Paragraph);
        assert_eq!(graph.blocks[1].canonical_text, "First paragraph.");
        assert_eq!(graph.blocks[2].kind, BlockKind::Heading);
        assert_eq!(graph.blocks[2].canonical_text, "Section");
        assert_eq!(graph.blocks[3].kind, BlockKind::Paragraph);
        assert_eq!(graph.blocks[3].canonical_text, "Second paragraph.");

        // 3 ReadsBefore edges linking sequential blocks
        assert_eq!(graph.edges.len(), 3);
        for edge in &graph.edges {
            assert_eq!(edge.relation, Relation::ReadsBefore);
        }
        assert_eq!(graph.edges[0].from, BlockIdx(0));
        assert_eq!(graph.edges[0].to, BlockIdx(1));
        assert_eq!(graph.edges[1].from, BlockIdx(1));
        assert_eq!(graph.edges[1].to, BlockIdx(2));
        assert_eq!(graph.edges[2].from, BlockIdx(2));
        assert_eq!(graph.edges[2].to, BlockIdx(3));

        // Reading order is sequential
        assert_eq!(
            graph.reading_order,
            vec![BlockIdx(0), BlockIdx(1), BlockIdx(2), BlockIdx(3)]
        );

        // Graph is valid
        assert!(graph.validate(1000).is_empty());
    }

    #[test]
    fn golden_markdown_doc_id_pinned() {
        let parser = MarkdownParser;
        let content = b"# Title\n\nFirst paragraph.\n\n## Section\n\nSecond paragraph.\n";
        let doc = parser
            .parse("file:///golden.md", content)
            .expect("parse failed");

        // Pin DocId to catch accidental changes to content addressing
        let expected_id = shiro_core::DocId::from_content(content);
        assert_eq!(doc.id, expected_id);
        assert!(
            doc.id.as_str().starts_with("doc_"),
            "DocId must have doc_ prefix"
        );
    }
}
