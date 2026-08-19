//! SDK executor — dispatches JSON "programs" to typed SDK operations.
//!
//! Used by Code Mode MCP: `execute(program)` calls this to run an operation.
//! A "program" is `{ "op": "<name>", "params": { ... } }`.

use serde_json::Value;
use shiro_core::ports::{Embedder, Parser, Reranker, VectorIndex};
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_store::Store;

use crate::ops;

/// Optional provider-agnostic adapters available to SDK program execution.
#[derive(Clone, Copy, Default)]
pub struct ExecutorPorts<'a> {
    /// Embedder paired with `vector_index` for vector-capable search.
    pub embedder: Option<&'a dyn Embedder>,
    /// Vector index paired with `embedder` and checked by fingerprint.
    pub vector_index: Option<&'a dyn VectorIndex>,
    /// Optional post-fusion reranker.
    pub reranker: Option<&'a dyn Reranker>,
}

/// Execute a JSON program against the given home/store/index/parser.
///
/// The program must be `{ "op": "...", "params": { ... } }`.
/// Returns the operation result as a JSON value.
pub fn execute(
    home: &ShiroHome,
    store: &Store,
    fts: &FtsIndex,
    parser: &dyn Parser,
    program: &Value,
) -> Result<Value, ShiroError> {
    execute_with_ports(home, store, fts, parser, ExecutorPorts::default(), program)
}

/// Execute a JSON program with optional retrieval adapters at the SDK seam.
pub fn execute_with_ports(
    home: &ShiroHome,
    store: &Store,
    fts: &FtsIndex,
    parser: &dyn Parser,
    ports: ExecutorPorts<'_>,
    program: &Value,
) -> Result<Value, ShiroError> {
    let op =
        program
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ShiroError::InvalidInput {
                message: "program missing 'op' field".into(),
            })?;

    let params = program
        .get("params")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    match op {
        "acquire_url" => exec_acquire(store, fts, &params),
        "add" => exec_add(store, fts, parser, &params),
        "ingest" => exec_ingest(store, fts, parser, &params),
        "search" => exec_search(store, fts, ports, &params),
        "search_pack" => exec_search_pack(store, fts, ports, &params),
        "read" => exec_read(store, &params),
        "list" => exec_list(store, &params),
        "remove" => exec_remove(store, fts, &params),
        "explain" => exec_explain(store, &params),
        "enrich" => exec_enrich(store, &params),
        "model_enrichment_propose" => exec_model_enrichment_propose(store, &params),
        "model_enrichment_resolve" => exec_model_enrichment_resolve(store, &params),
        "reindex" => exec_reindex(home, store),
        "reprocess" => exec_reprocess(home, store, fts, parser, ports.embedder, &params),
        "taxonomy_search" => exec_taxonomy_search(store, &params),
        "taxonomy_browse" => exec_taxonomy_browse(store, &params),
        "doctor" => exec_doctor(home),
        _ => Err(ShiroError::InvalidInput {
            message: format!("unknown operation: {op}"),
        }),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn str_param<'a>(params: &'a Value, key: &str) -> Result<&'a str, ShiroError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ShiroError::InvalidInput {
            message: format!("missing or invalid '{key}' parameter (expected string)"),
        })
}

fn u64_param(params: &Value, key: &str, default: u64) -> u64 {
    params.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
}

fn bool_param(params: &Value, key: &str, default: bool) -> bool {
    params.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn str_param_default<'a>(params: &'a Value, key: &str, default: &'a str) -> &'a str {
    params.get(key).and_then(|v| v.as_str()).unwrap_or(default)
}

fn string_list_param(params: &Value, key: &str) -> Result<Vec<String>, ShiroError> {
    match params.get(key) {
        None => Ok(Vec::new()),
        Some(Value::String(value)) => Ok(vec![value.clone()]),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| ShiroError::InvalidInput {
                        message: format!("invalid '{key}' filter (expected string array)"),
                    })
            })
            .collect(),
        Some(_) => Err(ShiroError::InvalidInput {
            message: format!("invalid '{key}' filter (expected string or string array)"),
        }),
    }
}

fn to_json<T: serde::Serialize>(val: T) -> Result<Value, ShiroError> {
    serde_json::to_value(val).map_err(|e| ShiroError::InvalidInput {
        message: format!("serialization failed: {e}"),
    })
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

fn exec_acquire(store: &Store, fts: &FtsIndex, params: &Value) -> Result<Value, ShiroError> {
    let input: ops::acquire::AcquireUrlInput =
        serde_json::from_value(params.clone()).map_err(|error| ShiroError::InvalidInput {
            message: format!("invalid acquire_url parameters: {error}"),
        })?;
    to_json(ops::acquire::execute(store, fts, &input)?)
}

fn exec_add(
    store: &Store,
    fts: &FtsIndex,
    parser: &dyn Parser,
    params: &Value,
) -> Result<Value, ShiroError> {
    let path = str_param(params, "path")?;
    let input = ops::add::AddInput {
        path: path.to_string(),
    };
    to_json(ops::add::execute(store, fts, parser, &input)?)
}

fn exec_ingest(
    store: &Store,
    fts: &FtsIndex,
    parser: &dyn Parser,
    params: &Value,
) -> Result<Value, ShiroError> {
    let dirs = params
        .get("dirs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| ShiroError::InvalidInput {
            message: "missing or invalid 'dirs' parameter (expected string array)".into(),
        })?;
    let max_files = params
        .get("max_files")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let input = ops::ingest::IngestInput { dirs, max_files };
    to_json(ops::ingest::execute(store, fts, parser, &input, None)?)
}

