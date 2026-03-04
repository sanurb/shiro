//! `shiro explain` — scoring breakdown for a search result.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_store::Store;

pub fn run(home: &ShiroHome, result_id: &str) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path())?;

    let (query, doc_id, segment_id, bm25_score, bm25_rank) = store.get_search_result(result_id)?;

    // Load segment details.
    let segments = store.get_segments(&doc_id)?;
    let segment = segments
        .iter()
        .find(|s| s.id == segment_id)
        .ok_or_else(|| ShiroError::NotFoundMsg {
            message: format!("segment {} not in store", segment_id),
        })?;

    let result = serde_json::json!({
        "result_id": result_id,
        "query": query,
        "doc_id": doc_id.as_str(),
        "segment_id": segment_id.as_str(),
        "block_id": segment.index,
        "span": {
            "start": segment.span.start(),
            "end": segment.span.end(),
        },
        "scores": {
            "bm25": {
                "score": bm25_score,
                "rank": bm25_rank,
            },
            "fused": bm25_score,
            // TODO: vector scores when vector backend is implemented.
            // TODO: taxonomy_boost when taxonomy is implemented.
        },
        "expansion": {
            // TODO: context expansion is not yet implemented.
            "rules_fired": [],
            "included_block_ids": [segment.index],
            "budgets": {
                "max_blocks": 12,
                "max_chars": 8000,
                "used_blocks": 1,
                "used_chars": segment.body.len(),
            },
        },
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple(
                format!("shiro read {} --text", doc_id),
                "Read the full document",
            ),
            NextAction::simple("shiro search <query>", "Run another search"),
        ],
    })
}
