//! `shiro search` — thin adapter over shiro-sdk search.
//!
//! Per ADR-007, output uses EntryPoint shape: block-level position
//! and context window. No segment identifiers in public output.

use std::collections::BTreeMap;

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::{SearchFilters, SearchInput};

pub use shiro_sdk::SearchMode;

#[allow(clippy::too_many_arguments)]
pub fn run(
    home: &ShiroHome,
    query: &str,
    mode: SearchMode,
    limit: usize,
    expand: bool,
    max_blocks: usize,
    max_chars: usize,
    rerank: bool,
    filters: SearchFilters,
) -> Result<CmdOutput, ShiroError> {
    let runtime_profile = match (mode, rerank) {
        (SearchMode::Bm25, false) => crate::runtime::RuntimeProfile::Base,
        (SearchMode::Bm25, true) => crate::runtime::RuntimeProfile::RerankOnly,
        (_, false) => crate::runtime::RuntimeProfile::Vector,
        (_, true) => crate::runtime::RuntimeProfile::Full,
    };
    let engine = crate::runtime::open_engine(home, runtime_profile)?;

    let input = SearchInput {
        query: query.to_string(),
        mode,
        limit,
        expand,
        max_blocks,
        max_chars,
        rerank,
        filters,
    };
    let output = engine.search(&input)?;

    // Convert SDK output to JSON envelope.
    let results: Vec<serde_json::Value> = output
        .hits
        .iter()
        .map(|h| {
            let context_window: Vec<serde_json::Value> = h
                .context_window
                .iter()
                .map(|cb| {
                    serde_json::json!({
                        "evidence_handle": cb.evidence_handle,
                        "block_idx": cb.block_idx,
                        "kind": cb.kind,
                        "heading_level": cb.heading_level,
                        "span": { "start": cb.span_start, "end": cb.span_end },
                        "text": cb.text,
                        "source_locators": cb.source_locators,
                    })
                })
                .collect();

            let mut scores = serde_json::Map::new();
            if let Some(bm25_rank) = h.scores.bm25_rank {
                scores.insert(
                    "bm25".to_string(),
                    serde_json::json!({
                        "score": h.scores.bm25_score,
                        "rank": bm25_rank,
                    }),
                );
            }
            if let Some(vector_rank) = h.scores.vector_rank {
                scores.insert(
                    "vector".to_string(),
                    serde_json::json!({
                        "score": h.scores.vector_score,
                        "rank": vector_rank,
                    }),
                );
            }
            scores.insert(
                "fused".to_string(),
                serde_json::json!({
                    "score": h.scores.fused_score,
                    "rank": h.scores.fused_rank,
                }),
            );
            if let Some(reranker_rank) = h.scores.reranker_rank {
                scores.insert(
                    "reranker".to_string(),
                    serde_json::json!({
                        "score": h.scores.reranker_score,
                        "rank": reranker_rank,
                    }),
                );
            }

            serde_json::json!({
                "result_id": h.result_id,
                "evidence_handle": h.evidence_handle,
                "doc_id": h.doc_id,
                "block_idx": h.block_idx,
                "block_kind": h.block_kind,
                "heading_level": h.heading_level,
                "span": { "start": h.span_start, "end": h.span_end },
                "source_locators": h.source_locators,
                "snippet": h.snippet,
                "scores": scores,
                "context_window": context_window,
            })
        })
        .collect();

    let result = serde_json::json!({
        "query": output.query,
        "mode": output.mode,
        "generations": { "fts": output.fts_generation },
        "retrieval_info": {
            "bm25_active": output.retrieval_info.bm25_active,
            "vector_active": output.retrieval_info.vector_active,
            "reranker_active": output.retrieval_info.reranker_active,
            "reranker_model": output.retrieval_info.reranker_model,
        },
        "results": results,
    });

    let mut next_actions = Vec::new();
    if let Some(first) = output.hits.first() {
        let mut params = BTreeMap::new();
        params.insert(
            "result_id".to_string(),
            ParamMeta {
                value: Some(serde_json::json!(first.result_id)),
                default: None,
                description: Some("Result ID from search".to_string()),
            },
        );
        next_actions.push(NextAction::with_params(
            "shiro explain <result_id>",
            "Explain why this result matched",
            params,
        ));
    }
    next_actions.push(NextAction::simple("shiro list", "List all documents"));

    Ok(CmdOutput {
        result,
        next_actions,
    })
}
