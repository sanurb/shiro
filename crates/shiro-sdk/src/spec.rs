//! SDK spec registry — machine-readable descriptions of every SDK operation.
//!
//! Used by Code Mode MCP: `shiro.search(query)` queries this index to discover
//! available operations, their input/output schemas, and examples.
//!
//! The index is a static, sorted array of [`OpSpec`] entries. Search results are
//! deterministically ordered by relevance score (desc), then by name (asc).

use serde::Serialize;

/// Description of a single SDK operation.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct OpSpec {
    /// Operation name (e.g. "search", "read").
    pub name: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Input parameters.
    pub params: &'static [ParamSpec],
    /// Description of the return type.
    pub returns: &'static str,
    /// JSON Schema ref for the input type (generated from schemars).
    pub input_schema_ref: &'static str,
    /// JSON Schema ref for the output type (generated from schemars).
    pub output_schema_ref: &'static str,
    /// Minimal usage example as a JSON program snippet.
    pub example: &'static str,
}

/// Description of an SDK operation parameter.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ParamSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub r#type: &'static str,
    pub required: bool,
}

/// A search result with relevance score.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SpecSearchResult {
    /// The matched operation spec.
    pub spec: &'static OpSpec,
    /// Relevance score (higher = better match). Deterministic for same query.
    pub score: u32,
}

// ---------------------------------------------------------------------------
// Static parameter specs
// ---------------------------------------------------------------------------

static ADD_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "path",
    description: "Path to the file to add (Markdown or PDF)",
    r#type: "string",
    required: true,
}];

static INGEST_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "dirs",
        description: "Directories to scan for documents",
        r#type: "string[]",
        required: true,
    },
    ParamSpec {
        name: "max_files",
        description: "Maximum files to process (0 = unlimited)",
        r#type: "u64",
        required: false,
    },
];

static SEARCH_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "query",
        description: "Search query text",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "limit",
        description: "Maximum results to return (default: 10)",
        r#type: "u64",
        required: false,
    },
    ParamSpec {
        name: "expand",
        description: "Expand results with surrounding context",
        r#type: "bool",
        required: false,
    },
];

static READ_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "id",
    description: "Document ID or title prefix to read",
    r#type: "string",
    required: true,
}];

static EXPLAIN_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "result_id",
    description: "Result ID from a previous search",
    r#type: "string",
    required: true,
}];

static LIST_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "limit",
    description: "Maximum documents to list (default: 100)",
    r#type: "u64",
    required: false,
}];

static REMOVE_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "id",
    description: "Document ID or title prefix to remove",
    r#type: "string",
    required: true,
}];

static ENRICH_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "id",
    description: "Document ID or title prefix to enrich with heuristic metadata",
    r#type: "string",
    required: true,
}];

static REINDEX_PARAMS: &[ParamSpec] = &[];
static DOCTOR_PARAMS: &[ParamSpec] = &[];

// ---------------------------------------------------------------------------
// Registry (sorted by name for deterministic ordering)
// ---------------------------------------------------------------------------

