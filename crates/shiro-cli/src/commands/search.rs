//! `shiro search` — BM25 / hybrid search over indexed documents.

use std::collections::BTreeMap;

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_store::Store;

/// Search mode — hybrid is the default (falls back to BM25-only until
/// vector backend is implemented).
#[derive(Debug, Clone, Copy)]
pub enum SearchMode {
    Hybrid,
    Bm25,
    Vector,
}

pub fn run(
    home: &ShiroHome,
    query: &str,
    mode: SearchMode,
    limit: usize,
) -> Result<CmdOutput, ShiroError> {
    // Vector mode is not yet implemented.
    if matches!(mode, SearchMode::Vector) {
        return Err(ShiroError::SearchFailed {
            message: "vector search not yet implemented; use --bm25 or --hybrid".to_string(),
        });
    }

    let store = Store::open(&home.db_path())?;
    let fts = FtsIndex::open(&home.tantivy_dir())?;

    let hits = fts.search(query, limit)?;

    // Build result IDs and persist for explain.
    let mut results = Vec::with_capacity(hits.len());
    let mut search_cache = Vec::new();

    for (rank_idx, hit) in hits.iter().enumerate() {
        let result_id = make_result_id(query, &hit.segment_id);

        search_cache.push((
            result_id.clone(),
            shiro_core::DocId::from_stored(&hit.doc_id).map_err(|e| ShiroError::SearchFailed {
                message: e.to_string(),
            })?,
            shiro_core::SegmentId::from_stored(&hit.segment_id).map_err(|e| {
                ShiroError::SearchFailed {
                    message: e.to_string(),
                }
            })?,
            hit.bm25_score,
            rank_idx + 1,
        ));

        let snippet = truncate_snippet(&hit.body, 200);
        results.push(serde_json::json!({
            "result_id": result_id,
            "doc_id": hit.doc_id,
            "segment_id": hit.segment_id,
            "block_id": hit.seg_index,
            "span": { "start": hit.span_start, "end": hit.span_end },
            "snippet": snippet,
            "scores": {
                "bm25": { "score": hit.bm25_score, "rank": hit.bm25_rank },
                "fused": hit.bm25_score,
            },
        }));
    }

    // Persist search results for explain.
    if !search_cache.is_empty() {
        if let Err(e) = store.save_search_results(query, &search_cache) {
            tracing::warn!(error = %e, "failed to cache search results for explain");
        }
    }

    let mode_str = match mode {
        SearchMode::Hybrid => "hybrid",
        SearchMode::Bm25 => "bm25",
        SearchMode::Vector => "vector",
    };

    let result = serde_json::json!({
        "query": query,
        "mode": mode_str,
        "results": results,
    });

    let mut next_actions = Vec::new();
    if let Some(first) = results.first() {
        let rid = first["result_id"].as_str().unwrap_or("");
        let mut params = BTreeMap::new();
        params.insert(
            "result_id".to_string(),
            ParamMeta {
                value: Some(serde_json::json!(rid)),
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

/// Generate a deterministic result_id from query + segment_id.
fn make_result_id(query: &str, segment_id: &str) -> String {
    let input = format!("{query}:{segment_id}");
    let hash = blake3::hash(input.as_bytes());
    format!("res_{}", &hash.to_hex()[..16])
}

/// Truncate a snippet to max_chars, breaking at word boundaries.
fn truncate_snippet(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    // Find the last space before max_chars.
    let truncated = &text[..max_chars];
    match truncated.rfind(' ') {
        Some(pos) => format!("{}...", &truncated[..pos]),
        None => format!("{truncated}..."),
    }
}
