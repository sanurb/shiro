//! `shiro search` — thin adapter over shiro-sdk search.

use std::collections::BTreeMap;

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::{Engine, SearchInput};

pub use shiro_sdk::SearchMode;

pub fn run(
    home: &ShiroHome,
    query: &str,
    mode: SearchMode,
    limit: usize,
    expand: bool,
    max_blocks: usize,
    max_chars: usize,
) -> Result<CmdOutput, ShiroError> {
    let engine = Engine::open(home.clone())?;

    let input = SearchInput {
        query: query.to_string(),
        mode,
        limit,
        expand,
        max_blocks,
        max_chars,
    };
    let output = engine.search(&input)?;

    // Convert SDK output to JSON envelope.
    let results: Vec<serde_json::Value> = output
        .hits
        .iter()
        .map(|h| {
            serde_json::json!({
                "result_id": h.result_id,
                "doc_id": h.doc_id,
                "segment_id": h.segment_id,
                "block_id": h.block_id,
                "span": { "start": h.span_start, "end": h.span_end },
                "snippet": h.snippet,
                "scores": {
                    "bm25": h.scores.bm25_rank.map(|_| serde_json::json!({
                        "score": h.scores.bm25_score, "rank": h.scores.bm25_rank
                    })),
                    "fused": h.scores.fused_score,
                },
                "expansion": {
                    "expanded": h.expansion.expanded,
                    "blocks": h.expansion.blocks,
                    "chars": h.expansion.chars,
                },
            })
        })
        .collect();

    let result = serde_json::json!({
        "query": output.query,
        "mode": output.mode,
        "generations": { "fts": output.fts_generation },
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