/// All available SDK operations, sorted by name.
pub static OPS: &[OpSpec] = &[
    OpSpec {
        name: "add",
        description: "Add a single file (Markdown or PDF) to the knowledge base",
        params: ADD_PARAMS,
        returns: "AddOutput { doc_id, status, title, segments, changed }",
        input_schema_ref: "AddInput",
        output_schema_ref: "AddOutput",
        example: r#"{"type":"call","op":"add","params":{"path":"/docs/readme.md"}}"#,
    },
    OpSpec {
        name: "doctor",
        description: "Run health checks on the knowledge base",
        params: DOCTOR_PARAMS,
        returns: "DoctorOutput { checks[], healthy }",
        input_schema_ref: "DoctorInput",
        output_schema_ref: "DoctorOutput",
        example: r#"{"type":"call","op":"doctor","params":{}}"#,
    },
    OpSpec {
        name: "enrich",
        description: "Enrich a document with heuristic metadata (title, summary, tags)",
        params: ENRICH_PARAMS,
        returns: "EnrichOutput { doc_id, title, summary_length, tags[] }",
        input_schema_ref: "EnrichInput",
        output_schema_ref: "EnrichOutput",
        example: r#"{"type":"let","name":"meta","call":{"op":"enrich","params":{"id":"doc_abc123"}}}"#,
    },
    OpSpec {
        name: "explain",
        description: "Explain why a search result was ranked as it was",
        params: EXPLAIN_PARAMS,
        returns:
            "ExplainOutput { result_id, query, doc_id, block_idx, block_kind, retrieval_trace }",
        input_schema_ref: "ExplainInput",
        output_schema_ref: "ExplainOutput",
        example: r#"{"type":"let","name":"trace","call":{"op":"explain","params":{"result_id":"res_abc123"}}}"#,
    },
    OpSpec {
        name: "ingest",
        description: "Batch-scan directories and add all supported documents",
        params: INGEST_PARAMS,
        returns: "IngestOutput { added, ready, failed, failures[] }",
        input_schema_ref: "IngestInput",
        output_schema_ref: "IngestOutput",
        example: r#"{"type":"let","name":"batch","call":{"op":"ingest","params":{"dirs":["/docs"]}}}"#,
    },
    OpSpec {
        name: "list",
        description: "List all documents in the knowledge base",
        params: LIST_PARAMS,
        returns: "ListOutput { documents[], truncated }",
        input_schema_ref: "ListInput",
        output_schema_ref: "ListOutput",
        example: r#"{"type":"let","name":"docs","call":{"op":"list","params":{"limit":20}}}"#,
    },
    OpSpec {
        name: "read",
        description: "Read the full content or segments of a document",
        params: READ_PARAMS,
        returns: "ReadOutput { doc_id, title, state, content }",
        input_schema_ref: "ReadInput",
        output_schema_ref: "ReadOutput",
        example: r#"{"type":"let","name":"doc","call":{"op":"read","params":{"id":"doc_abc123"}}}"#,
    },
    OpSpec {
        name: "reindex",
        description: "Rebuild the FTS index from all stored segments",
        params: REINDEX_PARAMS,
        returns: "ReindexOutput { index, status, documents, segments, generation }",
        input_schema_ref: "(none)",
        output_schema_ref: "ReindexOutput",
        example: r#"{"type":"call","op":"reindex","params":{}}"#,
    },
    OpSpec {
        name: "remove",
        description: "Remove a document from the knowledge base",
        params: REMOVE_PARAMS,
        returns: "RemoveOutput { doc_id, previous_state }",
        input_schema_ref: "RemoveInput",
        output_schema_ref: "RemoveOutput",
        example: r#"{"type":"call","op":"remove","params":{"id":"doc_abc123"}}"#,
    },
    OpSpec {
        name: "search",
        description: "Search documents using BM25 full-text search with optional context expansion",
        params: SEARCH_PARAMS,
        returns: "SearchOutput { query, mode, fts_generation, hits[] }",
        input_schema_ref: "SearchInput",
        output_schema_ref: "SearchOutput",
        example: r#"{"type":"let","name":"results","call":{"op":"search","params":{"query":"error handling","limit":5}}}"#,
    },
];

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Search the spec registry by keyword. Returns operations whose name,
/// description, or parameter names/descriptions match the query.
///
/// Results are deterministically ordered: by score (desc), then by name (asc).
/// Empty query returns all ops with equal score.
pub fn search_specs(query: &str, limit: usize) -> Vec<SpecSearchResult> {
    let q = query.to_lowercase();
    let terms: Vec<&str> = q.split_whitespace().collect();

    let mut results: Vec<SpecSearchResult> = OPS
        .iter()
        .filter_map(|op| {
            let score = score_op(op, &terms);
            if score > 0 || terms.is_empty() {
                Some(SpecSearchResult {
                    spec: op,
                    score: if terms.is_empty() { 1 } else { score },
                })
            } else {
                None
            }
        })
        .collect();

    // Deterministic sort: score desc, then name asc
    results.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.spec.name.cmp(b.spec.name))
    });

    results.truncate(limit);
    results
}

/// Score an operation against search terms.
/// Higher score = better match. Returns 0 if no match.
fn score_op(op: &OpSpec, terms: &[&str]) -> u32 {
    let mut total = 0u32;
    for term in terms {
        let mut term_score = 0u32;
        // Exact name match: highest weight
        if op.name == *term {
            term_score += 100;
        } else if op.name.contains(term) {
            term_score += 50;
        }
        // Description match
        if op.description.to_lowercase().contains(term) {
            term_score += 10;
        }
        // Parameter match
        for p in op.params {
            if p.name.contains(term) {
                term_score += 20;
            }
            if p.description.to_lowercase().contains(term) {
                term_score += 5;
            }
        }
        // Returns match
        if op.returns.to_lowercase().contains(term) {
            term_score += 5;
        }
        if term_score == 0 {
            return 0; // All terms must match (AND semantics)
        }
        total += term_score;
    }
    total
}

// ---------------------------------------------------------------------------
// Schema generation
// ---------------------------------------------------------------------------

