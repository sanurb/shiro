//! Translation layer: DoclingDocument → shiro canonical IR.
//!
//! This module converts Docling's internal representation into shiro's
//! `Document`, `BlockGraph`, and associated types. The translation is
//! deterministic: identical Docling JSON produces identical shiro IR.
//!
//! **Design invariants:**
//! - canonical_text is built by joining block texts with `\n\n` separators
//! - Spans are byte offsets into canonical_text (half-open)
//! - reading_order follows body tree depth-first traversal (Docling's reading order)
//! - ParseLoss records track content that couldn't be faithfully represented
//! - Docling types never leak past this module boundary

use shiro_core::ir::{
    Block, BlockGraph, BlockIdx, BlockKind, Document, Edge, LossKind, Metadata, ParseLoss, Relation,
};
use shiro_core::{DocId, Span};

use crate::schema::{ChildRef, DoclingDocument, TableData, TableItem, TextItem};

/// Translate a Docling document into shiro's canonical IR.
///
/// `raw_content` is the original file bytes (for DocId + source_hash).
/// `source_uri` is the original file path/URL.
pub fn translate(docling: &DoclingDocument, source_uri: &str, raw_content: &[u8]) -> Document {
    let id = DocId::from_content(raw_content);
    let source_hash = blake3::hash(raw_content).to_hex().to_string();

    let mut builder = IrBuilder::new();

    // Walk the body tree depth-first — this follows Docling's reading order.
    walk_children(&docling.body.children, docling, &mut builder);

    let (canonical_text, blocks, edges, losses) = builder.finish();

    let title = extract_title(docling, &blocks);

    let reading_order: Vec<BlockIdx> = (0..blocks.len()).map(BlockIdx).collect();

    Document {
        id,
        canonical_text,
        rendered_text: None,
        metadata: Metadata {
            title,
            source_uri: source_uri.to_string(),
            source_hash,
        },
        blocks: BlockGraph {
            blocks,
            edges,
            reading_order,
        },
        losses,
    }
}

/// Incremental builder for canonical_text + blocks + edges.
struct IrBuilder {
    canonical_text: String,
    blocks: Vec<Block>,
    edges: Vec<Edge>,
    losses: Vec<ParseLoss>,
}

impl IrBuilder {
    fn new() -> Self {
        Self {
            canonical_text: String::new(),
            blocks: Vec::new(),
            edges: Vec::new(),
            losses: Vec::new(),
        }
    }

    /// Append a block. Handles separator insertion and span tracking.
    fn push_block(&mut self, text: &str, kind: BlockKind) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        // Separator between blocks.
        let start = if self.canonical_text.is_empty() {
            0
        } else {
            self.canonical_text.push_str("\n\n");
            self.canonical_text.len()
        };

        self.canonical_text.push_str(trimmed);
        let end = self.canonical_text.len();

        let span = match Span::new(start, end) {
            Ok(s) => s,
            Err(_) => return, // Should never happen: start < end since trimmed is non-empty.
        };

        // ReadsBefore edge from previous block.
        let block_idx = self.blocks.len();
        if block_idx > 0 {
            self.edges.push(Edge {
                from: BlockIdx(block_idx - 1),
                to: BlockIdx(block_idx),
                relation: Relation::ReadsBefore,
            });
        }

        self.blocks.push(Block {
            canonical_text: trimmed.to_string(),
            rendered_text: None,
            kind,
            span,
        });
    }

    /// Record a parse loss.
    fn push_loss(&mut self, kind: LossKind, message: String) {
        self.losses.push(ParseLoss {
            kind,
            span: None,
            message,
        });
    }

    fn finish(self) -> (String, Vec<Block>, Vec<Edge>, Vec<ParseLoss>) {
        (self.canonical_text, self.blocks, self.edges, self.losses)
    }
}

/// Recursively walk the body tree's children in reading order.
fn walk_children(children: &[ChildRef], doc: &DoclingDocument, builder: &mut IrBuilder) {
    for child in children {
        match child {
            ChildRef::Inline(node) => {
                // Recurse into group/container.
                walk_children(&node.children, doc, builder);
            }
            ChildRef::Ref(ref_item) => {
                resolve_ref(&ref_item.reference, doc, builder);
            }
        }
    }
}

