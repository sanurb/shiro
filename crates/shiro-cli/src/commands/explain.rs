//! `shiro explain` — thin adapter over shiro-sdk explain.
//!
//! Per ADR-007, output uses block-level position. No segment identifiers.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::{Engine, ExplainInput};

pub fn run(home: &ShiroHome, result_id: &str) -> Result<CmdOutput, ShiroError> {
    let engine = Engine::open(home.clone())?;

    let input = ExplainInput {
        result_id: result_id.to_string(),
    };
    let output = engine.explain(&input)?;

    let retrieval_trace = serde_json::json!({
        "pipeline": output.retrieval_trace.pipeline,
        "stages": output.retrieval_trace.stages,
        "fusion": output.retrieval_trace.fusion,
        "filters_applied": [],
        "expansions_applied": [],
    });

    let result = serde_json::json!({
        "result_id": output.result_id,
        "query": output.query,
        "query_digest": output.query_digest,
        "generations": { "fts": output.fts_generation },
        "doc_id": output.doc_id,
        "block_idx": output.block_idx,
        "block_kind": output.block_kind,
        "span": {
            "start": output.span_start,
            "end": output.span_end,
        },
        "scores": serde_json::Value::Object({
            let mut scores = serde_json::Map::new();
            scores.insert("bm25".to_string(), serde_json::json!({
                "score": output.bm25_score,
                "rank": output.bm25_rank,
            }));
            if let Some(vector_rank) = output.vector_rank {
                scores.insert("vector".to_string(), serde_json::json!({
                    "score": output.vector_score,
                    "rank": vector_rank,
                }));
            }
            scores.insert("fused".to_string(), serde_json::json!({
                "score": output.fused_score,
                "rank": output.fused_rank,
            }));
            scores
        }),
        "retrieval_trace": retrieval_trace,
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple(
                format!("shiro read {} --text", output.doc_id),
                "Read the full document",
            ),
            NextAction::simple("shiro search <query>", "Run another search"),
        ],
    })
}