/// Generate JSON Schemas for all SDK input/output types.
///
/// Returns a JSON object mapping type name → JSON Schema.
pub fn generate_schemas() -> serde_json::Value {
    let mut schemas = serde_json::Map::new();

    macro_rules! add_schema {
        ($t:ty) => {
            let schema = schemars::schema_for!($t);
            schemas.insert(
                stringify!($t)
                    .rsplit("::")
                    .next()
                    .unwrap_or(stringify!($t))
                    .to_string(),
                serde_json::to_value(schema).unwrap_or_default(),
            );
        };
    }

    add_schema!(crate::ops::add::AddInput);
    add_schema!(crate::ops::add::AddOutput);
    add_schema!(crate::ops::ingest::IngestInput);
    add_schema!(crate::ops::ingest::IngestOutput);
    add_schema!(crate::ops::search::SearchInput);
    add_schema!(crate::ops::search::SearchOutput);
    add_schema!(crate::ops::read::ReadInput);
    add_schema!(crate::ops::read::ReadOutput);
    add_schema!(crate::ops::explain::ExplainInput);
    add_schema!(crate::ops::explain::ExplainOutput);
    add_schema!(crate::ops::list::ListInput);
    add_schema!(crate::ops::list::ListOutput);
    add_schema!(crate::ops::remove::RemoveInput);
    add_schema!(crate::ops::remove::RemoveOutput);
    add_schema!(crate::ops::enrich::EnrichInput);
    add_schema!(crate::ops::enrich::EnrichOutput);
    add_schema!(crate::ops::reindex::ReindexOutput);
    add_schema!(crate::ops::doctor::DoctorInput);
    add_schema!(crate::ops::doctor::DoctorOutput);
    add_schema!(crate::dsl::Node);
    add_schema!(crate::dsl::Limits);
    add_schema!(crate::dsl::ExecutionResult);

    serde_json::Value::Object(schemas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_sorted_by_name() {
        let names: Vec<&str> = OPS.iter().map(|op| op.name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "OPS must be sorted by name");
    }

    #[test]
    fn all_ops_have_required_fields() {
        for op in OPS {
            assert!(!op.name.is_empty(), "op has empty name");
            assert!(
                !op.description.is_empty(),
                "op {} has empty description",
                op.name
            );
            assert!(!op.returns.is_empty(), "op {} has empty returns", op.name);
            assert!(!op.example.is_empty(), "op {} has empty example", op.name);
            assert!(
                !op.input_schema_ref.is_empty(),
                "op {} has empty input_schema_ref",
                op.name
            );
            assert!(
                !op.output_schema_ref.is_empty(),
                "op {} has empty output_schema_ref",
                op.name
            );
        }
    }

    #[test]
    fn op_count_matches_sdk_surface() {
        assert_eq!(OPS.len(), 10, "expected 10 SDK operations");
    }

    #[test]
    fn search_specs_finds_by_exact_name() {
        let results = search_specs("search", 10);
        assert!(results.iter().any(|r| r.spec.name == "search"));
        // Exact name match should be first
        assert_eq!(results[0].spec.name, "search");
    }

    #[test]
    fn search_specs_finds_by_description() {
        let results = search_specs("knowledge base", 10);
        assert!(!results.is_empty());
    }

    #[test]
    fn search_specs_empty_query_returns_all() {
        let results = search_specs("", 100);
        assert_eq!(results.len(), OPS.len());
    }

    #[test]
    fn search_specs_deterministic() {
        let r1 = search_specs("document", 10);
        let r2 = search_specs("document", 10);
        let names1: Vec<&str> = r1.iter().map(|r| r.spec.name).collect();
        let names2: Vec<&str> = r2.iter().map(|r| r.spec.name).collect();
        assert_eq!(names1, names2, "search results must be deterministic");
    }

    #[test]
    fn search_specs_respects_limit() {
        let results = search_specs("", 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn search_specs_no_match_returns_empty() {
        let results = search_specs("zzzznonexistentzzz", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn generate_schemas_produces_valid_json() {
        let schemas = generate_schemas();
        assert!(schemas.is_object());
        let map = schemas.as_object().unwrap();
        // Should have all our types
        assert!(map.contains_key("AddInput"), "missing AddInput schema");
        assert!(
            map.contains_key("SearchOutput"),
            "missing SearchOutput schema"
        );
        assert!(map.contains_key("Node"), "missing Node schema");
        assert!(map.contains_key("Limits"), "missing Limits schema");
    }

    #[test]
    fn examples_are_valid_json() {
        for op in OPS {
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(op.example);
            assert!(
                parsed.is_ok(),
                "op {} has invalid JSON example: {}",
                op.name,
                op.example
            );
        }
    }
}