fn exec_search(
    store: &Store,
    fts: &FtsIndex,
    ports: ExecutorPorts<'_>,
    params: &Value,
) -> Result<Value, ShiroError> {
    let query = str_param(params, "query")?;
    let limit = u64_param(params, "limit", 10) as usize;
    let expand = bool_param(params, "expand", false);
    let rerank = bool_param(params, "rerank", false);
    let mode = match str_param_default(params, "mode", "hybrid") {
        "hybrid" => ops::search::SearchMode::Hybrid,
        "bm25" => ops::search::SearchMode::Bm25,
        "vector" => ops::search::SearchMode::Vector,
        other => {
            return Err(ShiroError::InvalidInput {
                message: format!("invalid search mode '{other}'"),
            });
        }
    };
    let input = ops::search::SearchInput {
        query: query.to_string(),
        mode,
        limit,
        expand,
        max_blocks: 12,
        max_chars: 8000,
        rerank,
        filters: ops::search::SearchFilters {
            tags: string_list_param(params, "tags")?,
            concept_ids: string_list_param(params, "concept_ids")?,
            document_ids: string_list_param(params, "document_ids")?,
        },
    };
    to_json(ops::search::execute(
        store,
        fts,
        ports.embedder,
        ports.vector_index,
        ports.reranker,
        &input,
    )?)
}
fn exec_search_pack(
    store: &Store,
    fts: &FtsIndex,
    ports: ExecutorPorts<'_>,
    params: &Value,
) -> Result<Value, ShiroError> {
    let mut normalized = params.clone();
    if let Some(mode) = normalized.get_mut("mode") {
        if let Some(value) = mode.as_str() {
            *mode = Value::String(
                match value {
                    "hybrid" => "Hybrid",
                    "bm25" => "Bm25",
                    "vector" => "Vector",
                    other => other,
                }
                .to_string(),
            );
        }
    }
    let input: ops::search_pack::SearchPackInput =
        serde_json::from_value(normalized).map_err(|error| ShiroError::InvalidInput {
            message: format!("invalid search_pack parameters: {error}"),
        })?;
    to_json(ops::search_pack::execute(
        store,
        fts,
        ports.embedder,
        ports.vector_index,
        ports.reranker,
        &input,
    )?)
}

fn exec_reprocess(
    home: &ShiroHome,
    store: &Store,
    fts: &FtsIndex,
    parser: &dyn Parser,
    embedder: Option<&dyn shiro_core::ports::Embedder>,
    params: &Value,
) -> Result<Value, ShiroError> {
    let input: ops::reprocess::ReprocessInput =
        serde_json::from_value(params.clone()).map_err(|error| ShiroError::InvalidInput {
            message: format!("invalid reprocess parameters: {error}"),
        })?;
    to_json(ops::reprocess::execute(
        home, store, fts, parser, embedder, &input,
    )?)
}

fn exec_model_enrichment_propose(store: &Store, params: &Value) -> Result<Value, ShiroError> {
    let input: ops::model_enrichment::ModelEnrichmentProposalInput =
        serde_json::from_value(params.clone()).map_err(|error| ShiroError::InvalidInput {
            message: format!("invalid model_enrichment_propose parameters: {error}"),
        })?;
    to_json(ops::model_enrichment::propose(store, &input)?)
}

fn exec_model_enrichment_resolve(store: &Store, params: &Value) -> Result<Value, ShiroError> {
    let input: ops::model_enrichment::ModelEnrichmentResolutionInput =
        serde_json::from_value(params.clone()).map_err(|error| ShiroError::InvalidInput {
            message: format!("invalid model_enrichment_resolve parameters: {error}"),
        })?;
    to_json(ops::model_enrichment::resolve(store, &input)?)
}

fn exec_taxonomy_search(store: &Store, params: &Value) -> Result<Value, ShiroError> {
    let input: ops::taxonomy::TaxonomySearchInput = serde_json::from_value(params.clone())
        .map_err(|error| ShiroError::InvalidInput {
            message: format!("invalid taxonomy_search parameters: {error}"),
        })?;
    to_json(ops::taxonomy::search(store, &input)?)
}