/// Resolve a JSON pointer reference (e.g. "#/texts/0") to a content block.
fn resolve_ref(reference: &str, doc: &DoclingDocument, builder: &mut IrBuilder) {
    let stripped = reference.strip_prefix('#').unwrap_or(reference);

    if let Some(idx_str) = stripped.strip_prefix("/texts/") {
        if let Ok(idx) = idx_str.parse::<usize>() {
            if let Some(text_item) = doc.texts.get(idx) {
                emit_text_block(text_item, builder);
                return;
            }
        }
        tracing::warn!(reference, "unresolvable text reference");
    } else if let Some(idx_str) = stripped.strip_prefix("/tables/") {
        if let Ok(idx) = idx_str.parse::<usize>() {
            if let Some(table_item) = doc.tables.get(idx) {
                emit_table_block(table_item, builder);
                return;
            }
        }
        tracing::warn!(reference, "unresolvable table reference");
    } else if let Some(idx_str) = stripped.strip_prefix("/pictures/") {
        if let Ok(idx) = idx_str.parse::<usize>() {
            if let Some(_picture) = doc.pictures.get(idx) {
                builder.push_loss(
                    LossKind::Image,
                    format!("picture at {reference} omitted (images not inlined)"),
                );
                return;
            }
        }
        tracing::warn!(reference, "unresolvable picture reference");
    } else {
        tracing::debug!(reference, "unknown reference prefix, skipping");
    }
}

/// Convert a Docling text item into a shiro block.
fn emit_text_block(item: &TextItem, builder: &mut IrBuilder) {
    let kind = label_to_block_kind(&item.label);
    builder.push_block(&item.text, kind);
}

/// Convert a Docling table item into a shiro block.
///
/// Tables are serialized as pipe-delimited text. If the table has structured
/// cell data, we produce a markdown-like table representation. Otherwise we
/// record a ParseLoss.
fn emit_table_block(item: &TableItem, builder: &mut IrBuilder) {
    match &item.data {
        Some(data) if !data.table_cells.is_empty() => {
            let text = render_table_as_text(data);
            if text.trim().is_empty() {
                builder.push_loss(
                    LossKind::Table,
                    "table with empty cells omitted".to_string(),
                );
            } else {
                builder.push_block(&text, BlockKind::TableCell);
            }
        }
        _ => {
            builder.push_loss(
                LossKind::Table,
                "table without structured data omitted".to_string(),
            );
        }
    }
}

/// Render table cells as pipe-delimited plain text.
///
/// Produces a readable representation that preserves table content for
/// indexing and retrieval while fitting within shiro's text-based IR.
fn render_table_as_text(data: &TableData) -> String {
    if data.num_rows == 0 || data.num_cols == 0 {
        return String::new();
    }

    // Build grid.
    let mut grid: Vec<Vec<String>> = vec![vec![String::new(); data.num_cols]; data.num_rows];
    for cell in &data.table_cells {
        if cell.row_index < data.num_rows && cell.col_index < data.num_cols {
            grid[cell.row_index][cell.col_index] = cell.text.clone();
        }
    }

    // Render as pipe-delimited rows.
    let mut lines = Vec::with_capacity(data.num_rows);
    for row in &grid {
        let line = row.join(" | ");
        lines.push(line);
    }

    lines.join("\n")
}

/// Map Docling's text label to shiro's BlockKind.
fn label_to_block_kind(label: &str) -> BlockKind {
    match label {
        "section_header" => BlockKind::Heading,
        "paragraph" | "text" => BlockKind::Paragraph,
        "list_item" => BlockKind::ListItem,
        "caption" | "figure_caption" | "table_caption" => BlockKind::Caption,
        "footnote" => BlockKind::Footnote,
        "code" => BlockKind::Code,
        "table" => BlockKind::TableCell,
        // Degrade gracefully: unknown labels become paragraphs.
        _ => BlockKind::Paragraph,
    }
}

/// Extract document title from Docling output.
///
/// Strategy: use Docling's `name` field, or the first heading, or the first
/// non-empty text block.
fn extract_title(docling: &DoclingDocument, blocks: &[Block]) -> Option<String> {
    // 1. Docling may provide a name.
    if let Some(name) = &docling.name {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return Some(truncate_title(trimmed));
        }
    }

    // 2. First heading block.
    if let Some(heading) = blocks.iter().find(|b| b.kind == BlockKind::Heading) {
        return Some(truncate_title(&heading.canonical_text));
    }

    // 3. First non-empty block.
    blocks.first().map(|b| truncate_title(&b.canonical_text))
}

