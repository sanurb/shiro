//! `explain` — scoring breakdown for a search result.
//!
//! Per ADR-007, the public output uses block-level position, not segment
//! identifiers. Segment resolution happens internally.

use serde::{Deserialize, Serialize};
use shiro_core::ShiroError;
use shiro_store::Store;

use crate::RRF_K;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExplainInput {
    pub result_id: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RetrievalTrace {
    pub pipeline: Vec<String>,
    pub stages: Vec<serde_json::Value>,
    pub fusion: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExplainOutput {
    pub result_id: String,
    pub query: String,
    pub query_digest: String,
    pub fts_generation: u64,
    pub doc_id: String,
    pub block_idx: usize,
    pub block_kind: String,
    pub span_start: usize,
    pub span_end: usize,
    pub bm25_score: f32,
    pub bm25_rank: usize,
    pub fused_score: f32,
    pub fused_rank: usize,
    pub retrieval_trace: RetrievalTrace,
}

pub fn execute(store: &Store, input: &ExplainInput) -> Result<ExplainOutput, ShiroError> {
    let detail = store.get_search_result(&input.result_id)?;

    // Load segment to get span info.
    let segments = store.get_segments(&detail.doc_id)?;
    let segment = segments
        .iter()
        .find(|s| s.id == detail.segment_id)
        .ok_or_else(|| ShiroError::NotFoundMsg {
            message: format!("segment {} not in store", detail.segment_id),
        })?;

    // Resolve segment to block via BlockGraph (ADR-007).
    let graph = store.get_block_graph(&detail.doc_id)?;
    let (block_idx, block_kind) = if graph.blocks.is_empty() {
        (segment.index, "PARAGRAPH".to_string())
    } else {
        let seg_start = segment.span.start();
        let seg_end = segment.span.end();
        let mut best_idx = 0;
        let mut best_overlap: usize = 0;
        for (i, block) in graph.blocks.iter().enumerate() {
            let b_start = block.span.start();
            let b_end = block.span.end();
            let overlap_start = seg_start.max(b_start);
            let overlap_end = seg_end.min(b_end);
            if overlap_start < overlap_end {
                let overlap = overlap_end - overlap_start;
                if overlap > best_overlap {
                    best_overlap = overlap;
                    best_idx = i;
                }
            }
        }
        let kind_str = format!("{:?}", graph.blocks[best_idx].kind).to_uppercase();
        (best_idx, kind_str)
    };

    let bm25_rank = detail.bm25_rank.unwrap_or(0);
    let bm25_score = detail.bm25_score.unwrap_or(0.0);
    let fused_score = detail.fused_score.unwrap_or(0.0);
    let fused_rank = detail.fused_rank.unwrap_or(0);
    let fts_gen = detail.fts_gen.unwrap_or(0);
    let query_digest = detail.query_digest.clone().unwrap_or_default();

    // Build retrieval trace.
    let mut pipeline = Vec::new();
    let mut stages = Vec::new();
    let mut contributions = serde_json::Map::new();

    if detail.bm25_rank.is_some() {
        pipeline.push("fts_bm25".to_string());
        let bm25_rrf = 1.0 / (RRF_K + bm25_rank as f64);
        stages.push(serde_json::json!({
            "name": "fts_bm25",
            "input_query": &detail.query,
            "this_result": {
                "rank": bm25_rank,
                "raw_score": bm25_score,
            },
        }));
        contributions.insert(
            "bm25".to_string(),
            serde_json::json!({
                "rank": bm25_rank,
                "rrf_contribution": bm25_rrf,
            }),
        );
    }

    let fusion = serde_json::json!({
        "method": "rrf",
        "k": RRF_K as u64,
        "contributions": contributions,
        "final_score": fused_score,
    });

    let retrieval_trace = RetrievalTrace {
        pipeline,
        stages,
        fusion,
    };

    Ok(ExplainOutput {
        result_id: input.result_id.clone(),
        query: detail.query,
        query_digest,
        fts_generation: fts_gen,
        doc_id: detail.doc_id.as_str().to_string(),
        block_idx,
        block_kind,
        span_start: segment.span.start(),
        span_end: segment.span.end(),
        bm25_score,
        bm25_rank,
        fused_score,
        fused_rank,
        retrieval_trace,
    })
}
