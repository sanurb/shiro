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
    /// Authority class used by MCP host policy.
    pub authority: &'static str,
    /// Relevance score (higher = better match). Deterministic for same query.
    pub score: u32,
}

// ---------------------------------------------------------------------------
// Static parameter specs
// ---------------------------------------------------------------------------

static ACQUIRE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "url",
        description: "HTTPS URL for a PDF, Markdown, or UTF-8 text source",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "parser",
        description: "Parser selection: auto, plaintext, markdown, or pdf",
        r#type: "string",
        required: false,
    },
    ParamSpec {
        name: "max_bytes",
        description: "Maximum response bytes",
        r#type: "u64",
        required: false,
    },
    ParamSpec {
        name: "timeout_ms",
        description: "End-to-end acquisition timeout",
        r#type: "u64",
        required: false,
    },
];

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
    ParamSpec {
        name: "tags",
        description: "Tag filters (OR within tags, AND with other filter fields)",
        r#type: "string[]",
        required: false,
    },
    ParamSpec {
        name: "concept_ids",
        description: "Concept ID filters (OR within concepts, AND across fields)",
        r#type: "string[]",
        required: false,
    },
    ParamSpec {
        name: "document_ids",
        description: "Document ID filters (OR within documents, AND across fields)",
        r#type: "string[]",
        required: false,
    },
];

static READ_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "id",
        description: "Document ID, stable evidence handle, or title prefix to read",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "mode",
        description: "Read mode: text, blocks, or outline",
        r#type: "string",
        required: false,
    },
    ParamSpec {
        name: "page",
        description: "One-based source page to read as canonical blocks",
        r#type: "u64",
        required: false,
    },
];

static SEARCH_PACK_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "queries",
        description: "Query objects with unique query_id and text fields",
        r#type: "object[]",
        required: true,
    },
    ParamSpec {
        name: "mode",
        description: "Search mode: hybrid, bm25, or vector",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "per_query_limit",
        description: "Maximum ranked candidates per query",
        r#type: "u64",
        required: true,
    },
    ParamSpec {
        name: "global_limit",
        description: "Maximum deduplicated evidence handles",
        r#type: "u64",
        required: true,
    },
    ParamSpec {
        name: "include_content",
        description: "Include snippets and context blocks (default false)",
        r#type: "boolean",
        required: true,
    },
];

static EXPLAIN_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "result_id",
    description: "Result ID from a previous search",
    r#type: "string",
    required: true,
}];

static LIST_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "limit",
        description: "Maximum documents to list (default: 100)",
        r#type: "u64",
        required: false,
    },
    ParamSpec {
        name: "tags",
        description: "Tag filters (OR within tags, AND with concepts)",
        r#type: "string[]",
        required: false,
    },
    ParamSpec {
        name: "concept_ids",
        description: "Concept ID filters (OR within concepts, AND with tags)",
        r#type: "string[]",
        required: false,
    },
];

static MODEL_ENRICHMENT_PROPOSE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "doc_id",
        description: "READY document to enrich",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "provider",
        description: "Attributed model provider",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "consent_id",
        description: "Explicit consent or policy reference",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "concepts",
        description: "Proposed concepts with mandatory text labels",
        r#type: "object[]",
        required: true,
    },
];

static MODEL_ENRICHMENT_RESOLVE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "proposal_id",
        description: "Proposal to promote or reject",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "action",
        description: "Resolution action: promote or reject",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "resolved_actor_id",
        description: "Human or policy actor authorizing resolution",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "approval_id",
        description: "Approval reference",
        r#type: "string",
        required: true,
    },
];

static TAXONOMY_SEARCH_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "query",
        description: "Label, synonym, definition, or scheme text",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "limit",
        description: "Maximum matching concepts",
        r#type: "u64",
        required: true,
    },
];

static TAXONOMY_BROWSE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "root_concept_id",
        description: "Optional concept root; omitted lists concepts",
        r#type: "string",
        required: false,
    },
    ParamSpec {
        name: "max_depth",
        description: "Maximum relation depth",
        r#type: "u64",
        required: true,
    },
    ParamSpec {
        name: "max_nodes",
        description: "Maximum returned concepts",
        r#type: "u64",
        required: true,
    },
];

