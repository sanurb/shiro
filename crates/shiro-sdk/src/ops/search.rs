//! Search operation — BM25 / hybrid search over indexed documents.
//!
//! Per ADR-007, the public retrieval result is an **EntryPoint**: the best
//! position in a document to begin reading, with a context window assembled
//! from the persisted BlockGraph (ADR-006). Segment identifiers are internal
//! and never appear in the SDK output.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use shiro_core::ports::{Embedder, Reranker, VectorIndex};
use shiro_core::{DocId, RunId, Segment, SegmentId, ShiroError};
use shiro_index::FtsIndex;
use shiro_store::Store;

use crate::fusion::{reciprocal_rank_fusion, RankedHit};
use crate::retrieval_policy::ResolvedRetrievalPolicy;
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

/// Typed retrieval filters. Values are ORed within each field and fields are ANDed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct SearchFilters {
    pub tags: Vec<String>,
    pub concept_ids: Vec<String>,
    pub document_ids: Vec<String>,
}

impl SearchFilters {
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.concept_ids.is_empty() && self.document_ids.is_empty()
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
    #[serde(default)]
    pub filters: SearchFilters,
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
    pub evidence_handle: shiro_core::EvidenceHandleId,
    pub doc_id: String,
    pub block_idx: usize,
    pub block_kind: String,
    pub heading_level: Option<u32>,
    pub span_start: usize,
    pub span_end: usize,
    pub source_locators: Vec<shiro_core::SourceLocator>,
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

/// Internal fused candidate retained until final entry-point materialization.
struct SearchCandidate {
    result_id: String,
    doc_id: DocId,
    segment: Segment,
    scores: SearchScores,
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

    // -- Resolve the exact active publication before selecting sources. --
    let active_fts_generation = store.active_generation("fts")?.as_u64();
    if fts.gen_id() != active_fts_generation {
        return Err(ShiroError::SearchFailed {
            message: format!(
                "FTS handle generation {} is stale; active generation is {}. Reopen the engine.",
                fts.gen_id(),
                active_fts_generation
            ),
        });
    }
    let active_vector_generation = store.active_generation("vector")?.as_u64();
    let vector_is_published = store
        .active_corpus_manifest()?
        .map(|manifest| manifest.vector_generation.is_some())
        .unwrap_or(true);
    let vector_generation_matches = vector_index
        .map(|index| index.generation_id() == active_vector_generation)
        .unwrap_or(false);

    // -- Determine active sources based on mode and publication availability --
    let vector_pair_available = embedder.is_some()
        && vector_index.is_some()
        && vector_is_published
        && vector_generation_matches;
    let vector_pair_incomplete = embedder.is_some() != vector_index.is_some();
    if !matches!(input.mode, SearchMode::Bm25) && vector_pair_incomplete {
        return Err(ShiroError::SearchFailed {
            message: "Vector search configuration incomplete: embedder and vector index must be attached together"
                .to_string(),
        });
    }
    let policy = ResolvedRetrievalPolicy::resolve(store, input, vector_pair_available, reranker)?;

