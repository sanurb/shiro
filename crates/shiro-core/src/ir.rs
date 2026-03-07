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
    /// Original text for display/rendering (preserves formatting).
    pub rendered_text: Option<String>,
    pub metadata: Metadata,
    /// Structural representation. Every parser produces blocks + reading_order;
    /// an empty BlockGraph means the document had no parseable content.
    pub blocks: BlockGraph,
    /// Content that was lost or degraded during parsing.
    pub losses: Vec<ParseLoss>,
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

/// A record of content that could not be faithfully represented in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseLoss {
    /// What kind of content was lost.
    pub kind: LossKind,
    /// Byte span in the original source where the loss occurred.
    pub span: Option<Span>,
    /// Human-readable description of what was lost.
    pub message: String,
}

/// Categories of parsing loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LossKind {
    /// An image/figure that was referenced but not inlined.
    Image,
    /// A table that was flattened or simplified.
    Table,
    /// A mathematical formula (LaTeX, etc.).
    Math,
    /// An embedded media reference (video, audio).
    Media,
    /// A complex layout element (columns, sidebars).
    Layout,
    /// Encoding issue (invalid bytes, unsupported charset).
    Encoding,
    /// Any other loss not covered above.
    Other,
}

impl std::fmt::Display for LossKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image => write!(f, "image"),
            Self::Table => write!(f, "table"),
            Self::Math => write!(f, "math"),
            Self::Media => write!(f, "media"),
            Self::Layout => write!(f, "layout"),
            Self::Encoding => write!(f, "encoding"),
            Self::Other => write!(f, "other"),
        }
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

use std::collections::HashSet;
use std::fmt;

/// An invariant violation found during BlockGraph validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrViolation {
    /// A block's span exceeds the document's canonical_text length.
    SpanOutOfBounds {
        block: usize,
        span_end: usize,
        text_len: usize,
    },
    /// reading_order contains an index that doesn't exist in blocks.
    InvalidReadingOrderIndex { index: usize, blocks_len: usize },
    /// reading_order doesn't cover all blocks (it should be a permutation).
    ReadingOrderIncomplete { expected: usize, got: usize },
    /// reading_order contains duplicate block indices.
    ReadingOrderDuplicate { index: usize },
    /// An edge references a block index out of bounds.
    EdgeOutOfBounds {
        edge_idx: usize,
        block_ref: usize,
        blocks_len: usize,
    },
    /// ReadsBefore edges form a cycle.
    CycleDetected { involved_blocks: Vec<usize> },
}

impl fmt::Display for IrViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpanOutOfBounds {
                block,
                span_end,
                text_len,
            } => {
                write!(
                    f,
                    "block {block}: span end {span_end} exceeds text length {text_len}"
                )
            }
            Self::InvalidReadingOrderIndex { index, blocks_len } => {
                write!(
                    f,
                    "reading_order index {index} out of bounds (blocks len {blocks_len})"
                )
            }
            Self::ReadingOrderIncomplete { expected, got } => {
                write!(f, "reading_order has {got} entries, expected {expected}")
            }
            Self::ReadingOrderDuplicate { index } => {
                write!(f, "reading_order contains duplicate index {index}")
            }
            Self::EdgeOutOfBounds {
                edge_idx,
                block_ref,
                blocks_len,
            } => {
                write!(f, "edge {edge_idx}: block ref {block_ref} out of bounds (blocks len {blocks_len})")
            }
            Self::CycleDetected { involved_blocks } => {
                write!(
                    f,
                    "ReadsBefore cycle detected involving blocks {involved_blocks:?}"
                )
            }
        }
    }
}

impl BlockGraph {
    /// An empty graph with no blocks, edges, or reading order.
    pub fn empty() -> Self {
        Self {
            blocks: Vec::new(),
            edges: Vec::new(),
            reading_order: Vec::new(),
        }
    }
}

