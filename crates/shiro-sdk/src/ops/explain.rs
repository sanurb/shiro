//! `explain` — scoring breakdown for a search result.
//!
//! Per ADR-007, the public output uses block-level position, not segment
//! identifiers. Segment resolution happens internally.

use serde::{Deserialize, Serialize};
use shiro_core::ShiroError;
use shiro_store::Store;

use crate::retrieval_result::build_retrieval_trace;
pub use crate::retrieval_result::RetrievalTrace;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExplainInput {
    pub result_id: String,
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
    pub vector_score: Option<f32>,
    pub vector_rank: Option<usize>,
    pub reranker_score: Option<f32>,
    pub reranker_rank: Option<usize>,
    pub fused_score: f32,
    pub fused_rank: usize,
    pub retrieval_trace: RetrievalTrace,
}

pub fn execute(store: &Store, input: &ExplainInput) -> Result<ExplainOutput, ShiroError> {
    let detail = store.get_search_result(&input.result_id)?;

    let bm25_rank = detail.bm25_rank.unwrap_or(0);
    let bm25_score = detail.bm25_score.unwrap_or(0.0);
    let vector_score = detail.vector_score;
    let vector_rank = detail.vector_rank;
    let reranker_score = detail.reranker_score;
    let reranker_rank = detail.reranker_rank;
    let fused_score = detail.fused_score.unwrap_or(0.0);
    let fused_rank = detail.fused_rank.unwrap_or(0);
    let fts_gen = detail.fts_gen.unwrap_or(0);
    let query_digest = detail.query_digest.clone().unwrap_or_default();
    let retrieval_trace = build_retrieval_trace(&detail);

    Ok(ExplainOutput {
        result_id: input.result_id.clone(),
        query: detail.query,
        query_digest,
        fts_generation: fts_gen,
        doc_id: detail.doc_id.as_str().to_string(),
        block_idx: detail.block_idx,
        block_kind: detail.block_kind,
        span_start: detail.span_start,
        span_end: detail.span_end,
        bm25_score,
        bm25_rank,
        vector_score,
        vector_rank,
        reranker_score,
        reranker_rank,
        fused_score,
        fused_rank,
        retrieval_trace,
    })
}
