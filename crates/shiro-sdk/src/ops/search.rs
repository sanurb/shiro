//! Search operation — BM25 / hybrid search over indexed documents.
//!
//! Per ADR-007, the public retrieval result is an **EntryPoint**: the best
//! position in a document to begin reading, with a context window assembled
//! from the persisted BlockGraph (ADR-006). Segment identifiers are internal
//! and never appear in the SDK output.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use shiro_core::ports::{Embedder, Reranker, VectorIndex};
use shiro_core::{DocId, Segment, SegmentId, ShiroError, Span};
use shiro_index::FtsIndex;
use shiro_store::Store;

use crate::fusion::{reciprocal_rank_fusion, RankedHit};
use crate::retrieval_result::materialize_entry_point;
pub use crate::retrieval_result::ContextBlock;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Search mode — hybrid is the default. Falls back to BM25-only when no
/// vector backend is configured.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
pub enum SearchMode {
    Hybrid,
    Bm25,
    Vector,
}

impl SearchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hybrid => "hybrid",
            Self::Bm25 => "bm25",
            Self::Vector => "vector",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchInput {
    pub query: String,
    pub mode: SearchMode,
    pub limit: usize,
    pub expand: bool,
    pub max_blocks: usize,
    pub max_chars: usize,
    /// Enable post-fusion reranking when a reranker is available.
    pub rerank: bool,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchScores {
    pub bm25_score: Option<f32>,
    pub bm25_rank: Option<usize>,
    pub vector_score: Option<f32>,
    pub vector_rank: Option<usize>,
    pub fused_score: f64,
    pub fused_rank: usize,
    pub reranker_score: Option<f32>,
    pub reranker_rank: Option<usize>,
}

/// The public retrieval result — per ADR-007, this is the single type
/// that consumers receive from search. No segment identifiers are exposed.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchHit {
    pub result_id: String,
    pub doc_id: String,
    pub block_idx: usize,
    pub block_kind: String,
    pub span_start: usize,
    pub span_end: usize,
    pub snippet: String,
    pub scores: SearchScores,
    pub context_window: Vec<ContextBlock>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchOutput {
    pub query: String,
    pub mode: String,
    pub fts_generation: u64,
    pub hits: Vec<SearchHit>,
    /// Summary of which retrieval sources and stages were active.
    pub retrieval_info: RetrievalInfo,
}

/// Machine-readable summary of what retrieval components were active.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RetrievalInfo {
    pub bm25_active: bool,
    pub vector_active: bool,
    pub reranker_active: bool,
    pub reranker_model: Option<String>,
}

// ---------------------------------------------------------------------------
// Execute
// ---------------------------------------------------------------------------