static REPROCESS_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "document_ids",
        description: "Optional document IDs; empty selects all READY documents",
        r#type: "string[]",
        required: false,
    },
    ParamSpec {
        name: "target",
        description: "Reprocessing target: parse, derived, or all",
        r#type: "string",
        required: true,
    },
    ParamSpec {
        name: "execute",
        description: "Execute the bounded plan; false returns a dry run",
        r#type: "boolean",
        required: false,
    },
    ParamSpec {
        name: "resume_manifest_id",
        description: "Require this verified active rollback manifest before execution",
        r#type: "string",
        required: false,
    },
];

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
        name: "acquire_url",
        description: "Safely acquire a bounded remote PDF or UTF-8 text source with provenance",
        params: ACQUIRE_PARAMS,
        returns: "AcquireUrlOutput { doc_id, final_url, redirects, signature, content_hash }",
        input_schema_ref: "AcquireUrlInput",
        output_schema_ref: "AcquireUrlOutput",
        example: r#"{"type":"call","op":"acquire_url","params":{"url":"https://example.com/paper.pdf","parser":"auto"}}"#,
    },
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
        returns: "IngestOutput { added, ready, failed, failures[], concept_proposals[] }",
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
        name: "model_enrichment_propose",
        description: "Store attributed model concepts as isolated reversible proposals",
        params: MODEL_ENRICHMENT_PROPOSE_PARAMS,
        returns: "ModelEnrichmentProposalOutput { proposal_id, status, trust_zone }",
        input_schema_ref: "ModelEnrichmentProposalInput",
        output_schema_ref: "ModelEnrichmentProposalOutput",
        example: r#"{"type":"call","op":"model_enrichment_propose","params":{"doc_id":"doc_...","provider":"provider","model":"model","actor_id":"agent","data_region":"local","retention_policy":"none","consent_id":"approval","concepts":[{"scheme_uri":"urn:topics","pref_label":"Topic","confidence":0.9}]}}"#,
    },
    OpSpec {
        name: "model_enrichment_resolve",
        description: "Explicitly promote or reject and reverse a model-enrichment proposal",
        params: MODEL_ENRICHMENT_RESOLVE_PARAMS,
        returns: "ModelEnrichmentResolutionOutput { proposal_id, status, applied_concept_ids }",
        input_schema_ref: "ModelEnrichmentResolutionInput",
        output_schema_ref: "ModelEnrichmentResolutionOutput",
        example: r#"{"type":"call","op":"model_enrichment_resolve","params":{"proposal_id":"proposal_...","action":"promote","resolved_actor_id":"local_user","approval_id":"approval"}}"#,
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
        name: "reprocess",
        description: "Plan or execute scoped bounded reprocessing from persisted source artifacts",
        params: REPROCESS_PARAMS,
        returns: "ReprocessOutput { status, plan, publication }",
        input_schema_ref: "ReprocessInput",
        output_schema_ref: "ReprocessOutput",
        example: r#"{"type":"call","op":"reprocess","params":{"document_ids":[],"target":"derived","execute":false}}"#,
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
    OpSpec {
        name: "search_pack",
        description: "Run multiple queries and deduplicate results by stable evidence handle",
        params: SEARCH_PACK_PARAMS,
        returns: "SearchPackOutput { query_count, unique_evidence_count, mode, hits[] }",
        input_schema_ref: "SearchPackInput",
        output_schema_ref: "SearchPackOutput",
        example: r#"{"type":"call","op":"search_pack","params":{"queries":[{"query_id":"q1","text":"error handling"}],"mode":"bm25","per_query_limit":5,"global_limit":10,"include_content":false,"max_blocks":12,"max_chars":8000,"rerank":false}}"#,
    },
    OpSpec {
        name: "taxonomy_browse",
        description: "Browse a bounded SKOS relation graph with text fallback for every concept",
        params: TAXONOMY_BROWSE_PARAMS,
        returns: "TaxonomyBrowseOutput { root_concept_id, truncated, concepts, relations }",
        input_schema_ref: "TaxonomyBrowseInput",
        output_schema_ref: "TaxonomyBrowseOutput",
        example: r#"{"type":"call","op":"taxonomy_browse","params":{"max_depth":2,"max_nodes":50}}"#,
    },
    OpSpec {
        name: "taxonomy_search",
        description: "Search taxonomy labels, synonyms, definitions, and schemes",
        params: TAXONOMY_SEARCH_PARAMS,
        returns: "TaxonomySearchOutput { concepts }",
        input_schema_ref: "TaxonomySearchInput",
        output_schema_ref: "TaxonomySearchOutput",
        example: r#"{"type":"call","op":"taxonomy_search","params":{"query":"retrieval","limit":20}}"#,
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
                    authority: operation_authority(op.name),
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

/// Return the authority class for one operation.
pub fn operation_authority(operation: &str) -> &'static str {
    match operation {
        "acquire_url"
        | "add"
        | "ingest"
        | "remove"
        | "enrich"
        | "model_enrichment_propose"
        | "model_enrichment_resolve"
        | "reindex"
        | "reprocess" => "write",
        _ => "read",
    }
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

    add_schema!(crate::ops::acquire::AcquireUrlInput);
    add_schema!(crate::ops::acquire::AcquireUrlOutput);
    add_schema!(crate::ops::add::AddInput);
    add_schema!(crate::ops::add::AddOutput);
    add_schema!(crate::ops::benchmark::BenchmarkManifest);
    add_schema!(crate::ops::benchmark::BenchmarkOutput);
    add_schema!(crate::ops::ingest::IngestInput);
    add_schema!(crate::ops::ingest::IngestOutput);
    add_schema!(crate::ops::search::SearchInput);
    add_schema!(crate::ops::search::SearchOutput);
    add_schema!(crate::ops::search_pack::SearchPackInput);
    add_schema!(crate::ops::search_pack::SearchPackOutput);
    add_schema!(crate::ops::reprocess::ReprocessInput);
    add_schema!(crate::ops::reprocess::ReprocessOutput);
    add_schema!(crate::ops::taxonomy::TaxonomyBrowseInput);
    add_schema!(crate::ops::taxonomy::TaxonomyBrowseOutput);
    add_schema!(crate::ops::taxonomy::TaxonomySearchInput);
    add_schema!(crate::ops::taxonomy::TaxonomySearchOutput);
    add_schema!(crate::ops::model_enrichment::ModelEnrichmentProposalInput);
    add_schema!(crate::ops::model_enrichment::ModelEnrichmentProposalOutput);
    add_schema!(crate::ops::model_enrichment::ModelEnrichmentResolutionInput);
    add_schema!(crate::ops::model_enrichment::ModelEnrichmentResolutionOutput);
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
        assert_eq!(OPS.len(), 17, "expected 17 SDK operations");
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