    if policy.use_vector {
        let active = crate::retrieval_embedding_fingerprint(
            &embedder
                .ok_or_else(|| ShiroError::EmbedFail {
                    message: "Vector compatibility check missing embedder".to_string(),
                })?
                .fingerprint(),
        );
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
    let bm25_hits = if policy.use_bm25 {
        fts.search_in_documents(
            &input.query,
            policy.source_candidate_limit,
            policy.scope.eligible_document_ids(),
        )?
    } else {
        Vec::new()
    };

    // -- Vector results --
    let vector_hits = if policy.use_vector {
        let emb = embedder.ok_or_else(|| ShiroError::EmbedFail {
            message: "embedder required for vector search".to_string(),
        })?;
        let vi = vector_index.ok_or_else(|| ShiroError::SearchFailed {
            message: "vector index required for vector search".to_string(),
        })?;
        if policy.source_candidate_limit == 0 {
            Vec::new()
        } else {
            let query_vec = emb.embed(&input.query)?;
            // VectorIndex has no metadata-filter interface yet. Exhaust the current
            // source, apply the authoritative scope, then truncate so selective
            // scopes cannot silently consume top-k with ineligible candidates.
            let mut scoped_hits = vi.search(&query_vec, vi.count()?)?;
            scoped_hits.retain(|hit| policy.scope.contains_segment(&hit.segment_id));
            scoped_hits.truncate(policy.source_candidate_limit);
            scoped_hits
        }
    } else {
        Vec::new()
    };

    // -- Generation tracking --
    let fts_gen = active_fts_generation;
    let vector_gen = if policy.use_vector {
        active_vector_generation
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

    // -- Immutable retrieval snapshot identity and query digest --
    let search_snapshot_id = RunId::generate();
    let query_digest = {
        let hash = blake3::hash(input.query.as_bytes());
        hash.to_hex()[..16].to_string()
    };

    // -- Build the bounded candidate pool in fused rank order --
    let mut candidates = Vec::with_capacity(fused.len().min(policy.source_candidate_limit));

    for fh in fused.iter().take(policy.source_candidate_limit) {
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

        let bm25_info = bm25_lookup.get(seg_id_str);
        let vector_info = vector_lookup.get(seg_id_str);
        let (fused_score, fused_rank) = rrf_lookup
            .get(seg_id_str.as_str())
            .copied()
            .unwrap_or((0.0, 0));

        let doc_id = DocId::from_stored(&doc_id_str).map_err(|e| ShiroError::SearchFailed {
            message: e.to_string(),
        })?;
        let segment = match load_segment_info(store, &doc_id, seg_id_str) {
            Ok(segment) => segment,
            Err(e) => {
                tracing::warn!(
                    segment_id = seg_id_str,
                    error = %e,
                    "skipping result: segment data unavailable"
                );
                continue;
            }
        };

        if !policy.scope.contains(&doc_id, &segment.id) {
            continue;
        }

        candidates.push(SearchCandidate {
            result_id: make_result_id(&search_snapshot_id, seg_id_str),
            doc_id,
            segment,
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
        });
    }

    // -- Post-fusion reranking --
    let reranker_model_name = if policy.use_reranker && !candidates.is_empty() {
        let rr = reranker.ok_or_else(|| ShiroError::SearchFailed {
            message: "reranker expected but missing".to_string(),
        })?;
        let model_name = rr.model_name().to_string();
        let rerank_count = policy.rerank_candidate_limit.min(candidates.len());
        let doc_texts: Vec<&str> = candidates
            .iter()
            .take(rerank_count)
            .map(|candidate| candidate.segment.retrieval_text.as_str())
            .collect();

        match rr.rerank(&input.query, &doc_texts, rerank_count) {
            Ok(rerank_results) => {
                let mut rerank_map: HashMap<usize, (f32, usize)> = HashMap::new();
                for (rank, result) in rerank_results.iter().enumerate() {
                    rerank_map.insert(result.index, (result.score, rank + 1));
                }

                for (index, candidate) in candidates.iter_mut().enumerate().take(rerank_count) {
                    if let Some(&(score, rank)) = rerank_map.get(&index) {
                        candidate.scores.reranker_score = Some(score);
                        candidate.scores.reranker_rank = Some(rank);
                    }
                }

                // Ranked candidates precede unranked candidates. Fused rank
                // remains the deterministic fallback within either group.
                candidates.sort_by(|left, right| {
                    left.scores
                        .reranker_rank
                        .unwrap_or(usize::MAX)
                        .cmp(&right.scores.reranker_rank.unwrap_or(usize::MAX))
                        .then_with(|| left.scores.fused_rank.cmp(&right.scores.fused_rank))
                });

                Some(model_name)
            }
            Err(e) => {
                // Reranking failure is non-fatal — fall back to RRF order.
                tracing::warn!(error = %e, "reranking failed, falling back to RRF order");
                None
            }
        }
    } else {
        None
    };

    let embedding_fingerprint = if policy.use_vector {
        embedder.map(|active_embedder| {
            crate::retrieval_embedding_fingerprint(&active_embedder.fingerprint()).fingerprint_hash
        })
    } else {
        None
    };
    let retrieval_policy_json = policy.snapshot_json(
        input,
        embedding_fingerprint.as_deref(),
        reranker_model_name.as_deref(),
    );

    candidates.truncate(input.limit);
    let mut hits = Vec::with_capacity(candidates.len());
    let mut search_cache = Vec::with_capacity(candidates.len());

    // Resolve canonical positions and expand context only for returned results.
    for candidate in candidates {
        let SearchCandidate {
            result_id,
            doc_id,
            segment,
            scores,
        } = candidate;
        let snippet = truncate_snippet(&segment.body, 200);
        let entry_point = materialize_entry_point(
            store,
            &doc_id,
            &segment,
            input.expand,
            input.max_blocks,
            input.max_chars,
        )?;

        search_cache.push(shiro_store::SearchResultRow {
            result_id: result_id.clone(),
            evidence_handle: entry_point.evidence_handle.clone(),
            doc_id: doc_id.clone(),
            segment_id: segment.id.clone(),
            bm25_score: scores.bm25_score,
            bm25_rank: scores.bm25_rank,
            vector_score: scores.vector_score,
            vector_rank: scores.vector_rank,
            fused_score: Some(scores.fused_score as f32),
            fused_rank: Some(scores.fused_rank),
            reranker_score: scores.reranker_score,
            reranker_rank: scores.reranker_rank,
            block_idx: entry_point.block_idx,
            block_kind: entry_point.block_kind.clone(),
            heading_level: entry_point.heading_level,
            span_start: entry_point.span_start,
            span_end: entry_point.span_end,
            source_locators: entry_point.source_locators.clone(),
        });

        hits.push(SearchHit {
            result_id,
            evidence_handle: entry_point.evidence_handle,
            doc_id: doc_id.as_str().to_string(),
            block_idx: entry_point.block_idx,
            block_kind: entry_point.block_kind,
            heading_level: entry_point.heading_level,
            span_start: entry_point.span_start,
            span_end: entry_point.span_end,
            source_locators: entry_point.source_locators,
            snippet,
            scores,
            context_window: entry_point.context_window,
        });
    }

    // A returned result must always resolve to this exact immutable explain snapshot.
    if !search_cache.is_empty() {
        let snapshot = shiro_store::SearchSnapshotMetadata {
            search_snapshot_id: search_snapshot_id.as_str(),
            retrieval_policy_json: &retrieval_policy_json,
            query: &input.query,
            query_digest: &query_digest,
            fts_generation: fts_gen,
            vector_generation: vector_gen,
        };
        store.save_search_results(&snapshot, &search_cache)?;
    }

    let retrieval_info = RetrievalInfo {
        bm25_active: policy.use_bm25 && !bm25_hits.is_empty(),
        vector_active: policy.use_vector && !vector_hits.is_empty(),
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

/// Generate a deterministic result ID within one immutable retrieval snapshot.
pub(crate) fn make_result_id(snapshot_id: &RunId, segment_id: &str) -> String {
    let input = format!("{}:{segment_id}", snapshot_id.as_str());
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
    use crate::Engine;
    use shiro_core::{
        Concept, ConceptId, EmbeddingFingerprint, EnrichmentResult, GenerationId,
        RerankCandidateLimit, RerankResult, ShiroHome, VectorHit,
    };
    use shiro_parse::PlainTextParser;

    fn activate_test_vector_publication(engine: &Engine) {
        let generation = engine.store.active_generation("fts").unwrap();
        engine
            .store
            .activate_corpus_manifest(&shiro_core::generation::CorpusManifest {
                manifest_id: format!("corpus_test_{}", shiro_core::RunId::generate().as_str()),
                corpus_digest: "test-corpus".to_string(),
                document_count: 0,
                segment_count: 0,
                fts_generation: generation,
                fts_digest: String::new(),
                vector_generation: Some(GenerationId::ZERO),
                vector_digest: Some("test-vector".to_string()),
                embedding_fingerprint_hash: None,
                created_at: "test".to_string(),
            })
            .unwrap();
    }

    struct FixedVectorIndex {
        fingerprint: EmbeddingFingerprint,
        hits: Vec<VectorHit>,
    }

    impl VectorIndex for FixedVectorIndex {
        fn embedding_fingerprint(&self) -> Result<Option<EmbeddingFingerprint>, ShiroError> {
            Ok(Some(crate::retrieval_embedding_fingerprint(
                &self.fingerprint,
            )))
        }

        fn upsert(&self, _id: &SegmentId, _embedding: &[f32]) -> Result<(), ShiroError> {
            Ok(())
        }

        fn delete(&self, _id: &SegmentId) -> Result<(), ShiroError> {
            Ok(())
        }

        fn delete_by_doc(&self, _doc_id: &DocId) -> Result<(), ShiroError> {
            Ok(())
        }

        fn search(&self, _query: &[f32], limit: usize) -> Result<Vec<VectorHit>, ShiroError> {
            Ok(self.hits.iter().take(limit).cloned().collect())
        }

        fn count(&self) -> Result<usize, ShiroError> {
            Ok(self.hits.len())
        }

        fn dimensions(&self) -> usize {
            self.fingerprint.dimensions
        }

        fn flush(&self) -> Result<(), ShiroError> {
            Ok(())
        }
    }

    struct PreferredEvidenceReranker {
        candidate_limit: RerankCandidateLimit,
    }

    impl PreferredEvidenceReranker {
        fn new(candidate_count: usize) -> Self {
            Self {
                candidate_limit: RerankCandidateLimit::new(candidate_count).unwrap(),
            }
        }
    }

    impl Reranker for PreferredEvidenceReranker {
        fn rerank_candidate_limit(&self) -> RerankCandidateLimit {
            self.candidate_limit
        }

        fn rerank(
            &self,
            _query: &str,
            documents: &[&str],
            top_n: usize,
        ) -> Result<Vec<RerankResult>, ShiroError> {
            let mut results: Vec<RerankResult> = documents
                .iter()
                .enumerate()
                .map(|(index, document)| RerankResult {
                    index,
                    score: if document.contains("preferred evidence") {
                        1.0
                    } else {
                        0.0
                    },
                })
                .collect();
            results.sort_by(|left, right| {
                right
                    .score
                    .total_cmp(&left.score)
                    .then_with(|| left.index.cmp(&right.index))
            });
            results.truncate(top_n);
            Ok(results)
        }

        fn model_name(&self) -> &str {
            "preferred-evidence-test-reranker"
        }
    }

    #[test]
    fn ready_bm25_hit_disappears_immediately_after_tombstone_without_purge() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let parser = PlainTextParser;
        let content = b"authoritative ready-only retrieval evidence";
        let doc_id = DocId::from_content(content);

        crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "ready-only.txt",
            content,
        )
        .unwrap();

        let input = SearchInput {
            query: "authoritative".to_string(),
            mode: SearchMode::Bm25,
            limit: 10,
            expand: false,
            max_blocks: 12,
            max_chars: 8000,
            rerank: false,
            filters: SearchFilters::default(),
        };
        let ready = engine.search(&input).unwrap();
        assert_eq!(ready.hits.len(), 1);
        assert_eq!(ready.hits[0].doc_id, doc_id.as_str());

        engine
            .remove(&crate::ops::remove::RemoveInput {
                id: doc_id.as_str().to_string(),
                purge: false,
            })
            .unwrap();

        let deleted = engine.search(&input).unwrap();
        assert!(deleted.hits.is_empty());
    }

    #[test]
    fn repeated_query_results_have_distinct_immutable_explain_snapshots() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let parser = PlainTextParser;
        crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "snapshot-evidence.txt",
            b"immutable snapshot evidence",
        )
        .unwrap();
        let input = SearchInput {
            query: "snapshot".to_string(),
            mode: SearchMode::Bm25,
            limit: 1,
            expand: false,
            max_blocks: 12,
            max_chars: 8000,
            rerank: false,
            filters: SearchFilters::default(),
        };

        let first = engine.search(&input).unwrap();
        let second = engine.search(&input).unwrap();
        assert_eq!(first.hits.len(), 1);
        assert_eq!(second.hits.len(), 1);
        assert_ne!(first.hits[0].result_id, second.hits[0].result_id);

        let first_explain = engine
            .explain(&crate::ops::explain::ExplainInput {
                result_id: first.hits[0].result_id.clone(),
            })
            .unwrap();
        let second_explain = engine
            .explain(&crate::ops::explain::ExplainInput {
                result_id: second.hits[0].result_id.clone(),
            })
            .unwrap();
        assert_eq!(
            first_explain.fused_score,
            first.hits[0].scores.fused_score as f32
        );
        assert_eq!(
            second_explain.fused_score,
            second.hits[0].scores.fused_score as f32
        );
        assert_eq!(
            first_explain.retrieval_trace.pipeline[0],
            "retrieval_policy"
        );
        assert_eq!(
            first_explain.retrieval_trace.stages[0]["policy"]["mode"],
            "bm25"
        );
        assert_ne!(
            first_explain.retrieval_trace.stages[0]["search_snapshot_id"],
            second_explain.retrieval_trace.stages[0]["search_snapshot_id"]
        );
        assert!(first_explain
            .retrieval_trace
            .pipeline
            .iter()
            .any(|stage| stage == "provenance"));
    }