impl BlockGraph {
    /// Validate all structural invariants. Returns violations found.
    ///
    /// Checks (per docs/ARCHITECTURE.md):
    /// 1. All block spans within `[0, canonical_text_len)`
    /// 2. reading_order is a permutation of block indices
    /// 3. All edge endpoints reference valid blocks
    /// 4. ReadsBefore edges are acyclic
    pub fn validate(&self, canonical_text_len: usize) -> Vec<IrViolation> {
        let mut violations = Vec::new();
        let n = self.blocks.len();

        // 1. Span bounds
        for (i, block) in self.blocks.iter().enumerate() {
            if block.span.end() > canonical_text_len {
                violations.push(IrViolation::SpanOutOfBounds {
                    block: i,
                    span_end: block.span.end(),
                    text_len: canonical_text_len,
                });
            }
        }

        // 2. reading_order: valid indices
        for idx in &self.reading_order {
            if idx.0 >= n {
                violations.push(IrViolation::InvalidReadingOrderIndex {
                    index: idx.0,
                    blocks_len: n,
                });
            }
        }

        // 3. reading_order: completeness
        if self.reading_order.len() != n {
            violations.push(IrViolation::ReadingOrderIncomplete {
                expected: n,
                got: self.reading_order.len(),
            });
        }

        // 4. reading_order: no duplicates
        {
            let mut seen = HashSet::with_capacity(self.reading_order.len());
            for idx in &self.reading_order {
                if !seen.insert(idx.0) {
                    violations.push(IrViolation::ReadingOrderDuplicate { index: idx.0 });
                }
            }
        }

        // 5. Edge bounds
        for (i, edge) in self.edges.iter().enumerate() {
            if edge.from.0 >= n {
                violations.push(IrViolation::EdgeOutOfBounds {
                    edge_idx: i,
                    block_ref: edge.from.0,
                    blocks_len: n,
                });
            }
            if edge.to.0 >= n {
                violations.push(IrViolation::EdgeOutOfBounds {
                    edge_idx: i,
                    block_ref: edge.to.0,
                    blocks_len: n,
                });
            }
        }

        // 6. Cycle detection on ReadsBefore edges (iterative 3-color DFS)
        if n > 0 {
            // Build adjacency list for ReadsBefore edges only
            let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
            for edge in &self.edges {
                if edge.relation == Relation::ReadsBefore && edge.from.0 < n && edge.to.0 < n {
                    adj[edge.from.0].push(edge.to.0);
                }
            }

            // 0 = White, 1 = Gray, 2 = Black
            let mut color = vec![0u8; n];
            // parent tracking for cycle reconstruction
            let mut parent = vec![usize::MAX; n];

            for start in 0..n {
                if color[start] != 0 {
                    continue;
                }
                let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
                color[start] = 1; // Gray

                while let Some((node, idx)) = stack.last_mut() {
                    let node = *node;
                    if *idx < adj[node].len() {
                        let next = adj[node][*idx];
                        *idx += 1;
                        match color[next] {
                            0 => {
                                // White → visit
                                color[next] = 1;
                                parent[next] = node;
                                stack.push((next, 0));
                            }
                            1 => {
                                // Gray → cycle found. Reconstruct.
                                let mut cycle = vec![next];
                                let mut cur = node;
                                while cur != next {
                                    cycle.push(cur);
                                    cur = parent[cur];
                                }
                                cycle.reverse();
                                violations.push(IrViolation::CycleDetected {
                                    involved_blocks: cycle,
                                });
                                // Stop after first cycle found
                                return violations;
                            }
                            _ => { /* Black → already finished */ }
                        }
                    } else {
                        color[node] = 2; // Black
                        stack.pop();
                    }
                }
            }
        }

        violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::Span;
    use proptest::collection::vec as pvec;
    use proptest::prelude::*;

    fn make_block(start: usize, end: usize) -> Block {
        Block {
            canonical_text: String::new(),
            rendered_text: None,
            kind: BlockKind::Paragraph,
            span: Span::new(start, end).unwrap(),
        }
    }

    fn valid_3_block_graph() -> BlockGraph {
        BlockGraph {
            blocks: vec![make_block(0, 5), make_block(5, 10), make_block(10, 15)],
            edges: vec![
                Edge {
                    from: BlockIdx(0),
                    to: BlockIdx(1),
                    relation: Relation::ReadsBefore,
                },
                Edge {
                    from: BlockIdx(1),
                    to: BlockIdx(2),
                    relation: Relation::ReadsBefore,
                },
            ],
            reading_order: vec![BlockIdx(0), BlockIdx(1), BlockIdx(2)],
        }
    }

    #[test]
    fn test_valid_graph() {
        let g = valid_3_block_graph();
        assert!(g.validate(15).is_empty());
    }

    #[test]
    fn test_span_out_of_bounds() {
        let g = BlockGraph {
            blocks: vec![make_block(0, 20)],
            edges: vec![],
            reading_order: vec![BlockIdx(0)],
        };
        let v = g.validate(10);
        assert_eq!(
            v,
            vec![IrViolation::SpanOutOfBounds {
                block: 0,
                span_end: 20,
                text_len: 10
            }]
        );
    }

    #[test]
    fn test_reading_order_incomplete() {
        let g = BlockGraph {
            blocks: vec![make_block(0, 5), make_block(5, 10)],
            edges: vec![],
            reading_order: vec![BlockIdx(0)],
        };
        let v = g.validate(10);
        assert!(v.contains(&IrViolation::ReadingOrderIncomplete {
            expected: 2,
            got: 1
        }));
    }

    #[test]
    fn test_reading_order_duplicate() {
        let g = BlockGraph {
            blocks: vec![make_block(0, 5), make_block(5, 10)],
            edges: vec![],
            reading_order: vec![BlockIdx(0), BlockIdx(0)],
        };
        let v = g.validate(10);
        assert!(v.contains(&IrViolation::ReadingOrderDuplicate { index: 0 }));
    }

    #[test]
    fn test_edge_out_of_bounds() {
        let g = BlockGraph {
            blocks: vec![make_block(0, 5)],
            edges: vec![Edge {
                from: BlockIdx(0),
                to: BlockIdx(5),
                relation: Relation::ReadsBefore,
            }],
            reading_order: vec![BlockIdx(0)],
        };
        let v = g.validate(5);
        assert!(v.contains(&IrViolation::EdgeOutOfBounds {
            edge_idx: 0,
            block_ref: 5,
            blocks_len: 1
        }));
    }

    #[test]
    fn test_cycle_detected() {
        let g = BlockGraph {
            blocks: vec![make_block(0, 5), make_block(5, 10), make_block(10, 15)],
            edges: vec![
                Edge {
                    from: BlockIdx(0),
                    to: BlockIdx(1),
                    relation: Relation::ReadsBefore,
                },
                Edge {
                    from: BlockIdx(1),
                    to: BlockIdx(2),
                    relation: Relation::ReadsBefore,
                },
                Edge {
                    from: BlockIdx(2),
                    to: BlockIdx(0),
                    relation: Relation::ReadsBefore,
                },
            ],
            reading_order: vec![BlockIdx(0), BlockIdx(1), BlockIdx(2)],
        };
        let v = g.validate(15);
        assert!(v
            .iter()
            .any(|v| matches!(v, IrViolation::CycleDetected { .. })));
    }

    #[test]
    fn test_empty_graph() {
        let g = BlockGraph {
            blocks: vec![],
            edges: vec![],
            reading_order: vec![],
        };
        assert!(g.validate(0).is_empty());
    }

    #[test]
    fn test_self_loop_detected() {
        let g = BlockGraph {
            blocks: vec![make_block(0, 5)],
            edges: vec![Edge {
                from: BlockIdx(0),
                to: BlockIdx(0),
                relation: Relation::ReadsBefore,
            }],
            reading_order: vec![BlockIdx(0)],
        };
        let v = g.validate(5);
        assert!(v
            .iter()
            .any(|v| matches!(v, IrViolation::CycleDetected { .. })));
    }

    #[test]
    fn test_non_reads_before_cycle_ignored() {
        // CaptionOf edges forming a cycle should NOT be flagged
        let g = BlockGraph {
            blocks: vec![make_block(0, 5), make_block(5, 10)],
            edges: vec![
                Edge {
                    from: BlockIdx(0),
                    to: BlockIdx(1),
                    relation: Relation::CaptionOf,
                },
                Edge {
                    from: BlockIdx(1),
                    to: BlockIdx(0),
                    relation: Relation::CaptionOf,
                },
            ],
            reading_order: vec![BlockIdx(0), BlockIdx(1)],
        };
        assert!(g.validate(10).is_empty());
    }

    #[test]
    fn test_display_messages() {
        let v = IrViolation::SpanOutOfBounds {
            block: 0,
            span_end: 20,
            text_len: 10,
        };
        assert_eq!(v.to_string(), "block 0: span end 20 exceeds text length 10");
    }

    /// Generate a valid BlockGraph with n blocks, all spans within text_len.
    fn arb_valid_graph(
        max_blocks: usize,
        text_len: usize,
    ) -> impl Strategy<Value = (BlockGraph, usize)> {
        (1..=max_blocks).prop_flat_map(move |n| {
            pvec(0..text_len, n).prop_map(move |starts| {
                let mut starts = starts;
                starts.sort();
                let blocks: Vec<Block> = starts
                    .iter()
                    .enumerate()
                    .map(|(i, &s)| {
                        let end = if i + 1 < starts.len() {
                            starts[i + 1].min(text_len)
                        } else {
                            text_len
                        };
                        let end = end.max(s); // ensure start <= end
                        Block {
                            canonical_text: String::new(),
                            rendered_text: None,
                            kind: BlockKind::Paragraph,
                            span: Span::new(s, end).unwrap(),
                        }
                    })
                    .collect();
                let reading_order: Vec<BlockIdx> = (0..blocks.len()).map(BlockIdx).collect();
                let graph = BlockGraph {
                    blocks,
                    edges: vec![],
                    reading_order,
                };
                (graph, text_len)
            })
        })
    }

    proptest! {
        #[test]
        fn valid_graph_validates_clean((graph, text_len) in arb_valid_graph(10, 100)) {
            let violations = graph.validate(text_len);
            prop_assert!(violations.is_empty(), "unexpected violations: {:?}", violations);
        }

        #[test]
        fn span_shrink_never_oob(
            text_len in 1..200usize,
            start in 0..200usize,
            end in 0..200usize,
        ) {
            // Any span within bounds should validate clean
            if start <= end && end <= text_len {
                let span = Span::new(start, end).unwrap();
                let block = Block {
                    canonical_text: String::new(),
                    rendered_text: None,
                    kind: BlockKind::Paragraph,
                    span,
                };
                let graph = BlockGraph {
                    blocks: vec![block],
                    edges: vec![],
                    reading_order: vec![BlockIdx(0)],
                };
                let violations = graph.validate(text_len);
                let oob: Vec<_> = violations.iter().filter(|v| matches!(v, IrViolation::SpanOutOfBounds { .. })).collect();
                prop_assert!(oob.is_empty(), "span [{start}, {end}) should be valid for text_len {text_len}");
            }
        }

        #[test]
        fn reading_order_permutation_is_valid(n in 1..20usize) {
            // Any permutation of 0..n should be a valid reading_order
            let blocks: Vec<Block> = (0..n).map(|i| Block {
                canonical_text: String::new(),
                rendered_text: None,
                kind: BlockKind::Paragraph,
                span: Span::new(i, i + 1).unwrap(),
            }).collect();
            // Identity permutation
            let reading_order: Vec<BlockIdx> = (0..n).map(BlockIdx).collect();
            let graph = BlockGraph { blocks, edges: vec![], reading_order };
            let violations = graph.validate(n);
            let ro_issues: Vec<_> = violations.iter().filter(|v| {
                matches!(v, IrViolation::ReadingOrderIncomplete { .. } | IrViolation::ReadingOrderDuplicate { .. } | IrViolation::InvalidReadingOrderIndex { .. })
            }).collect();
            prop_assert!(ro_issues.is_empty(), "identity permutation should be valid");
        }
    }

    #[test]
    fn test_parse_loss_display() {
        assert_eq!(LossKind::Image.to_string(), "image");
        assert_eq!(LossKind::Table.to_string(), "table");
        assert_eq!(LossKind::Math.to_string(), "math");
        assert_eq!(LossKind::Media.to_string(), "media");
        assert_eq!(LossKind::Layout.to_string(), "layout");
        assert_eq!(LossKind::Encoding.to_string(), "encoding");
        assert_eq!(LossKind::Other.to_string(), "other");
    }
    proptest! {
        #[test]
        fn non_reads_before_edges_stay_acyclic(
            n in 2..15usize,
            edge_count in 0..20usize,
        ) {
            let blocks: Vec<Block> = (0..n).map(|i| Block {
                canonical_text: String::new(),
                rendered_text: None,
                kind: BlockKind::Paragraph,
                span: Span::new(i, i + 1).unwrap(),
            }).collect();
            let reading_order: Vec<BlockIdx> = (0..n).map(BlockIdx).collect();
            // Generate edges with non-ReadsBefore relations only
            let relations = [Relation::CaptionOf, Relation::FootnoteOf, Relation::RefersTo];
            let edges: Vec<Edge> = (0..edge_count).map(|i| Edge {
                from: BlockIdx(i % n),
                to: BlockIdx((i + 1) % n),
                relation: relations[i % relations.len()],
            }).collect();
            let graph = BlockGraph { blocks, edges, reading_order };
            let violations = graph.validate(n);
            let cycles: Vec<_> = violations.iter().filter(|v| matches!(v, IrViolation::CycleDetected { .. })).collect();
            prop_assert!(cycles.is_empty(), "non-ReadsBefore edges should never cause cycle violations");
        }
    }

    proptest! {
        #[test]
        fn missing_reading_order_entry_detected(n in 2..20usize, remove_idx in 0..20usize) {
            let remove_idx = remove_idx % n;
            let blocks: Vec<Block> = (0..n).map(|i| Block {
                canonical_text: String::new(),
                rendered_text: None,
                kind: BlockKind::Paragraph,
                span: Span::new(i, i + 1).unwrap(),
            }).collect();
            let mut reading_order: Vec<BlockIdx> = (0..n).map(BlockIdx).collect();
            reading_order.remove(remove_idx);
            let graph = BlockGraph { blocks, edges: vec![], reading_order };
            let violations = graph.validate(n);
            prop_assert!(
                violations.iter().any(|v| matches!(v, IrViolation::ReadingOrderIncomplete { .. })),
                "removing entry should cause ReadingOrderIncomplete, got: {:?}", violations
            );
        }
    }
}
