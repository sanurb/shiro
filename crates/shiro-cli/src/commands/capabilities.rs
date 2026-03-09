//! `shiro capabilities` — describe shiro's capabilities as structured JSON.

use crate::envelope::{CmdOutput, NextAction};
use shiro_core::{ShiroError, ShiroHome};
use shiro_store::Store;

/// Static registry of all CLI commands.
const COMMANDS: &[&str] = &[
    "init",
    "add",
    "ingest",
    "search",
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
    "completions",
    "enrich",
];

/// Known parsers and their status.
const PARSERS: &[&str] = &["plaintext", "markdown", "pdf", "docling"];

pub fn run(home: &ShiroHome) -> Result<CmdOutput, ShiroError> {
    let schema_version = Store::open(&home.db_path())
        .and_then(|s| s.schema_version())
        .unwrap_or(0);

    // Check what's actually available
    let fts_available = home.tantivy_dir().as_std_path().is_dir();
    let vector_available = home.vector_dir().as_std_path().is_dir()
        && home.vector_dir().join("flat.jsonl").as_std_path().is_file();

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
            "result_id":  { "prefix": "res_",  "algorithm": "blake3(query:segment_id)[..16]" },
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