    #[test]
    fn search_and_explain_return_snapshot_correct_source_locators() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let ingested = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &PlainTextParser,
            "locator-evidence.txt",
            b"physical source locator evidence",
        )
        .unwrap();
        let mut graph = engine.store.get_block_graph(&ingested.doc_id).unwrap();
        graph.blocks[0].source_locators =
            vec![shiro_core::SourceLocator::new(7, None, None, None).unwrap()];
        engine
            .store
            .put_block_graph(&ingested.doc_id, &graph)
            .unwrap();

        let search = engine
            .search(&SearchInput {
                query: "locator evidence".to_string(),
                mode: SearchMode::Bm25,
                limit: 1,
                expand: true,
                max_blocks: 12,
                max_chars: 8000,
                rerank: false,
                filters: SearchFilters::default(),
            })
            .unwrap();
        assert_eq!(search.hits[0].source_locators[0].page_number(), 7);
        assert_eq!(
            search.hits[0].context_window[0].source_locators[0].page_number(),
            7
        );
        let page = engine
            .read(&crate::ops::read::ReadInput {
                id: ingested.doc_id.as_str().to_string(),
                mode: crate::ops::read::ReadMode::Page,
                page: Some(7),
            })
            .unwrap();
        let crate::ops::read::ReadContent::Blocks { blocks } = page.content else {
            panic!("page read must return blocks");
        };
        assert_eq!(blocks.len(), 1);

