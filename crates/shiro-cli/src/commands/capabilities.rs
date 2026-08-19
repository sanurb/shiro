//! `shiro capabilities` — describe shiro's capabilities as structured JSON.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_store::Store;

/// Static registry of all CLI commands.
const COMMANDS: &[&str] = &[
    "init",
    "add",
    "acquire-url",
    "ingest",
    "search",
    "search-pack",
    "read",
    "explain",
    "list",
    "remove",
    "doctor",
    "config",
    "capabilities",
    "mcp",
    "taxonomy",
    "reindex",
    "reprocess",
    "benchmark",
    "completions",
    "enrich",
    "enrich-model",
];

/// Known parsers and their status.
const PARSERS: &[&str] = &["plaintext", "markdown", "pdf", "docling"];

pub fn run(home: &ShiroHome) -> Result<CmdOutput, ShiroError> {
    let store = Store::open(&home.db_path()).ok();
    let schema_version = store
        .as_ref()
        .and_then(|store| store.schema_version().ok())
        .unwrap_or(0);

    // Check the exact generations selected by the authoritative pointers.
    let fts_generation = store
        .as_ref()
        .and_then(|store| store.active_generation("fts").ok())
        .map(|generation| generation.as_u64())
        .unwrap_or(0);
    let vector_generation = store
        .as_ref()
        .and_then(|store| store.active_generation("vector").ok())
        .map(|generation| generation.as_u64())
        .unwrap_or(0);
    let vector_published = store
        .as_ref()
        .and_then(|store| store.active_corpus_manifest().ok().flatten())
        .map(|manifest| manifest.vector_generation.is_some())
        .unwrap_or(true);
    let fts_available = home
        .tantivy_generation_dir(fts_generation)
        .as_std_path()
        .is_dir();
    let vector_available = vector_published
        && home
            .vector_data_path(vector_generation)
            .as_std_path()
            .is_file();

    let result = serde_json::json!({
        "schemaVersion": 2,
        "version": env!("CARGO_PKG_VERSION"),
        "schema_version": schema_version,
        "commands": COMMANDS,
        "state_machine": {
            "states": ["STAGED", "INDEXING", "READY", "FAILED", "DELETED"],
            "transitions": [
                { "from": "STAGED",   "to": "INDEXING" },
                { "from": "INDEXING",  "to": "READY" },
                { "from": "INDEXING",  "to": "FAILED" },
                { "from": "FAILED",    "to": "STAGED" },
                { "from": "*",         "to": "DELETED" },
            ],
        },
        "id_schemes": {
            "doc_id":     { "prefix": "doc_",  "algorithm": "blake3(content)" },
            "segment_id": { "prefix": "seg_",  "algorithm": "blake3(doc_id:index)" },
            "run_id":     { "prefix": "run_",  "algorithm": "timestamp" },
            "concept_id": { "prefix": "con_",  "algorithm": "blake3(scheme_uri:pref_label)" },
            "result_id":  { "prefix": "res_",  "algorithm": "blake3(search_snapshot:segment_id)" },
            "evidence_handle": { "prefix": "blk_", "algorithm": "blake3(doc_id:block_text:occurrence)" },
        },
        "parsers": PARSERS,
        "features": {
            "fts_bm25":       "implemented",
            "hybrid_search":  "implemented",
            "vector_embed":   "implemented",
            "reranking":      "implemented",
            "taxonomy":       "implemented",
            "enrichment":     "heuristic_only",
            "mcp_server":     "code_mode",
            "completions":    "implemented",
            "judged_benchmark": "implemented",
            "stable_evidence_handles": "implemented",
            "multi_query_search_pack": "implemented",
            "bounded_reprocessing_planner": "implemented",
            "mcp_current_protocol_and_authority": "implemented",
            "bounded_url_acquisition": "implemented",
            "taxonomy_browse_search": "implemented",
            "reversible_model_enrichment_proposals": "implemented",
            "automatic_concept_proposals": "proposed_only",
        },
        "taxonomy": {
            "subcommands": crate::commands::TAXONOMY_SUBCOMMANDS,
            "hierarchical_filter_semantics": "ancestor_or_self_matches_descendants",
            "cycle_error_code": "E_TAXONOMY_CYCLE",
        },
        "embedding": {
            "providers": ["fastembed", "http"],
            "vector_index": "flat",
            "fusion": "rrf",
            "reranker_providers": ["fastembed"],
        },
        "storage": {
            "engine":     "sqlite",
            "fts_engine": "tantivy",
            "fts_present": fts_available,
            "vector_present": vector_available,
        },
    });

    Ok(CmdOutput {
        result,
        next_actions: vec![
            NextAction::simple("shiro doctor", "Run consistency checks"),
            NextAction::simple("shiro list", "List documents"),
        ],
    })
}
