//! SDK executor — dispatches JSON "programs" to typed SDK operations.
//!
//! Used by Code Mode MCP: `execute(program)` calls this to run an operation.
//! A "program" is `{ "op": "<name>", "params": { ... } }`.

use serde_json::Value;
use shiro_core::ports::Parser;
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_store::Store;

use crate::ops;

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
        "add" => exec_add(store, fts, parser, &params),
        "ingest" => exec_ingest(store, fts, parser, &params),
        "search" => exec_search(store, fts, &params),
        "read" => exec_read(store, &params),
        "list" => exec_list(store, &params),
        "remove" => exec_remove(store, fts, &params),
        "explain" => exec_explain(store, &params),
        "enrich" => exec_enrich(store, &params),
        "reindex" => exec_reindex(home, store),
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

fn to_json<T: serde::Serialize>(val: T) -> Result<Value, ShiroError> {
    serde_json::to_value(val).map_err(|e| ShiroError::InvalidInput {
        message: format!("serialization failed: {e}"),
    })
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

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

fn exec_search(store: &Store, fts: &FtsIndex, params: &Value) -> Result<Value, ShiroError> {
    let query = str_param(params, "query")?;
    let limit = u64_param(params, "limit", 10) as usize;
    let expand = bool_param(params, "expand", false);
    let input = ops::search::SearchInput {
        query: query.to_string(),
        mode: ops::search::SearchMode::Bm25,
        limit,
        expand,
        max_blocks: 12,
        max_chars: 8000,
    };
    to_json(ops::search::execute(store, fts, &input)?)
}

fn exec_read(store: &Store, params: &Value) -> Result<Value, ShiroError> {
    let id = str_param(params, "id")?;
    let input = ops::read::ReadInput {
        id: id.to_string(),
        mode: ops::read::ReadMode::Text,
    };
    to_json(ops::read::execute(store, &input)?)
}

fn exec_list(store: &Store, params: &Value) -> Result<Value, ShiroError> {
    let limit = u64_param(params, "limit", 100) as usize;
    let input = ops::list::ListInput { limit };
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