fn exec_taxonomy_browse(store: &Store, params: &Value) -> Result<Value, ShiroError> {
    let input: ops::taxonomy::TaxonomyBrowseInput = serde_json::from_value(params.clone())
        .map_err(|error| ShiroError::InvalidInput {
            message: format!("invalid taxonomy_browse parameters: {error}"),
        })?;
    to_json(ops::taxonomy::browse(store, &input)?)
}

fn exec_read(store: &Store, params: &Value) -> Result<Value, ShiroError> {
    let id = str_param(params, "id")?;
    let page = params
        .get("page")
        .and_then(Value::as_u64)
        .map(u32::try_from)
        .transpose()
        .map_err(|_| ShiroError::InvalidInput {
            message: "page exceeds the supported range".to_string(),
        })?;
    let mode = if page.is_some() {
        ops::read::ReadMode::Page
    } else {
        match params.get("mode").and_then(Value::as_str).unwrap_or("text") {
            "text" => ops::read::ReadMode::Text,
            "blocks" => ops::read::ReadMode::Blocks,
            "outline" => ops::read::ReadMode::Outline,
            other => {
                return Err(ShiroError::InvalidInput {
                    message: format!("unknown read mode: {other}"),
                });
            }
        }
    };
    let input = ops::read::ReadInput {
        id: id.to_string(),
        mode,
        page,
    };
    to_json(ops::read::execute(store, &input)?)
}

fn exec_list(store: &Store, params: &Value) -> Result<Value, ShiroError> {
    let limit = u64_param(params, "limit", 100) as usize;
    let input = ops::list::ListInput {
        limit,
        filters: ops::search::SearchFilters {
            tags: string_list_param(params, "tags")?,
            concept_ids: string_list_param(params, "concept_ids")?,
            document_ids: string_list_param(params, "document_ids")?,
        },
    };
    to_json(ops::list::execute(store, &input)?)
}

fn exec_remove(store: &Store, fts: &FtsIndex, params: &Value) -> Result<Value, ShiroError> {
    let id = str_param(params, "id")?;
    let input = ops::remove::RemoveInput {
        id: id.to_string(),
        purge: true,
    };
    to_json(ops::remove::execute(store, Some(fts), &input)?)
}

fn exec_explain(store: &Store, params: &Value) -> Result<Value, ShiroError> {
    let result_id = str_param(params, "result_id")?;
    let input = ops::explain::ExplainInput {
        result_id: result_id.to_string(),
    };
    to_json(ops::explain::execute(store, &input)?)
}

fn exec_enrich(store: &Store, params: &Value) -> Result<Value, ShiroError> {
    let id = str_param(params, "id")?;
    let input = ops::enrich::EnrichInput {
        doc_id: id.to_string(),
    };
    to_json(ops::enrich::execute(store, &input)?)
}

fn exec_reindex(home: &ShiroHome, store: &Store) -> Result<Value, ShiroError> {
    to_json(ops::reindex::execute(home, store)?)
}

fn exec_doctor(home: &ShiroHome) -> Result<Value, ShiroError> {
    let input = ops::doctor::DoctorInput {
        verify_vector: false,
    };
    to_json(ops::doctor::execute(home, &input)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_home(name: &str) -> ShiroHome {
        let dir = std::env::temp_dir().join(format!("shiro-exec-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        ShiroHome::new(camino::Utf8PathBuf::try_from(dir).unwrap())
    }

    #[test]
    fn missing_op_returns_error() {
        let home = test_home("missing-op");
        let store = Store::open(&home.db_path()).unwrap();
        let fts = FtsIndex::open(&home.tantivy_dir()).unwrap();
        let parser = shiro_parse::MarkdownParser;

        let program = serde_json::json!({});
        let err = execute(&home, &store, &fts, &parser, &program).unwrap_err();
        assert!(err.to_string().contains("missing 'op'"), "got: {err}");
    }

    #[test]
    fn unknown_op_returns_error() {
        let home = test_home("unknown-op");
        let store = Store::open(&home.db_path()).unwrap();
        let fts = FtsIndex::open(&home.tantivy_dir()).unwrap();
        let parser = shiro_parse::MarkdownParser;

        let program = serde_json::json!({"op": "nonexistent"});
        let err = execute(&home, &store, &fts, &parser, &program).unwrap_err();
        assert!(err.to_string().contains("unknown operation"), "got: {err}");
    }

    #[test]
    fn list_op_works_on_empty_store() {
        let home = test_home("list-empty");
        let store = Store::open(&home.db_path()).unwrap();
        let fts = FtsIndex::open(&home.tantivy_dir()).unwrap();
        let parser = shiro_parse::MarkdownParser;

        let program = serde_json::json!({"op": "list", "params": {"limit": 10}});
        let result = execute(&home, &store, &fts, &parser, &program).unwrap();
        assert!(result.get("documents").is_some());
    }
}
