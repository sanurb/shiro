//! `shiro explain` — thin adapter over shiro-sdk explain.

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
        "segment_id": output.segment_id,
        "block_id": output.block_id,
        "span": {
            "start": output.span_start,
            "end": output.span_end,
        },
        "scores": {
            "bm25": {
                "score": output.bm25_score,
                "rank": output.bm25_rank,
            },
            "fused": {
                "score": output.fused_score,
                "rank": output.fused_rank,
            },
        },
        "retrieval_trace": retrieval_trace,
        "expansion": {
            "rules_fired": [],
            "included_block_ids": [output.block_id],
            "budgets": {
                "max_blocks": 12,
                "max_chars": 8000,
                "used_blocks": 1,
            },
        },
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
