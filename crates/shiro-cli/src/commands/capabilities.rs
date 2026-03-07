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
const PARSERS: &[&str] = &["plaintext", "markdown", "pdf"];

pub fn run(home: &ShiroHome) -> Result<CmdOutput, ShiroError> {
    let schema_version = Store::open(&home.db_path())
        .and_then(|s| s.schema_version())
        .unwrap_or(0);

    // Check what's actually available
    let fts_available = home.tantivy_dir().as_std_path().is_dir();

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
            "hybrid_search":  "bm25_only",
            "taxonomy":       "implemented",
            "enrichment":     "heuristic_only",
            "mcp_server":     "code_mode",
            "completions":    "implemented",
            "vector_embed":   "http_embedder",
        },
        "storage": {
            "engine":     "sqlite",
            "fts_engine": "tantivy",
            "fts_present": fts_available,
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
