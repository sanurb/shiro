//! Canonical retrieval result materialization and explanation evidence.
//!
//! This module is the single policy home for turning an internal segment hit
//! into a public EntryPoint and for rendering the retrieval evidence persisted
//! with that EntryPoint. Search and explain must not reconstruct these facts
//! independently.

use serde::{Deserialize, Serialize};
use shiro_core::ir::{BlockGraph, Segment};
use shiro_core::{DocId, ShiroError};
use shiro_store::{SearchResultDetail, Store};

use crate::RRF_K;

/// A block in the reading-order context around an EntryPoint.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ContextBlock {
    pub block_idx: usize,
    pub kind: String,
    pub span_start: usize,
    pub span_end: usize,
    pub text: String,
}

/// The canonical public position and context derived from a segment hit.
pub(crate) struct MaterializedEntryPoint {
    pub block_idx: usize,
    pub block_kind: String,
    pub span_start: usize,
    pub span_end: usize,
    pub context_window: Vec<ContextBlock>,
}

/// Complete source evidence used by the public explain operation.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RetrievalTrace {
    pub pipeline: Vec<String>,
    pub stages: Vec<serde_json::Value>,
    pub fusion: serde_json::Value,
}

/// Materialize a segment hit against the persisted canonical BlockGraph.
///
/// ADR-007 requires a real block position. Missing, empty, or non-overlapping
/// graphs are system errors rather than degraded paragraph-zero results.
pub(crate) fn materialize_entry_point(
    store: &Store,
    doc_id: &DocId,
    segment: &Segment,
    expand: bool,
    max_blocks: usize,
    max_chars: usize,
) -> Result<MaterializedEntryPoint, ShiroError> {
    let graph = store
        .get_block_graph(doc_id)
        .map_err(|error| ShiroError::SearchFailed {
            message: format!("EntryPoint block graph unavailable for {doc_id}: {error}"),
        })?;
    validate_entry_point_graph(doc_id, &graph)?;

    let block_idx =
        resolve_segment_block(&graph, segment).ok_or_else(|| ShiroError::SearchFailed {
            message: format!(
                "EntryPoint segment {} does not overlap a canonical block in {doc_id}",
                segment.id
            ),
        })?;
    let block = &graph.blocks[block_idx];
    let (span_start, span_end) = block_relative_segment_span(block, segment);
    let context_window = if expand && max_blocks > 0 && max_chars > 0 {
        build_context_window(&graph, block_idx, max_blocks, max_chars)
    } else {
        Vec::new()
    };

    Ok(MaterializedEntryPoint {
        block_idx,
        block_kind: block_kind_name(&block.kind),
        span_start,
        span_end,
        context_window,
    })
}

/// Render the exhaustive retrieval trace from evidence saved during search.
pub(crate) fn build_retrieval_trace(detail: &SearchResultDetail) -> RetrievalTrace {
    let mut pipeline = Vec::new();
    let mut stages = Vec::new();
    let mut fusion_contributions = serde_json::Map::new();

    if let Some(rank) = detail.bm25_rank {
        pipeline.push("fts_bm25".to_string());
        stages.push(serde_json::json!({
            "name": "fts_bm25",
            "generation": detail.fts_gen,
            "input_query": &detail.query,
            "this_result": {
                "rank": rank,
                "raw_score": detail.bm25_score.unwrap_or(0.0),
            },
        }));
        fusion_contributions.insert(
            "bm25".to_string(),
            serde_json::json!({
                "rank": rank,
                "rrf_contribution": 1.0 / (RRF_K + rank as f64),
            }),
        );
    }

    if let Some(rank) = detail.vector_rank {
        pipeline.push("vector".to_string());
        stages.push(serde_json::json!({
            "name": "vector",
            "generation": detail.vec_gen,
            "input_query": &detail.query,
            "this_result": {
                "rank": rank,
                "raw_score": detail.vector_score.unwrap_or(0.0),
            },
        }));
        fusion_contributions.insert(
            "vector".to_string(),
            serde_json::json!({
                "rank": rank,
                "rrf_contribution": 1.0 / (RRF_K + rank as f64),
            }),
        );
    }

    if let Some(rank) = detail.reranker_rank {
        pipeline.push("reranker".to_string());
        stages.push(serde_json::json!({
            "name": "reranker",
            "input_query": &detail.query,
            "this_result": {
                "rank": rank,
                "raw_score": detail.reranker_score.unwrap_or(0.0),
            },
        }));
    }

    RetrievalTrace {
        pipeline,
        stages,
        fusion: serde_json::json!({
            "method": "rrf",
            "k": RRF_K as u64,
            "contributions": fusion_contributions,
            "final_score": detail.fused_score.unwrap_or(0.0),
        }),
    }
}

