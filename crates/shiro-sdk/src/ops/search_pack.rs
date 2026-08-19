//! Batched, deduplicated multi-query evidence retrieval.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use shiro_core::ports::{Embedder, Reranker, VectorIndex};
use shiro_core::{EvidenceHandleId, ShiroError, SourceLocator};
use shiro_index::FtsIndex;
use shiro_store::Store;

use super::search::{self, ContextBlock, SearchFilters, SearchInput, SearchMode};
use crate::RRF_K;

const MAX_SEARCH_PACK_QUERIES: usize = 32;
const MAX_SEARCH_PACK_RESULTS: usize = 200;
const MAX_SEARCH_PACK_QUERY_BYTES: usize = 16_384;
const MAX_SEARCH_PACK_CONTEXT_BLOCKS: usize = 100;
const MAX_SEARCH_PACK_CONTEXT_BYTES: usize = 1_048_576;

/// One named query within a multi-query search pack.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchPackQuery {
    pub query_id: String,
    pub text: String,
}

/// Bounded multi-query request with shared retrieval policy and optional content expansion.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchPackInput {
    pub queries: Vec<SearchPackQuery>,
    pub mode: SearchMode,
    #[serde(default = "default_per_query_limit")]
    pub per_query_limit: usize,
    #[serde(default = "default_global_limit")]
    pub global_limit: usize,
    #[serde(default)]
    pub include_content: bool,
    #[serde(default = "default_max_blocks")]
    pub max_blocks: usize,
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
    #[serde(default)]
    pub rerank: bool,
    #[serde(default)]
    pub filters: SearchFilters,
}

/// Deduplicated multi-query evidence results and pre-truncation evidence count.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchPackOutput {
    pub query_count: usize,
    pub unique_evidence_count: usize,
    pub returned_count: usize,
    pub mode: String,
    pub hits: Vec<SearchPackHit>,
}

/// Stable canonical evidence matched by one or more named search-pack queries.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchPackHit {
    pub evidence_handle: EvidenceHandleId,
    pub doc_id: String,
    pub block_idx: usize,
    pub block_kind: String,
    pub heading_level: Option<u32>,
    pub span_start: usize,
    pub span_end: usize,
    pub source_locators: Vec<SourceLocator>,
    pub matched_queries: Vec<String>,
    pub result_ids: BTreeMap<String, String>,
    pub aggregate_score: f64,
    pub best_rank: usize,
    pub snippet: Option<String>,
    pub context_window: Vec<ContextBlock>,
}

fn default_per_query_limit() -> usize {
    10
}

fn default_global_limit() -> usize {
    20
}

fn default_max_blocks() -> usize {
    12
}

fn default_max_chars() -> usize {
    8_000
}

struct AccumulatedHit {
    hit: SearchPackHit,
    matched_query_set: HashSet<String>,
}

/// Execute bounded queries independently, then deduplicate and rank by stable evidence handle.
pub fn execute(
    store: &Store,
    fts: &FtsIndex,
    embedder: Option<&dyn Embedder>,
    vector_index: Option<&dyn VectorIndex>,
    reranker: Option<&dyn Reranker>,
    input: &SearchPackInput,
) -> Result<SearchPackOutput, ShiroError> {
    validate_input(input)?;
    let mut accumulated: HashMap<EvidenceHandleId, AccumulatedHit> = HashMap::new();

    for query in &input.queries {
        let output = search::execute(
            store,
            fts,
            embedder,
            vector_index,
            reranker,
            &SearchInput {
                query: query.text.clone(),
                mode: input.mode,
                limit: input.per_query_limit,
                expand: input.include_content,
                max_blocks: input.max_blocks,
                max_chars: input.max_chars,
                rerank: input.rerank,
                filters: input.filters.clone(),
            },
        )?;
        for (rank, result) in output.hits.into_iter().enumerate() {
            let contribution = 1.0 / (RRF_K + (rank + 1) as f64);
            let handle = result.evidence_handle.clone();
            let entry = accumulated
                .entry(handle.clone())
                .or_insert_with(|| AccumulatedHit {
                    matched_query_set: HashSet::new(),
                    hit: SearchPackHit {
                        evidence_handle: handle,
                        doc_id: result.doc_id.clone(),
                        block_idx: result.block_idx,
                        block_kind: result.block_kind.clone(),
                        heading_level: result.heading_level,
                        span_start: result.span_start,
                        span_end: result.span_end,
                        source_locators: result.source_locators.clone(),
                        matched_queries: Vec::new(),
                        result_ids: BTreeMap::new(),
                        aggregate_score: 0.0,
                        best_rank: rank + 1,
                        snippet: input.include_content.then(|| result.snippet.clone()),
                        context_window: if input.include_content {
                            result.context_window
                        } else {
                            Vec::new()
                        },
                    },
                });
            entry.hit.aggregate_score += contribution;
            entry.hit.best_rank = entry.hit.best_rank.min(rank + 1);
            entry
                .hit
                .result_ids
                .insert(query.query_id.clone(), result.result_id);
            if entry.matched_query_set.insert(query.query_id.clone()) {
                entry.hit.matched_queries.push(query.query_id.clone());
            }
        }
    }

    let mut hits = accumulated
        .into_values()
        .map(|accumulated| accumulated.hit)
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .aggregate_score
            .total_cmp(&left.aggregate_score)
            .then_with(|| {
                left.evidence_handle
                    .as_str()
                    .cmp(right.evidence_handle.as_str())
            })
    });
    let unique_evidence_count = hits.len();
    hits.truncate(input.global_limit);

    Ok(SearchPackOutput {
        query_count: input.queries.len(),
        unique_evidence_count,
        returned_count: hits.len(),
        mode: input.mode.as_str().to_string(),
        hits,
    })
}

