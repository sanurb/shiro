//! `shiro search-pack` — bounded multi-query retrieval with stable evidence deduplication.

use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::{SearchFilters, SearchMode, SearchPackInput, SearchPackQuery};

use crate::envelope::{CmdOutput, NextAction};
use crate::runtime;

/// CLI adapter options for one bounded, deduplicated multi-query evidence search.
pub struct SearchPackOptions<'a> {
    pub queries: &'a [String],
    pub mode: SearchMode,
    pub per_query_limit: usize,
    pub global_limit: usize,
    pub include_content: bool,
    pub rerank: bool,
    pub tags: &'a [String],
    pub concept_ids: &'a [String],
    pub document_ids: &'a [String],
}

/// Execute a search pack and return stable evidence handles in the JSON envelope.
pub fn run(home: &ShiroHome, options: SearchPackOptions<'_>) -> Result<CmdOutput, ShiroError> {
    let runtime_profile = match (options.mode, options.rerank) {
        (SearchMode::Bm25, false) => runtime::RuntimeProfile::Base,
        (SearchMode::Bm25, true) => runtime::RuntimeProfile::RerankOnly,
        (_, false) => runtime::RuntimeProfile::Vector,
        (_, true) => runtime::RuntimeProfile::Full,
    };
    let engine = runtime::open_engine(home, runtime_profile)?;
    let output = engine.search_pack(&SearchPackInput {
        queries: options
            .queries
            .iter()
            .enumerate()
            .map(|(index, text)| SearchPackQuery {
                query_id: format!("q{}", index + 1),
                text: text.clone(),
            })
            .collect(),
        mode: options.mode,
        per_query_limit: options.per_query_limit,
        global_limit: options.global_limit,
        include_content: options.include_content,
        max_blocks: 12,
        max_chars: 8_000,
        rerank: options.rerank,
        filters: SearchFilters {
            tags: options.tags.to_vec(),
            concept_ids: options.concept_ids.to_vec(),
            document_ids: options.document_ids.to_vec(),
        },
    })?;
    let value = serde_json::to_value(output).map_err(|error| ShiroError::StoreCorrupt {
        message: format!("failed to serialize search-pack output: {error}"),
    })?;
    Ok(CmdOutput {
        result: value,
        next_actions: vec![
            NextAction::simple("shiro read", "Read a returned evidence_handle"),
            NextAction::simple("shiro explain", "Explain a per-query result_id"),
        ],
    })
}