/// Run a search query and return typed, deterministically-ranked results.
pub fn execute(
    store: &Store,
    fts: &FtsIndex,
    embedder: Option<&dyn Embedder>,
    vector_index: Option<&dyn VectorIndex>,
    reranker: Option<&dyn Reranker>,
    input: &SearchInput,
) -> Result<SearchOutput, ShiroError> {
    // Empty query -> empty output.
    if input.query.is_empty() {
        return Ok(SearchOutput {
            query: String::new(),
            mode: input.mode.as_str().to_string(),
            fts_generation: 0,
            hits: Vec::new(),
            retrieval_info: RetrievalInfo {
                bm25_active: false,
                vector_active: false,
                reranker_active: false,
                reranker_model: None,
            },
        });
    }

    // -- Determine active sources based on mode and availability --
    let vector_pair_available = embedder.is_some() && vector_index.is_some();
    let vector_pair_incomplete = embedder.is_some() != vector_index.is_some();
    if !matches!(input.mode, SearchMode::Bm25) && vector_pair_incomplete {
        return Err(ShiroError::SearchFailed {
            message: "Vector search configuration incomplete: embedder and vector index must be attached together"
                .to_string(),
        });
    }
    if matches!(input.mode, SearchMode::Vector) && !vector_pair_available {
        return Err(ShiroError::SearchFailed {
            message:
                "Vector search requested but no compatible embedder and vector index are configured"
                    .to_string(),
        });
    }

    let use_bm25 = !matches!(input.mode, SearchMode::Vector);
    let use_vector = !matches!(input.mode, SearchMode::Bm25) && vector_pair_available;
    let use_reranker = input.rerank && reranker.is_some();

    if use_vector {
        let active = embedder
            .ok_or_else(|| ShiroError::EmbedFail {
                message: "Vector compatibility check missing embedder".to_string(),
            })?
            .fingerprint();
        let stored = vector_index
            .ok_or_else(|| ShiroError::SearchFailed {
                message: "Vector compatibility check missing index".to_string(),
            })?
            .embedding_fingerprint()?;
        match stored {
            Some(stored) if stored.fingerprint_hash == active.fingerprint_hash => {}
            Some(stored) => {
                return Err(ShiroError::FingerprintMismatch {
                    message: format!(
                        "Embedding fingerprint mismatch: stored={}/{}({}d), active={}/{}({}d). Rebuild the vector index.",
                        stored.provider,
                        stored.model,
                        stored.dimensions,
                        active.provider,
                        active.model,
                        active.dimensions,
                    ),
                });
            }
            None => {
                return Err(ShiroError::FingerprintMismatch {
                    message: "Vector index has no embedding fingerprint; rebuild the vector index"
                        .to_string(),
                });
            }
        }
    }

    // -- FTS results --
    let bm25_hits = if use_bm25 {
        fts.search(&input.query, input.limit)?
    } else {
        Vec::new()
    };

    // -- Vector results --
    let vector_hits = if use_vector {
        let emb = embedder.ok_or_else(|| ShiroError::EmbedFail {
            message: "embedder required for vector search".to_string(),
        })?;
        let vi = vector_index.ok_or_else(|| ShiroError::SearchFailed {
            message: "vector index required for vector search".to_string(),
        })?;
        let query_vec = emb.embed(&input.query)?;
        vi.search(&query_vec, input.limit)?
    } else {
        Vec::new()
    };

    // -- Generation tracking --
    let fts_gen = store.active_generation("fts")?.as_u64();
    let vector_gen = if use_vector {
        store.active_generation("vector")?.as_u64()
    } else {
        0
    };

    // -- Build RRF ranked list --
    let mut ranked_map: HashMap<String, RankedHit> = HashMap::new();

    for h in &bm25_hits {
        let entry = ranked_map
            .entry(h.segment_id.clone())
            .or_insert_with(|| RankedHit {
                id: h.segment_id.clone(),
                bm25_rank: None,
                vector_rank: None,
            });
        entry.bm25_rank = Some(h.bm25_rank);
    }

    for (rank, vh) in vector_hits.iter().enumerate() {
        let seg_id = vh.segment_id.as_str().to_string();
        let entry = ranked_map
            .entry(seg_id.clone())
            .or_insert_with(|| RankedHit {
                id: seg_id,
                bm25_rank: None,
                vector_rank: None,
            });
        entry.vector_rank = Some(rank + 1);
    }

    let ranked_vec: Vec<RankedHit> = ranked_map.into_values().collect();
    let fused = reciprocal_rank_fusion(&ranked_vec);

    let rrf_lookup: HashMap<&str, (f64, usize)> = fused
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.as_str(), (f.rrf_score, i + 1)))
        .collect();

    // -- BM25 score/rank lookup --
    let bm25_lookup: HashMap<String, (f32, usize)> = bm25_hits
        .iter()
        .map(|h| (h.segment_id.clone(), (h.bm25_score, h.bm25_rank)))
        .collect();

    // -- Vector score lookup --
    let vector_lookup: HashMap<String, (f32, usize)> = vector_hits
        .iter()
        .enumerate()
        .map(|(i, vh)| (vh.segment_id.as_str().to_string(), (vh.score, i + 1)))
        .collect();

    // -- FTS body map --
    let fts_body_map: HashMap<String, &shiro_index::FtsHit> = bm25_hits
        .iter()
        .map(|h| (h.segment_id.clone(), h))
        .collect();

    // -- Query digest --
    let query_digest = {
        let hash = blake3::hash(input.query.as_bytes());
        hash.to_hex()[..16].to_string()
    };

    // -- Build output in fused rank order --
    let mut hits = Vec::with_capacity(fused.len().min(input.limit));
    let mut search_cache: Vec<shiro_store::SearchResultRow> = Vec::new();
    // Collect segment bodies for reranking
    let mut hit_segment_bodies: Vec<String> = Vec::new();

    for fh in fused.iter().take(input.limit) {
        let seg_id_str = &fh.id;

        // Resolve doc_id — from FTS hit if available, else from store.
        let doc_id_str = if let Some(fts_hit) = fts_body_map.get(seg_id_str) {
            fts_hit.doc_id.clone()
        } else {
            match load_doc_id_for_segment(store, seg_id_str) {
                Ok(did) => did,
                Err(e) => {
                    tracing::warn!(
                        segment_id = seg_id_str,
                        error = %e,
                        "skipping result: can't resolve doc_id"
                    );
                    continue;
                }
            }
        };

        let result_id = make_result_id(&input.query, seg_id_str);

        let bm25_info = bm25_lookup.get(seg_id_str);
        let vector_info = vector_lookup.get(seg_id_str);
        let (fused_score, fused_rank) = rrf_lookup
            .get(seg_id_str.as_str())
            .copied()
            .unwrap_or((0.0, 0));

        let doc_id = DocId::from_stored(&doc_id_str).map_err(|e| ShiroError::SearchFailed {
            message: e.to_string(),
        })?;
        let segment = if let Some(fts_hit) = fts_body_map.get(seg_id_str) {
            let segment_id =
                SegmentId::from_stored(seg_id_str).map_err(|e| ShiroError::SearchFailed {
                    message: e.to_string(),
                })?;
            Segment {
                id: segment_id,
                doc_id: doc_id.clone(),
                index: fts_hit.seg_index,
                span: Span::new(fts_hit.span_start, fts_hit.span_end).map_err(|e| {
                    ShiroError::SearchFailed {
                        message: format!("Invalid indexed segment span for {seg_id_str}: {e}"),
                    }
                })?,
                body: fts_hit.body.clone(),
            }
        } else {
            match load_segment_info(store, &doc_id, seg_id_str) {
                Ok(segment) => segment,
                Err(e) => {
                    tracing::warn!(
                        segment_id = seg_id_str,
                        error = %e,
                        "skipping result: segment data unavailable"
                    );
                    continue;
                }
            }
        };

        let snippet = truncate_snippet(&segment.body, 200);
        let entry_point = materialize_entry_point(
            store,
            &doc_id,
            &segment,
            input.expand,
            input.max_blocks,
            input.max_chars,
        )?;

        // Persist the internal segment evidence and the canonical public position.
        search_cache.push(shiro_store::SearchResultRow {
            result_id: result_id.clone(),
            doc_id: doc_id.clone(),
            segment_id: segment.id.clone(),
            bm25_score: bm25_info.map(|i| i.0),
            bm25_rank: bm25_info.map(|i| i.1),
            vector_score: vector_info.map(|i| i.0),
            vector_rank: vector_info.map(|i| i.1),
            fused_score: Some(fused_score as f32),
            fused_rank: Some(fused_rank),
            reranker_score: None,
            reranker_rank: None,
            block_idx: entry_point.block_idx,
            block_kind: entry_point.block_kind.clone(),
            span_start: entry_point.span_start,
            span_end: entry_point.span_end,
        });

        hit_segment_bodies.push(segment.body.clone());

        hits.push(SearchHit {
            result_id,
            doc_id: doc_id_str,
            block_idx: entry_point.block_idx,
            block_kind: entry_point.block_kind,
            span_start: entry_point.span_start,
            span_end: entry_point.span_end,
            snippet,
            scores: SearchScores {
                bm25_score: bm25_info.map(|i| i.0),
                bm25_rank: bm25_info.map(|i| i.1),
                vector_score: vector_info.map(|i| i.0),
                vector_rank: vector_info.map(|i| i.1),
                fused_score,
                fused_rank,
                reranker_score: None,
                reranker_rank: None,
            },
            context_window: entry_point.context_window,
        });
    }

    // -- Post-fusion reranking --
    let reranker_model_name = if use_reranker && !hits.is_empty() {
        let rr = reranker.ok_or_else(|| ShiroError::SearchFailed {
            message: "reranker expected but missing".to_string(),
        })?;
        let model_name = rr.model_name().to_string();
        let doc_texts: Vec<&str> = hit_segment_bodies.iter().map(|s| s.as_str()).collect();
        let top_n = hits.len();

        match rr.rerank(&input.query, &doc_texts, top_n) {
            Ok(rerank_results) => {
                // Build index→(score, rank) map from reranker output
                let mut rerank_map: HashMap<usize, (f32, usize)> = HashMap::new();
                for (rank, rr_result) in rerank_results.iter().enumerate() {
                    rerank_map.insert(rr_result.index, (rr_result.score, rank + 1));
                }

                // Apply reranker scores to hits and search_cache
                for (i, hit) in hits.iter_mut().enumerate() {
                    if let Some(&(score, rank)) = rerank_map.get(&i) {
                        hit.scores.reranker_score = Some(score);
                        hit.scores.reranker_rank = Some(rank);
                        // Propagate to search_cache for explain persistence
                        if i < search_cache.len() {
                            search_cache[i].reranker_score = Some(score);
                            search_cache[i].reranker_rank = Some(rank);
                        }
                    }
                }

                // Re-sort hits by reranker rank (ascending = best first)
                hits.sort_by(|a, b| {
                    let ra = a.scores.reranker_rank.unwrap_or(usize::MAX);
                    let rb = b.scores.reranker_rank.unwrap_or(usize::MAX);
                    ra.cmp(&rb)
                });

                Some(model_name)
            }
            Err(e) => {
                // Reranking failure is non-fatal — fall back to RRF order
                tracing::warn!(error = %e, "reranking failed, falling back to RRF order");
                None
            }
        }
    } else {
        None
    };

    // Persist search results for explain.
    if !search_cache.is_empty() {
        if let Err(e) = store.save_search_results(
            &input.query,
            &query_digest,
            fts_gen,
            vector_gen,
            &search_cache,
        ) {
            tracing::warn!(error = %e, "failed to cache search results for explain");
        }
    }

    let retrieval_info = RetrievalInfo {
        bm25_active: use_bm25 && !bm25_hits.is_empty(),
        vector_active: use_vector && !vector_hits.is_empty(),
        reranker_active: reranker_model_name.is_some(),
        reranker_model: reranker_model_name,
    };

    Ok(SearchOutput {
        query: input.query.clone(),
        mode: input.mode.as_str().to_string(),
        fts_generation: fts_gen,
        hits,
        retrieval_info,
    })
}