        let deferred = engine
            .read(&crate::ops::read::ReadInput {
                id: search.hits[0].evidence_handle.as_str().to_string(),
                mode: crate::ops::read::ReadMode::Blocks,
                page: None,
            })
            .unwrap();
        assert_eq!(
            deferred.evidence_resolution.as_ref().unwrap().status,
            "ACTIVE"
        );

        graph.blocks[0].source_locators =
            vec![shiro_core::SourceLocator::new(99, None, None, None).unwrap()];
        engine
            .store
            .put_block_graph(&ingested.doc_id, &graph)
            .unwrap();
        let explain = engine
            .explain(&crate::ops::explain::ExplainInput {
                result_id: search.hits[0].result_id.clone(),
            })
            .unwrap();
        assert_eq!(explain.source_locators[0].page_number(), 7);
        assert_eq!(
            explain.evidence_handle.as_ref(),
            Some(&search.hits[0].evidence_handle)
        );
    }

    #[test]
    fn indexing_and_failed_documents_do_not_leak_from_existing_fts_entries() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let parser = PlainTextParser;
        let indexing = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "indexing-divergence.txt",
            b"divergence indexing evidence",
        )
        .unwrap();
        let failed = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "failed-divergence.txt",
            b"divergence failed evidence",
        )
        .unwrap();

        let (indexing_document, _) = engine.store.get_document(&indexing.doc_id).unwrap();
        engine
            .store
            .put_document(&indexing_document, shiro_core::DocState::Indexing)
            .unwrap();
        let (failed_document, _) = engine.store.get_document(&failed.doc_id).unwrap();
        engine
            .store
            .put_document(&failed_document, shiro_core::DocState::Failed)
            .unwrap();

        let output = engine
            .search(&SearchInput {
                query: "divergence".to_string(),
                mode: SearchMode::Bm25,
                limit: 10,
                expand: true,
                max_blocks: 12,
                max_chars: 8000,
                rerank: false,
                filters: SearchFilters::default(),
            })
            .unwrap();

        assert!(output.hits.is_empty());
    }

    #[test]
    fn context_and_explain_are_unavailable_after_tombstone() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let parser = PlainTextParser;
        let ingested = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "explain-ready-only.txt",
            b"explainable authoritative evidence",
        )
        .unwrap();
        let input = SearchInput {
            query: "explainable".to_string(),
            mode: SearchMode::Bm25,
            limit: 1,
            expand: true,
            max_blocks: 12,
            max_chars: 8000,
            rerank: false,
            filters: SearchFilters::default(),
        };

        let ready = engine.search(&input).unwrap();
        assert_eq!(ready.hits.len(), 1);
        assert!(!ready.hits[0].context_window.is_empty());
        let result_id = ready.hits[0].result_id.clone();
        let evidence_handle = ready.hits[0].evidence_handle.clone();
        engine
            .explain(&crate::ops::explain::ExplainInput {
                result_id: result_id.clone(),
            })
            .unwrap();

        engine
            .remove(&crate::ops::remove::RemoveInput {
                id: ingested.doc_id.as_str().to_string(),
                purge: false,
            })
            .unwrap();

        let deleted = engine.search(&input).unwrap();
        assert!(deleted.hits.is_empty());
        let explain_error = engine
            .explain(&crate::ops::explain::ExplainInput { result_id })
            .unwrap_err();
        assert!(matches!(explain_error, ShiroError::NotFoundMsg { .. }));
        let read_error = engine
            .read(&crate::ops::read::ReadInput {
                id: evidence_handle.as_str().to_string(),
                mode: crate::ops::read::ReadMode::Blocks,
                page: None,
            })
            .unwrap_err();
        assert!(matches!(read_error, ShiroError::NotFoundMsg { .. }));
    }

    #[test]
    fn vector_only_and_hybrid_fill_from_ready_scope() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let parser = PlainTextParser;

        let deleted = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "deleted-vector.txt",
            b"eligible eligible deleted vector evidence",
        )
        .unwrap();
        let deleted_segment = engine.store.get_segments(&deleted.doc_id).unwrap()[0]
            .id
            .clone();
        engine
            .remove(&crate::ops::remove::RemoveInput {
                id: deleted.doc_id.as_str().to_string(),
                purge: false,
            })
            .unwrap();

        let competitor = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "competitor-vector.txt",
            b"eligible eligible ready competitor",
        )
        .unwrap();
        let competitor_segment = engine.store.get_segments(&competitor.doc_id).unwrap()[0]
            .id
            .clone();
        let ready = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "ready-vector.txt",
            b"eligible ready vector evidence",
        )
        .unwrap();
        let ready_segment = engine.store.get_segments(&ready.doc_id).unwrap()[0]
            .id
            .clone();

        activate_test_vector_publication(&engine);
        let embedder = shiro_embed::DeterministicStubEmbedder::new(4);
        let vector_index = FixedVectorIndex {
            fingerprint: embedder.fingerprint(),
            hits: vec![
                VectorHit {
                    segment_id: deleted_segment,
                    score: 1.0,
                },
                VectorHit {
                    segment_id: competitor_segment,
                    score: 0.8,
                },
                VectorHit {
                    segment_id: ready_segment,
                    score: 0.5,
                },
            ],
        };
        let engine = engine
            .with_embedder(Box::new(embedder))
            .with_vector_index(Box::new(vector_index));
        let mut input = SearchInput {
            query: "eligible".to_string(),
            mode: SearchMode::Vector,
            limit: 1,
            expand: false,
            max_blocks: 12,
            max_chars: 8000,
            rerank: false,
            filters: SearchFilters {
                document_ids: vec![ready.doc_id.as_str().to_string()],
                ..SearchFilters::default()
            },
        };

        let vector = engine.search(&input).unwrap();
        assert_eq!(vector.hits.len(), 1);
        assert_eq!(vector.hits[0].doc_id, ready.doc_id.as_str());

        input.mode = SearchMode::Hybrid;
        let hybrid = engine.search(&input).unwrap();
        assert_eq!(hybrid.hits.len(), 1);
        assert_eq!(hybrid.hits[0].doc_id, ready.doc_id.as_str());
    }

    #[test]
    fn structural_retrieval_text_finds_body_by_heading_without_polluting_snippet() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &shiro_parse::MarkdownParser,
            "structural.md",
            b"# Alpha\n\n## Beta Context\n\nneedle evidence without heading terms",
        )
        .unwrap();

        let output = engine
            .search(&SearchInput {
                query: "Beta Context".to_string(),
                mode: SearchMode::Bm25,
                limit: 10,
                expand: false,
                max_blocks: 12,
                max_chars: 8000,
                rerank: false,
                filters: SearchFilters::default(),
            })
            .unwrap();
        let body_hit = output
            .hits
            .iter()
            .find(|hit| hit.block_kind == "PARAGRAPH")
            .expect("heading context should retrieve the paragraph body");
        assert!(body_hit.snippet.contains("needle evidence"));
        assert!(!body_hit.snippet.contains("Section:"));
    }

    #[test]
    fn hybrid_deduplicates_segments_present_in_bm25_and_vector_sources() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let ingested = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &PlainTextParser,
            "hybrid-dedup.txt",
            b"shared hybrid evidence",
        )
        .unwrap();
        let segment_id = engine.store.get_segments(&ingested.doc_id).unwrap()[0]
            .id
            .clone();
        activate_test_vector_publication(&engine);
        let embedder = shiro_embed::DeterministicStubEmbedder::new(4);
        let vector_index = FixedVectorIndex {
            fingerprint: embedder.fingerprint(),
            hits: vec![VectorHit {
                segment_id,
                score: 1.0,
            }],
        };

        let output = engine
            .with_embedder(Box::new(embedder))
            .with_vector_index(Box::new(vector_index))
            .search(&SearchInput {
                query: "hybrid evidence".to_string(),
                mode: SearchMode::Hybrid,
                limit: 10,
                expand: false,
                max_blocks: 12,
                max_chars: 8000,
                rerank: false,
                filters: SearchFilters::default(),
            })
            .unwrap();

        assert_eq!(output.hits.len(), 1);
        assert!(output.hits[0].scores.bm25_rank.is_some());
        assert!(output.hits[0].scores.vector_rank.is_some());
    }

    #[test]
    fn reranking_cannot_reintroduce_deleted_vector_candidate() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let parser = PlainTextParser;
        let deleted = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "preferred-deleted.txt",
            b"candidate preferred evidence",
        )
        .unwrap();
        let deleted_segment = engine.store.get_segments(&deleted.doc_id).unwrap()[0]
            .id
            .clone();
        engine
            .remove(&crate::ops::remove::RemoveInput {
                id: deleted.doc_id.as_str().to_string(),
                purge: false,
            })
            .unwrap();
        let ready = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "ordinary-ready.txt",
            b"candidate ordinary evidence",
        )
        .unwrap();
        let ready_segment = engine.store.get_segments(&ready.doc_id).unwrap()[0]
            .id
            .clone();

        activate_test_vector_publication(&engine);
        let embedder = shiro_embed::DeterministicStubEmbedder::new(4);
        let vector_index = FixedVectorIndex {
            fingerprint: embedder.fingerprint(),
            hits: vec![
                VectorHit {
                    segment_id: deleted_segment,
                    score: 1.0,
                },
                VectorHit {
                    segment_id: ready_segment,
                    score: 0.5,
                },
            ],
        };
        let output = engine
            .with_embedder(Box::new(embedder))
            .with_vector_index(Box::new(vector_index))
            .with_reranker(Box::new(PreferredEvidenceReranker::new(2)))
            .search(&SearchInput {
                query: "candidate".to_string(),
                mode: SearchMode::Vector,
                limit: 1,
                expand: false,
                max_blocks: 12,
                max_chars: 8000,
                rerank: true,
                filters: SearchFilters::default(),
            })
            .unwrap();

        assert_eq!(output.hits.len(), 1);
        assert_eq!(output.hits[0].doc_id, ready.doc_id.as_str());
        assert_eq!(output.hits[0].scores.reranker_rank, Some(1));
    }

    #[test]
    fn tag_concept_and_document_filters_resolve_before_bm25_ranking() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let parser = PlainTextParser;

        for (index, content) in [
            "filtered filtered filtered competitor",
            "filtered filtered second competitor",
        ]
        .iter()
        .enumerate()
        {
            crate::ops::document_ingestion::ingest_document_bytes(
                &engine.store,
                &engine.fts,
                &parser,
                &format!("competitor-{index}.txt"),
                content.as_bytes(),
            )
            .unwrap();
        }
        let target = crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "target.txt",
            b"filtered target evidence",
        )
        .unwrap();
        engine
            .store
            .put_enrichment(&EnrichmentResult {
                doc_id: target.doc_id.clone(),
                title: None,
                summary: None,
                tags: vec!["alpha".to_string()],
                concepts: Vec::new(),
                provider: "test".to_string(),
                content_hash: "test".to_string(),
                created_at: "test".to_string(),
            })
            .unwrap();
        let concept = Concept {
            id: ConceptId::new("https://example.test/scheme", "Filter Concept"),
            scheme_uri: "https://example.test/scheme".to_string(),
            pref_label: "Filter Concept".to_string(),
            alt_labels: Vec::new(),
            definition: None,
        };
        engine.store.put_concept(&concept).unwrap();
        engine
            .store
            .assign_concept_to_doc(&target.doc_id, &concept.id, 1.0, "test")
            .unwrap();

        let output = engine
            .search(&SearchInput {
                query: "filtered".to_string(),
                mode: SearchMode::Bm25,
                limit: 1,
                expand: true,
                max_blocks: 12,
                max_chars: 8000,
                rerank: false,
                filters: SearchFilters {
                    tags: vec!["ALPHA".to_string()],
                    concept_ids: vec![concept.id.as_str().to_string()],
                    document_ids: vec![target.doc_id.as_str().to_string()],
                },
            })
            .unwrap();

        assert_eq!(output.hits.len(), 1);
        assert_eq!(output.hits[0].doc_id, target.doc_id.as_str());
        let explained = engine
            .explain(&crate::ops::explain::ExplainInput {
                result_id: output.hits[0].result_id.clone(),
            })
            .unwrap();
        assert_eq!(
            explained.retrieval_trace.stages[0]["policy"]["filters"]["tags"][0],
            "ALPHA"
        );
    }

    #[test]
    fn selective_ready_scope_fills_bm25_result_limit() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let parser = PlainTextParser;
        let deleted_contents = [
            "scope scope scope scope scope deleted first",
            "scope scope scope scope deleted second",
            "scope scope scope deleted third",
        ];
        for (index, content) in deleted_contents.iter().enumerate() {
            let ingested = crate::ops::document_ingestion::ingest_document_bytes(
                &engine.store,
                &engine.fts,
                &parser,
                &format!("deleted-{index}.txt"),
                content.as_bytes(),
            )
            .unwrap();
            engine
                .remove(&crate::ops::remove::RemoveInput {
                    id: ingested.doc_id.as_str().to_string(),
                    purge: false,
                })
                .unwrap();
        }

        let ready_contents = ["scope ready fourth", "scope ready fifth"];
        let mut ready_doc_ids = Vec::new();
        for (index, content) in ready_contents.iter().enumerate() {
            let ingested = crate::ops::document_ingestion::ingest_document_bytes(
                &engine.store,
                &engine.fts,
                &parser,
                &format!("ready-{index}.txt"),
                content.as_bytes(),
            )
            .unwrap();
            ready_doc_ids.push(ingested.doc_id.as_str().to_string());
        }

        let output = engine
            .search(&SearchInput {
                query: "scope".to_string(),
                mode: SearchMode::Bm25,
                limit: 2,
                expand: false,
                max_blocks: 12,
                max_chars: 8000,
                rerank: false,
                filters: SearchFilters::default(),
            })
            .unwrap();

        assert_eq!(output.hits.len(), 2);
        assert!(output
            .hits
            .iter()
            .all(|hit| ready_doc_ids.contains(&hit.doc_id)));
    }

    #[test]
    fn reranker_can_promote_candidate_below_result_limit() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        let parser = PlainTextParser;
        let preferred_content =
            format!("candidate {} preferred evidence", "background ".repeat(100));
        let preferred_doc_id = DocId::from_content(preferred_content.as_bytes());

        crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "higher-bm25-score.txt",
            b"candidate candidate candidate",
        )
        .unwrap();
        crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "second-higher-bm25-score.txt",
            b"candidate candidate",
        )
        .unwrap();
        crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &parser,
            "preferred-evidence.txt",
            preferred_content.as_bytes(),
        )
        .unwrap();

        let mut input = SearchInput {
            query: "candidate".to_string(),
            mode: SearchMode::Bm25,
            limit: 3,
            expand: false,
            max_blocks: 12,
            max_chars: 8000,
            rerank: false,
            filters: SearchFilters::default(),
        };
        let baseline = engine.search(&input).unwrap();
        assert_eq!(baseline.hits.len(), 3);
        assert_eq!(baseline.hits[2].doc_id, preferred_doc_id.as_str());

        input.limit = 1;
        input.rerank = true;
        let engine = engine.with_reranker(Box::new(PreferredEvidenceReranker::new(2)));
        let limited_pool = engine.search(&input).unwrap();
        assert_ne!(limited_pool.hits[0].doc_id, preferred_doc_id.as_str());

        let reranked = engine
            .with_reranker(Box::new(PreferredEvidenceReranker::new(3)))
            .search(&input)
            .unwrap();

        assert_eq!(reranked.hits.len(), 1);
        assert_eq!(reranked.hits[0].doc_id, preferred_doc_id.as_str());
        assert_eq!(reranked.hits[0].scores.reranker_rank, Some(1));
    }

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
        let snapshot_id = RunId::new("run_snapshot");
        let id1 = make_result_id(&snapshot_id, "seg_abc");
        let id2 = make_result_id(&snapshot_id, "seg_abc");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("res_"));
        assert_eq!(id1.len(), 4 + 16); // "res_" + 16 hex chars
    }

    #[test]
    fn make_result_id_differs_for_different_inputs() {
        let id1 = make_result_id(&RunId::new("run_first"), "seg_abc");
        let id2 = make_result_id(&RunId::new("run_second"), "seg_abc");
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
            filters: SearchFilters::default(),
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