fn truncate_title(s: &str) -> String {
    if s.len() > 200 {
        s[..200].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::*;

    /// Helper: build a minimal DoclingDocument for testing.
    fn minimal_doc(texts: Vec<TextItem>, body_refs: Vec<String>) -> DoclingDocument {
        let children = body_refs
            .into_iter()
            .map(|r| ChildRef::Ref(RefItem { reference: r }))
            .collect();

        DoclingDocument {
            schema_name: Some("DoclingDocument".to_string()),
            version: Some("1.0.0".to_string()),
            name: None,
            texts,
            tables: Vec::new(),
            pictures: Vec::new(),
            body: NodeItem {
                self_ref: Some("#/body".to_string()),
                label: None,
                children,
                content_layer: None,
            },
            furniture: NodeItem::default(),
            pages: std::collections::HashMap::new(),
        }
    }

    fn text_item(label: &str, text: &str, idx: usize) -> TextItem {
        TextItem {
            self_ref: Some(format!("#/texts/{idx}")),
            label: label.to_string(),
            text: text.to_string(),
            orig: None,
            prov: Vec::new(),
            level: None,
        }
    }

    #[test]
    fn translate_single_paragraph() {
        let doc = minimal_doc(
            vec![text_item("paragraph", "Hello world.", 0)],
            vec!["#/texts/0".to_string()],
        );
        let result = translate(&doc, "test.pdf", b"fake-pdf-content");

        assert_eq!(result.canonical_text, "Hello world.");
        assert_eq!(result.blocks.blocks.len(), 1);
        assert_eq!(result.blocks.blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(result.blocks.blocks[0].canonical_text, "Hello world.");
        assert!(result.losses.is_empty());
    }

    #[test]
    fn translate_multiple_blocks_reading_order() {
        let doc = minimal_doc(
            vec![
                text_item("section_header", "Introduction", 0),
                text_item("paragraph", "First paragraph.", 1),
                text_item("paragraph", "Second paragraph.", 2),
            ],
            vec![
                "#/texts/0".to_string(),
                "#/texts/1".to_string(),
                "#/texts/2".to_string(),
            ],
        );
        let result = translate(&doc, "test.pdf", b"fake-pdf-content");

        assert_eq!(
            result.canonical_text,
            "Introduction\n\nFirst paragraph.\n\nSecond paragraph."
        );
        assert_eq!(result.blocks.blocks.len(), 3);
        assert_eq!(result.blocks.blocks[0].kind, BlockKind::Heading);
        assert_eq!(result.blocks.blocks[1].kind, BlockKind::Paragraph);
        assert_eq!(result.blocks.blocks[2].kind, BlockKind::Paragraph);

        // ReadsBefore edges.
        assert_eq!(result.blocks.edges.len(), 2);
        assert_eq!(result.blocks.edges[0].from, BlockIdx(0));
        assert_eq!(result.blocks.edges[0].to, BlockIdx(1));
        assert_eq!(result.blocks.edges[1].from, BlockIdx(1));
        assert_eq!(result.blocks.edges[1].to, BlockIdx(2));

        // Reading order.
        assert_eq!(result.blocks.reading_order.len(), 3);
    }

    #[test]
    fn translate_spans_are_valid() {
        let doc = minimal_doc(
            vec![
                text_item("paragraph", "Alpha", 0),
                text_item("paragraph", "Bravo", 1),
            ],
            vec!["#/texts/0".to_string(), "#/texts/1".to_string()],
        );
        let result = translate(&doc, "test.pdf", b"x");
        let text = &result.canonical_text;

        for block in &result.blocks.blocks {
            let extracted = &text[block.span.start()..block.span.end()];
            assert_eq!(
                extracted, block.canonical_text,
                "span must index correctly into canonical_text"
            );
        }

        // Validate graph invariants.
        let violations = result.blocks.validate(result.canonical_text.len());
        assert!(violations.is_empty(), "violations: {violations:?}");
    }

    #[test]
    fn translate_picture_produces_loss() {
        let doc = DoclingDocument {
            schema_name: Some("DoclingDocument".to_string()),
            version: Some("1.0.0".to_string()),
            name: None,
            texts: vec![text_item("paragraph", "Before image.", 0)],
            tables: Vec::new(),
            pictures: vec![PictureItem {
                self_ref: Some("#/pictures/0".to_string()),
                label: "picture".to_string(),
                prov: Vec::new(),
            }],
            body: NodeItem {
                self_ref: None,
                label: None,
                children: vec![
                    ChildRef::Ref(RefItem {
                        reference: "#/texts/0".to_string(),
                    }),
                    ChildRef::Ref(RefItem {
                        reference: "#/pictures/0".to_string(),
                    }),
                ],
                content_layer: None,
            },
            furniture: NodeItem::default(),
            pages: std::collections::HashMap::new(),
        };

        let result = translate(&doc, "test.pdf", b"x");
        assert_eq!(result.blocks.blocks.len(), 1);
        assert_eq!(result.losses.len(), 1);
        assert_eq!(result.losses[0].kind, LossKind::Image);
    }

    #[test]
    fn translate_table_with_data() {
        let table = TableItem {
            self_ref: Some("#/tables/0".to_string()),
            label: "table".to_string(),
            prov: Vec::new(),
            data: Some(TableData {
                table_cells: vec![
                    crate::schema::TableCell {
                        text: "Name".to_string(),
                        row_index: 0,
                        col_index: 0,
                        row_span: 1,
                        col_span: 1,
                        is_header: true,
                    },
                    crate::schema::TableCell {
                        text: "Age".to_string(),
                        row_index: 0,
                        col_index: 1,
                        row_span: 1,
                        col_span: 1,
                        is_header: true,
                    },
                    crate::schema::TableCell {
                        text: "Alice".to_string(),
                        row_index: 1,
                        col_index: 0,
                        row_span: 1,
                        col_span: 1,
                        is_header: false,
                    },
                    crate::schema::TableCell {
                        text: "30".to_string(),
                        row_index: 1,
                        col_index: 1,
                        row_span: 1,
                        col_span: 1,
                        is_header: false,
                    },
                ],
                num_rows: 2,
                num_cols: 2,
            }),
        };

        let doc = DoclingDocument {
            schema_name: Some("DoclingDocument".to_string()),
            version: Some("1.0.0".to_string()),
            name: None,
            texts: Vec::new(),
            tables: vec![table],
            pictures: Vec::new(),
            body: NodeItem {
                self_ref: None,
                label: None,
                children: vec![ChildRef::Ref(RefItem {
                    reference: "#/tables/0".to_string(),
                })],
                content_layer: None,
            },
            furniture: NodeItem::default(),
            pages: std::collections::HashMap::new(),
        };

        let result = translate(&doc, "test.pdf", b"x");
        assert_eq!(result.blocks.blocks.len(), 1);
        assert_eq!(result.blocks.blocks[0].kind, BlockKind::TableCell);
        assert!(result.blocks.blocks[0].canonical_text.contains("Name"));
        assert!(result.blocks.blocks[0].canonical_text.contains("Alice"));
    }

    #[test]
    fn translate_empty_document() {
        let doc = minimal_doc(Vec::new(), Vec::new());
        let result = translate(&doc, "test.pdf", b"x");
        assert!(result.canonical_text.is_empty());
        assert!(result.blocks.blocks.is_empty());
    }

    #[test]
    fn translate_label_mapping() {
        assert_eq!(label_to_block_kind("section_header"), BlockKind::Heading);
        assert_eq!(label_to_block_kind("paragraph"), BlockKind::Paragraph);
        assert_eq!(label_to_block_kind("list_item"), BlockKind::ListItem);
        assert_eq!(label_to_block_kind("caption"), BlockKind::Caption);
        assert_eq!(label_to_block_kind("footnote"), BlockKind::Footnote);
        assert_eq!(label_to_block_kind("code"), BlockKind::Code);
        assert_eq!(label_to_block_kind("table"), BlockKind::TableCell);
        // Unknown labels degrade to paragraph.
        assert_eq!(label_to_block_kind("unknown_type"), BlockKind::Paragraph);
    }

    #[test]
    fn translate_title_from_name() {
        let mut doc = minimal_doc(
            vec![text_item("paragraph", "Body text.", 0)],
            vec!["#/texts/0".to_string()],
        );
        doc.name = Some("My Document Title".to_string());

        let result = translate(&doc, "test.pdf", b"x");
        assert_eq!(result.metadata.title.as_deref(), Some("My Document Title"));
    }

    #[test]
    fn translate_title_from_heading_when_no_name() {
        let doc = minimal_doc(
            vec![
                text_item("section_header", "Chapter One", 0),
                text_item("paragraph", "Body text.", 1),
            ],
            vec!["#/texts/0".to_string(), "#/texts/1".to_string()],
        );

        let result = translate(&doc, "test.pdf", b"x");
        assert_eq!(result.metadata.title.as_deref(), Some("Chapter One"));
    }

    #[test]
    fn translate_nested_groups() {
        // Simulate a section group containing children.
        let doc = DoclingDocument {
            schema_name: Some("DoclingDocument".to_string()),
            version: Some("1.0.0".to_string()),
            name: None,
            texts: vec![
                text_item("section_header", "Section 1", 0),
                text_item("paragraph", "Content A", 1),
                text_item("paragraph", "Content B", 2),
            ],
            tables: Vec::new(),
            pictures: Vec::new(),
            body: NodeItem {
                self_ref: None,
                label: None,
                children: vec![
                    ChildRef::Inline(NodeItem {
                        self_ref: None,
                        label: Some("group".to_string()),
                        children: vec![
                            ChildRef::Ref(RefItem {
                                reference: "#/texts/0".to_string(),
                            }),
                            ChildRef::Ref(RefItem {
                                reference: "#/texts/1".to_string(),
                            }),
                        ],
                        content_layer: None,
                    }),
                    ChildRef::Ref(RefItem {
                        reference: "#/texts/2".to_string(),
                    }),
                ],
                content_layer: None,
            },
            furniture: NodeItem::default(),
            pages: std::collections::HashMap::new(),
        };

        let result = translate(&doc, "test.pdf", b"x");
        assert_eq!(result.blocks.blocks.len(), 3);
        assert_eq!(result.canonical_text, "Section 1\n\nContent A\n\nContent B");
    }
}