fn validate_entry_point_graph(doc_id: &DocId, graph: &BlockGraph) -> Result<(), ShiroError> {
    if graph.blocks.is_empty() {
        return Err(ShiroError::SearchFailed {
            message: format!("EntryPoint block graph is empty for {doc_id}"),
        });
    }
    let canonical_text_len = graph
        .blocks
        .iter()
        .map(|block| block.span.end())
        .max()
        .unwrap_or(0);
    let violations = graph.validate(canonical_text_len);
    if violations.is_empty() {
        Ok(())
    } else {
        Err(ShiroError::SearchFailed {
            message: format!(
                "EntryPoint block graph is invalid for {doc_id}: {}",
                violations
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        })
    }
}

fn resolve_segment_block(graph: &BlockGraph, segment: &Segment) -> Option<usize> {
    graph
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            let overlap_start = segment.span.start().max(block.span.start());
            let overlap_end = segment.span.end().min(block.span.end());
            (overlap_start < overlap_end).then(|| (index, overlap_end - overlap_start))
        })
        .max_by(|(left_index, left_overlap), (right_index, right_overlap)| {
            left_overlap
                .cmp(right_overlap)
                .then_with(|| right_index.cmp(left_index))
        })
        .map(|(index, _)| index)
}

fn block_relative_segment_span(block: &shiro_core::ir::Block, segment: &Segment) -> (usize, usize) {
    let overlap_start = segment.span.start().max(block.span.start());
    let overlap_end = segment.span.end().min(block.span.end());
    (
        overlap_start - block.span.start(),
        overlap_end - block.span.start(),
    )
}

fn build_context_window(
    graph: &BlockGraph,
    hit_block_idx: usize,
    max_blocks: usize,
    max_chars: usize,
) -> Vec<ContextBlock> {
    let hit_position = graph
        .reading_order
        .iter()
        .position(|index| index.0 == hit_block_idx);
    let Some(hit_position) = hit_position else {
        return Vec::new();
    };

    let mut positions = vec![hit_position];
    let mut total_chars = graph.blocks[hit_block_idx].canonical_text.len();
    let mut distance = 1usize;

    while positions.len() < max_blocks && total_chars < max_chars {
        let candidates = [
            hit_position.checked_sub(distance),
            hit_position.checked_add(distance),
        ];
        let mut added = false;
        for position in candidates.into_iter().flatten() {
            let Some(block_index) = graph.reading_order.get(position).map(|index| index.0) else {
                continue;
            };
            let block = &graph.blocks[block_index];
            let block_chars = block.canonical_text.len();
            if positions.len() < max_blocks && total_chars + block_chars <= max_chars {
                positions.push(position);
                total_chars += block_chars;
                added = true;
            }
        }
        if !added
            && hit_position.checked_sub(distance).is_none()
            && hit_position
                .checked_add(distance)
                .map_or(true, |position| position >= graph.reading_order.len())
        {
            break;
        }
        distance += 1;
        if distance > graph.reading_order.len() {
            break;
        }
    }

    positions.sort_unstable();
    positions
        .into_iter()
        .filter_map(|position| graph.reading_order.get(position))
        .map(|index| {
            let block = &graph.blocks[index.0];
            ContextBlock {
                block_idx: index.0,
                kind: block_kind_name(&block.kind),
                span_start: block.span.start(),
                span_end: block.span.end(),
                text: block.canonical_text.clone(),
            }
        })
        .collect()
}

fn block_kind_name(kind: &shiro_core::ir::BlockKind) -> String {
    format!("{kind:?}").to_uppercase()
}

#[cfg(test)]
mod tests {
    use shiro_core::ir::{Block, BlockIdx, BlockKind};
    use shiro_core::{SegmentId, Span};

    use super::*;

    fn test_graph() -> BlockGraph {
        BlockGraph {
            blocks: vec![
                Block {
                    kind: BlockKind::Heading,
                    span: Span::new(0, 5).expect("heading span"),
                    canonical_text: "Title".to_string(),
                    rendered_text: None,
                },
                Block {
                    kind: BlockKind::Paragraph,
                    span: Span::new(6, 17).expect("paragraph span"),
                    canonical_text: "hello world".to_string(),
                    rendered_text: None,
                },
            ],
            edges: Vec::new(),
            reading_order: vec![BlockIdx(0), BlockIdx(1)],
        }
    }

    #[test]
    fn segment_resolves_to_largest_overlap_then_lowest_index() {
        let doc_id = DocId::from_content(b"Title\nhello world");
        let segment = Segment {
            id: SegmentId::new(&doc_id, 0),
            doc_id,
            index: 0,
            span: Span::new(4, 10).expect("segment span"),
            body: "e\nhell".to_string(),
        };

        let graph = test_graph();
        assert_eq!(resolve_segment_block(&graph, &segment), Some(1));
        assert_eq!(
            block_relative_segment_span(&graph.blocks[1], &segment),
            (0, 4)
        );
    }

    #[test]
    fn context_window_stays_in_reading_order() {
        let context = build_context_window(&test_graph(), 1, 2, 100);
        assert_eq!(context.len(), 2);
        assert_eq!(context[0].block_idx, 0);
        assert_eq!(context[1].block_idx, 1);
    }
}