// ---------------------------------------------------------------------------
// Helpers (pub(crate) for reuse within SDK)
// ---------------------------------------------------------------------------

/// Generate a deterministic result_id from query + segment_id (blake3).
pub(crate) fn make_result_id(query: &str, segment_id: &str) -> String {
    let input = format!("{query}:{segment_id}");
    let hash = blake3::hash(input.as_bytes());
    format!("res_{}", &hash.to_hex()[..16])
}

/// Truncate a snippet to `max_chars`, breaking at word boundaries.
pub(crate) fn truncate_snippet(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    // Find the last char boundary at or before max_chars.
    let mut end = max_chars;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = &text[..end];
    match truncated.rfind(' ') {
        Some(pos) => format!("{}...", &truncated[..pos]),
        None => format!("{truncated}..."),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Load the doc_id for a segment from the store.
fn load_doc_id_for_segment(store: &Store, segment_id: &str) -> Result<String, ShiroError> {
    let seg_id = SegmentId::from_stored(segment_id).map_err(|e| ShiroError::SearchFailed {
        message: e.to_string(),
    })?;
    store
        .segment_doc_id(&seg_id)
        .map_err(|e| ShiroError::SearchFailed {
            message: format!("cannot find doc for segment {segment_id}: {e}"),
        })
}

/// Load segment body and metadata from the store.
fn load_segment_info(
    store: &Store,
    doc_id: &DocId,
    segment_id: &str,
) -> Result<Segment, ShiroError> {
    let segments = store.get_segments(doc_id)?;
    segments
        .into_iter()
        .find(|segment| segment.id.as_str() == segment_id)
        .ok_or_else(|| ShiroError::SearchFailed {
            message: format!("Segment data unavailable for {segment_id}"),
        })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii() {
        assert_eq!(truncate_snippet("short", 100), "short");
        assert_eq!(truncate_snippet("hello world foo", 11), "hello...");
    }

    #[test]
    fn truncate_unicode_safe() {
        // 4-byte emoji: slicing at byte 5 would be mid-character.
        let text = "a \u{1F600} bcdef ghijk"; // 'a ' + 4-byte emoji + ' bcdef ghijk'
        let result = truncate_snippet(text, 5);
        // Must not panic. Should back up to char boundary.
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_no_space() {
        assert_eq!(truncate_snippet("abcdefghij", 5), "abcde...");
    }

    #[test]
    fn truncate_exact_boundary() {
        assert_eq!(truncate_snippet("12345", 5), "12345");
    }

    #[test]
    fn make_result_id_deterministic() {
        let id1 = make_result_id("hello", "seg_abc");
        let id2 = make_result_id("hello", "seg_abc");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("res_"));
        assert_eq!(id1.len(), 4 + 16); // "res_" + 16 hex chars
    }

    #[test]
    fn make_result_id_differs_for_different_inputs() {
        let id1 = make_result_id("hello", "seg_abc");
        let id2 = make_result_id("world", "seg_abc");
        assert_ne!(id1, id2);
    }

    #[test]
    fn search_mode_str() {
        assert_eq!(SearchMode::Hybrid.as_str(), "hybrid");
        assert_eq!(SearchMode::Bm25.as_str(), "bm25");
        assert_eq!(SearchMode::Vector.as_str(), "vector");
    }

    #[test]
    fn incompatible_vector_fingerprint_blocks_hybrid_but_not_bm25() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap();
        let store = Store::open(&root.join("shiro.db")).unwrap();
        let fts = FtsIndex::open(&root.join("tantivy")).unwrap();
        let embedder = shiro_embed::DeterministicStubEmbedder::new(4);
        let incompatible = shiro_core::EmbeddingFingerprint::new(
            "stub".to_string(),
            "different-model".to_string(),
            4,
            "l2".to_string(),
            "none".to_string(),
            "full_segment".to_string(),
        );
        let vector_index =
            shiro_index::FlatIndex::open_compatible(4, root.join("vector.jsonl"), &incompatible)
                .unwrap();
        let mut input = SearchInput {
            query: "query".to_string(),
            mode: SearchMode::Hybrid,
            limit: 10,
            expand: false,
            max_blocks: 12,
            max_chars: 8000,
            rerank: false,
        };

        let error = execute(
            &store,
            &fts,
            Some(&embedder),
            Some(&vector_index),
            None,
            &input,
        )
        .unwrap_err();
        assert!(matches!(error, ShiroError::FingerprintMismatch { .. }));

        input.mode = SearchMode::Bm25;
        let output = execute(
            &store,
            &fts,
            Some(&embedder),
            Some(&vector_index),
            None,
            &input,
        )
        .unwrap();
        assert!(output.hits.is_empty());
    }
}