fn validate_input(input: &SearchPackInput) -> Result<(), ShiroError> {
    if input.queries.is_empty() || input.queries.len() > MAX_SEARCH_PACK_QUERIES {
        return Err(ShiroError::InvalidInput {
            message: format!(
                "search pack query count must be between 1 and {MAX_SEARCH_PACK_QUERIES}"
            ),
        });
    }
    if input.per_query_limit == 0
        || input.per_query_limit > MAX_SEARCH_PACK_RESULTS
        || input.global_limit == 0
        || input.global_limit > MAX_SEARCH_PACK_RESULTS
    {
        return Err(ShiroError::InvalidInput {
            message: format!(
                "search pack result limits must be between 1 and {MAX_SEARCH_PACK_RESULTS}"
            ),
        });
    }
    if input.max_blocks > MAX_SEARCH_PACK_CONTEXT_BLOCKS
        || input.max_chars > MAX_SEARCH_PACK_CONTEXT_BYTES
    {
        return Err(ShiroError::InvalidInput {
            message: format!(
                "search pack context exceeds max_blocks={MAX_SEARCH_PACK_CONTEXT_BLOCKS} or max_chars={MAX_SEARCH_PACK_CONTEXT_BYTES}"
            ),
        });
    }
    let mut ids = HashSet::new();
    for query in &input.queries {
        if query.query_id.trim().is_empty()
            || query.text.trim().is_empty()
            || query.text.len() > MAX_SEARCH_PACK_QUERY_BYTES
            || !ids.insert(query.query_id.as_str())
        {
            return Err(ShiroError::InvalidInput {
                message: format!("invalid or duplicate search-pack query: {}", query.query_id),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;

    #[test]
    fn search_pack_rejects_unbounded_query_fanout() {
        let input = SearchPackInput {
            queries: (0..=MAX_SEARCH_PACK_QUERIES)
                .map(|index| SearchPackQuery {
                    query_id: format!("q{index}"),
                    text: "bounded evidence".to_string(),
                })
                .collect(),
            mode: SearchMode::Bm25,
            per_query_limit: 5,
            global_limit: 5,
            include_content: false,
            max_blocks: 12,
            max_chars: 8_000,
            rerank: false,
            filters: SearchFilters::default(),
        };

        let error = validate_input(&input).unwrap_err();
        assert!(matches!(error, ShiroError::InvalidInput { .. }));
    }

    #[test]
    fn multiple_queries_deduplicate_evidence_and_omit_content_by_default() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(temporary.path())
            .unwrap()
            .to_owned();
        let home = shiro_core::ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let engine = Engine::open(home).unwrap();
        crate::ops::document_ingestion::ingest_document_bytes(
            &engine.store,
            &engine.fts,
            &shiro_parse::PlainTextParser,
            "pack.txt",
            b"batched evidence retrieval",
        )
        .unwrap();

        let output = engine
            .search_pack(&SearchPackInput {
                queries: vec![
                    SearchPackQuery {
                        query_id: "q1".to_string(),
                        text: "batched evidence".to_string(),
                    },
                    SearchPackQuery {
                        query_id: "q2".to_string(),
                        text: "evidence retrieval".to_string(),
                    },
                ],
                mode: SearchMode::Bm25,
                per_query_limit: 5,
                global_limit: 5,
                include_content: false,
                max_blocks: 12,
                max_chars: 8_000,
                rerank: false,
                filters: SearchFilters::default(),
            })
            .unwrap();

        assert_eq!(output.query_count, 2);
        assert_eq!(output.unique_evidence_count, 1);
        assert_eq!(output.hits[0].matched_queries, vec!["q1", "q2"]);
        assert_eq!(output.hits[0].result_ids.len(), 2);
        assert!(output.hits[0].snippet.is_none());
        assert!(output.hits[0].context_window.is_empty());
    }
}
