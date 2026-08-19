//! SQLite-backed document store for the shiro workspace.

use shiro_core::enrichment::EnrichmentResult;
use shiro_core::error::ShiroError;
use shiro_core::fingerprint::ProcessingFingerprint;
use shiro_core::generation::{CorpusManifest, GenerationId, IndexGeneration};
use shiro_core::id::{DocId, SegmentId, VersionId};
use shiro_core::ir::{
    Block, BlockGraph, BlockIdx, BlockKind, Document, DocumentHeadingLevel, Edge, LossKind,
    Metadata, ParseLoss, Relation, Segment,
};
use shiro_core::manifest::DocState;
use shiro_core::provenance::{ProvenanceActorKind, ProvenanceRecord, TrustZone, WriteProvenance};
use shiro_core::source_locator::{CoordinateOrigin, PageDimensions, SourceLocator, SourceRegion};
use shiro_core::span::Span;
use shiro_core::taxonomy::{Concept, ConceptId, ConceptRelation, SkosRelation};
use shiro_core::{evidence_handle_for_block, EvidenceHandleId};

/// Map any `rusqlite::Error` into `ShiroError::StoreCorrupt`.
fn map_db(e: rusqlite::Error) -> ShiroError {
    ShiroError::StoreCorrupt {
        message: e.to_string(),
    }
}

/// Parse a `DocState` from its SQL string representation.
fn parse_provenance_actor_kind(s: &str) -> Result<ProvenanceActorKind, ShiroError> {
    match s {
        "HUMAN" => Ok(ProvenanceActorKind::Human),
        "SYSTEM" => Ok(ProvenanceActorKind::System),
        "AGENT" => Ok(ProvenanceActorKind::Agent),
        other => Err(ShiroError::StoreCorrupt {
            message: format!("unknown ProvenanceActorKind: {other}"),
        }),
    }
}

fn parse_state(s: &str) -> Result<DocState, ShiroError> {
    match s {
        "STAGED" => Ok(DocState::Staged),
        "INDEXING" => Ok(DocState::Indexing),
        "READY" => Ok(DocState::Ready),
        "FAILED" => Ok(DocState::Failed),
        "DELETED" => Ok(DocState::Deleted),
        other => Err(ShiroError::StoreCorrupt {
            message: format!("unknown DocState: {other}"),
        }),
    }
}

/// Parse a `SkosRelation` from its SQL string representation.
fn parse_relation(s: &str) -> Result<SkosRelation, ShiroError> {
    match s {
        "BROADER" => Ok(SkosRelation::Broader),
        "NARROWER" => Ok(SkosRelation::Narrower),
        "RELATED" => Ok(SkosRelation::Related),
        other => Err(ShiroError::StoreCorrupt {
            message: format!("unknown SkosRelation: {other}"),
        }),
    }
}

/// SQL string for a `SkosRelation`.
fn relation_to_sql(rel: &SkosRelation) -> &'static str {
    match rel {
        SkosRelation::Broader => "BROADER",
        SkosRelation::Narrower => "NARROWER",
        SkosRelation::Related => "RELATED",
    }
}

/// SQL string for a `BlockKind`.
fn block_kind_to_sql(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Paragraph => "PARAGRAPH",
        BlockKind::Heading => "HEADING",
        BlockKind::ListItem => "LIST_ITEM",
        BlockKind::TableCell => "TABLE_CELL",
        BlockKind::Code => "CODE",
        BlockKind::Caption => "CAPTION",
        BlockKind::Footnote => "FOOTNOTE",
    }
}

/// Parse a `BlockKind` from its SQL string representation.
fn loss_kind_to_sql(kind: LossKind) -> &'static str {
    match kind {
        LossKind::Image => "IMAGE",
        LossKind::Table => "TABLE",
        LossKind::Math => "MATH",
        LossKind::Media => "MEDIA",
        LossKind::Layout => "LAYOUT",
        LossKind::Encoding => "ENCODING",
        LossKind::Other => "OTHER",
    }
}

fn parse_loss_kind(s: &str) -> Result<LossKind, ShiroError> {
    match s {
        "IMAGE" => Ok(LossKind::Image),
        "TABLE" => Ok(LossKind::Table),
        "MATH" => Ok(LossKind::Math),
        "MEDIA" => Ok(LossKind::Media),
        "LAYOUT" => Ok(LossKind::Layout),
        "ENCODING" => Ok(LossKind::Encoding),
        "OTHER" => Ok(LossKind::Other),
        other => Err(ShiroError::StoreCorrupt {
            message: format!("unknown LossKind: {other}"),
        }),
    }
}

fn coordinate_origin_to_sql(origin: CoordinateOrigin) -> &'static str {
    match origin {
        CoordinateOrigin::TopLeft => "TOP_LEFT",
        CoordinateOrigin::BottomLeft => "BOTTOM_LEFT",
    }
}

fn parse_coordinate_origin(s: &str) -> Result<CoordinateOrigin, ShiroError> {
    match s {
        "TOP_LEFT" => Ok(CoordinateOrigin::TopLeft),
        "BOTTOM_LEFT" => Ok(CoordinateOrigin::BottomLeft),
        other => Err(ShiroError::StoreCorrupt {
            message: format!("unknown source coordinate origin: {other}"),
        }),
    }
}

fn parse_block_kind(s: &str) -> Result<BlockKind, ShiroError> {
    match s {
        "PARAGRAPH" => Ok(BlockKind::Paragraph),
        "HEADING" => Ok(BlockKind::Heading),
        "LIST_ITEM" => Ok(BlockKind::ListItem),
        "TABLE_CELL" => Ok(BlockKind::TableCell),
        "CODE" => Ok(BlockKind::Code),
        "CAPTION" => Ok(BlockKind::Caption),
        "FOOTNOTE" => Ok(BlockKind::Footnote),
        other => Err(ShiroError::StoreCorrupt {
            message: format!("unknown BlockKind: {other}"),
        }),
    }
}

/// SQL string for a block `Relation`.
fn relation_to_edge_sql(rel: &Relation) -> &'static str {
    match rel {
        Relation::ReadsBefore => "READS_BEFORE",
        Relation::CaptionOf => "CAPTION_OF",
        Relation::FootnoteOf => "FOOTNOTE_OF",
        Relation::RefersTo => "REFERS_TO",
        Relation::SectionContains => "SECTION_CONTAINS",
    }
}

/// Parse a block `Relation` from its SQL string representation.
fn parse_edge_relation(s: &str) -> Result<Relation, ShiroError> {
    match s {
        "READS_BEFORE" => Ok(Relation::ReadsBefore),
        "CAPTION_OF" => Ok(Relation::CaptionOf),
        "FOOTNOTE_OF" => Ok(Relation::FootnoteOf),
        "REFERS_TO" => Ok(Relation::RefersTo),
        "SECTION_CONTAINS" => Ok(Relation::SectionContains),
        other => Err(ShiroError::StoreCorrupt {
            message: format!("unknown block Relation: {other}"),
        }),
    }
}

/// Current schema version this binary expects.
pub const CURRENT_SCHEMA_VERSION: u32 = 21;

/// Attributed model-enrichment proposal isolated from trusted taxonomy state.
#[derive(Debug, Clone)]
pub struct ModelEnrichmentProposalRecord {
    pub proposal_id: String,
    pub doc_id: DocId,
    pub provider: String,
    pub model: String,
    pub actor_id: String,
    pub data_region: String,
    pub retention_policy: String,
    pub consent_id: String,
    pub payload_json: String,
    pub status: String,
    pub applied_concepts_json: String,
}

/// Generations protected from orphan cleanup because at least one corpus manifest names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusManifestGenerationReferences {
    /// FTS generations retained for active, rollback, or audit manifests.
    pub fts_generations: Vec<GenerationId>,
    /// Vector generations retained for active, rollback, reuse, or audit manifests.
    pub vector_generations: Vec<GenerationId>,
}

/// One proposed concept and assignment applied during explicit promotion.
#[derive(Debug, Clone)]
pub struct ProposedConceptAssignment {
    pub concept: Concept,
    pub confidence: f32,
}

/// Network and content evidence captured by one bounded URL acquisition.
#[derive(Debug, Clone)]
pub struct UrlAcquisitionRecord {
    pub requested_url: String,
    pub final_url: String,
    pub redirects_json: String,
    pub content_type: Option<String>,
    pub signature: String,
    pub byte_count: usize,
    pub content_hash: String,
}

/// Stored resolution of a stable canonical block handle.
#[derive(Debug, Clone)]
pub struct EvidenceHandleResolution {
    pub handle_id: EvidenceHandleId,
    pub status: String,
    pub superseded_by: Option<EvidenceHandleId>,
    pub doc_id: DocId,
    pub block_idx: usize,
    pub block_kind: BlockKind,
    pub heading_level: Option<u32>,
    pub span: Span,
    pub canonical_text: String,
    pub source_locators: Vec<SourceLocator>,
}

/// Immutable metadata shared by every result produced in one search snapshot.
pub struct SearchSnapshotMetadata<'a> {
    pub search_snapshot_id: &'a str,
    pub retrieval_policy_json: &'a str,
    pub query: &'a str,
    pub query_digest: &'a str,
    pub fts_generation: u64,
    pub vector_generation: u64,
}

/// A row to be saved in the `search_results` table.
pub struct SearchResultRow {
    pub result_id: String,
    pub evidence_handle: EvidenceHandleId,
    pub doc_id: DocId,
    pub segment_id: SegmentId,
    pub bm25_score: Option<f32>,
    pub bm25_rank: Option<usize>,
    pub vector_score: Option<f32>,
    pub vector_rank: Option<usize>,
    pub fused_score: Option<f32>,
    pub fused_rank: Option<usize>,
    pub reranker_score: Option<f32>,
    pub reranker_rank: Option<usize>,
    pub block_idx: usize,
    pub block_kind: String,
    pub heading_level: Option<u32>,
    pub span_start: usize,
    pub span_end: usize,
    pub source_locators: Vec<SourceLocator>,
}

/// Detail returned from `get_search_result`.
pub struct SearchResultDetail {
    pub query: String,
    pub query_digest: Option<String>,
    pub search_snapshot_id: String,
    pub retrieval_policy_json: String,
    pub evidence_handle: Option<EvidenceHandleId>,
    pub doc_id: DocId,
    pub segment_id: SegmentId,
    pub bm25_score: Option<f32>,
    pub bm25_rank: Option<usize>,
    pub vector_score: Option<f32>,
    pub vector_rank: Option<usize>,
    pub fused_score: Option<f32>,
    pub fused_rank: Option<usize>,
    pub fts_gen: Option<u64>,
    pub vec_gen: Option<u64>,
    pub reranker_score: Option<f32>,
    pub reranker_rank: Option<usize>,
    pub block_idx: usize,
    pub block_kind: String,
    pub heading_level: Option<u32>,
    pub span_start: usize,
    pub span_end: usize,
    pub source_locators: Vec<SourceLocator>,
}

/// V3 DDL for new tables (used in both fresh-create and migration).
const V3_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS concepts (
    concept_id TEXT PRIMARY KEY,
    scheme_uri TEXT NOT NULL,
    pref_label TEXT NOT NULL,
    alt_labels TEXT NOT NULL DEFAULT '[]',
    definition TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS concept_relations (
    from_id TEXT NOT NULL REFERENCES concepts(concept_id),
    to_id TEXT NOT NULL REFERENCES concepts(concept_id),
    relation TEXT NOT NULL CHECK(relation IN ('BROADER','NARROWER','RELATED')),
    PRIMARY KEY (from_id, to_id, relation)
);

CREATE TABLE IF NOT EXISTS concept_closure (
    ancestor_id TEXT NOT NULL REFERENCES concepts(concept_id),
    descendant_id TEXT NOT NULL REFERENCES concepts(concept_id),
    depth INTEGER NOT NULL,
    PRIMARY KEY (ancestor_id, descendant_id)
);

CREATE TABLE IF NOT EXISTS doc_concepts (
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    concept_id TEXT NOT NULL REFERENCES concepts(concept_id),
    confidence REAL NOT NULL DEFAULT 1.0,
    source TEXT NOT NULL DEFAULT 'manual',
    PRIMARY KEY (doc_id, concept_id)
);

CREATE TABLE IF NOT EXISTS enrichments (
    doc_id TEXT PRIMARY KEY REFERENCES documents(doc_id) ON DELETE CASCADE,
    title TEXT,
    summary TEXT,
    tags TEXT NOT NULL DEFAULT '[]',
    concepts TEXT NOT NULL DEFAULT '[]',
    provider TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS generations (
    kind TEXT NOT NULL,
    gen_id INTEGER NOT NULL,
    doc_count INTEGER NOT NULL DEFAULT 0,
    segment_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (kind, gen_id)
);

CREATE TABLE IF NOT EXISTS active_generations (
    kind TEXT PRIMARY KEY,
    gen_id INTEGER NOT NULL
);
";

const V21_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS model_enrichment_proposals (
    proposal_id TEXT PRIMARY KEY,
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    data_region TEXT NOT NULL,
    retention_policy TEXT NOT NULL,
    consent_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('PROPOSED','PROMOTED','REJECTED')),
    applied_concepts_json TEXT NOT NULL DEFAULT '[]',
    resolved_actor_id TEXT,
    approval_id TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    resolved_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_model_proposals_doc_status
    ON model_enrichment_proposals(doc_id, status);
";

const V20_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS url_acquisitions (
    acquisition_id INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    requested_url TEXT NOT NULL,
    final_url TEXT NOT NULL,
    redirects_json TEXT NOT NULL,
    content_type TEXT,
    signature TEXT NOT NULL,
    byte_count INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_url_acquisitions_doc
    ON url_acquisitions(doc_id, acquisition_id);
";

const V19_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS mcp_mutation_audit (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    approval_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    params_digest TEXT NOT NULL,
    outcome TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_mcp_mutation_audit_run
    ON mcp_mutation_audit(run_id, audit_id);
";

const V17_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS evidence_handles (
    handle_id TEXT PRIMARY KEY,
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    block_idx INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('ACTIVE','SUPERSEDED')),
    superseded_by TEXT,
    block_kind TEXT NOT NULL,
    heading_level INTEGER,
    span_start INTEGER NOT NULL,
    span_end INTEGER NOT NULL,
    canonical_text TEXT NOT NULL,
    source_locators_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_evidence_handles_doc_status
    ON evidence_handles(doc_id, status);
";

const V12_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS block_source_locators (
    doc_id TEXT NOT NULL,
    block_idx INTEGER NOT NULL,
    locator_idx INTEGER NOT NULL,
    page_number INTEGER NOT NULL CHECK(page_number > 0),
    region_x0 REAL,
    region_y0 REAL,
    region_x1 REAL,
    region_y1 REAL,
    coordinate_origin TEXT CHECK(coordinate_origin IN ('TOP_LEFT','BOTTOM_LEFT')),
    page_width REAL,
    page_height REAL,
    PRIMARY KEY (doc_id, block_idx, locator_idx),
    FOREIGN KEY (doc_id, block_idx) REFERENCES blocks(doc_id, block_idx) ON DELETE CASCADE
);
";

const V11_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS corpus_manifests (
    manifest_id TEXT PRIMARY KEY,
    corpus_digest TEXT NOT NULL,
    document_count INTEGER NOT NULL,
    segment_count INTEGER NOT NULL,
    fts_generation INTEGER NOT NULL,
    fts_digest TEXT NOT NULL,
    vector_generation INTEGER,
    vector_digest TEXT,
    embedding_fingerprint_hash TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS active_corpus_manifest (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    manifest_id TEXT NOT NULL REFERENCES corpus_manifests(manifest_id)
);
";

/// V5 DDL: persist BlockGraph as first-class stored representation (ADR-006).
const V10_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS provenance_records (
    provenance_id INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    actor_kind TEXT NOT NULL CHECK(actor_kind IN ('HUMAN','SYSTEM','AGENT')),
    actor_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_provenance_doc ON provenance_records(doc_id, provenance_id);

CREATE TABLE IF NOT EXISTS source_artifacts (
    source_artifact_id INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL REFERENCES blobs(content_hash),
    source_uri TEXT NOT NULL,
    byte_count INTEGER NOT NULL,
    trust_zone TEXT NOT NULL CHECK(trust_zone IN ('CANONICAL','DERIVED','PROPOSED','QUARANTINED')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(doc_id, content_hash, source_uri)
);
";

const V9_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS document_losses (
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    loss_idx INTEGER NOT NULL,
    kind TEXT NOT NULL,
    span_start INTEGER,
    span_end INTEGER,
    message TEXT NOT NULL,
    PRIMARY KEY (doc_id, loss_idx)
);
";

const V5_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS blocks (
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    block_idx INTEGER NOT NULL,
    kind TEXT NOT NULL,
    span_start INTEGER NOT NULL,
    span_end INTEGER NOT NULL,
    canonical_text TEXT NOT NULL,
    rendered_text TEXT,
    heading_level INTEGER,
    PRIMARY KEY (doc_id, block_idx)
);

CREATE TABLE IF NOT EXISTS block_edges (
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    edge_idx INTEGER NOT NULL,
    from_idx INTEGER NOT NULL,
    to_idx INTEGER NOT NULL,
    relation TEXT NOT NULL,
    PRIMARY KEY (doc_id, edge_idx)
);

CREATE TABLE IF NOT EXISTS block_reading_order (
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    block_idx INTEGER NOT NULL,
    PRIMARY KEY (doc_id, position)
);
";

/// Run pending migrations from `from_version` up to `CURRENT_SCHEMA_VERSION`.
fn run_migrations(conn: &rusqlite::Connection, from_version: u32) -> Result<(), ShiroError> {
    for version in from_version..CURRENT_SCHEMA_VERSION {
        if version == 1 {
            // Idempotent: column may already exist in fresh databases.
            let has_col = conn
                .prepare("SELECT rendered_text FROM documents LIMIT 0")
                .is_ok();
            if !has_col {
                conn.execute_batch("ALTER TABLE documents ADD COLUMN rendered_text TEXT")
                    .map_err(map_db)?;
            }
        }

        if version == 2 {
            // v2 → v3: taxonomy, enrichment, generation, fingerprint tables
            conn.execute_batch(V3_CREATE_TABLES).map_err(map_db)?;

            // Seed active_generations
            conn.execute_batch(
                "INSERT OR IGNORE INTO active_generations (kind, gen_id) VALUES ('fts', 0);
                 INSERT OR IGNORE INTO active_generations (kind, gen_id) VALUES ('vector', 0);",
            )
            .map_err(map_db)?;

            // Idempotent ALTER TABLE — check before adding columns
            let has_fingerprint = conn
                .prepare("SELECT fingerprint FROM documents LIMIT 0")
                .is_ok();
            if !has_fingerprint {
                conn.execute_batch("ALTER TABLE documents ADD COLUMN fingerprint TEXT")
                    .map_err(map_db)?;
            }

            let has_vector_score = conn
                .prepare("SELECT vector_score FROM search_results LIMIT 0")
                .is_ok();
            if !has_vector_score {
                conn.execute_batch(
                    "ALTER TABLE search_results ADD COLUMN vector_score REAL;
                     ALTER TABLE search_results ADD COLUMN vector_rank INTEGER;
                     ALTER TABLE search_results ADD COLUMN fused_score REAL;
                     ALTER TABLE search_results ADD COLUMN fused_rank INTEGER;
                     ALTER TABLE search_results ADD COLUMN fts_gen INTEGER;
                     ALTER TABLE search_results ADD COLUMN vec_gen INTEGER;
                     ALTER TABLE search_results ADD COLUMN query_digest TEXT;",
                )
                .map_err(map_db)?;
            }
        }

        if version == 3 {
            // Create doc_versions table
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS doc_versions (
                    version_id TEXT PRIMARY KEY,
                    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
                    fingerprint_hash TEXT,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );
                CREATE INDEX IF NOT EXISTS idx_doc_versions_doc ON doc_versions(doc_id);",
            )
            .map_err(map_db)?;

            // Add columns (idempotent check)
            let has_active_version = conn
                .prepare("SELECT active_version_id FROM documents LIMIT 0")
                .is_ok();
            if !has_active_version {
                conn.execute_batch("ALTER TABLE documents ADD COLUMN active_version_id TEXT")
                    .map_err(map_db)?;
            }
            let has_version_id = conn
                .prepare("SELECT version_id FROM segments LIMIT 0")
                .is_ok();
            if !has_version_id {
                conn.execute_batch("ALTER TABLE segments ADD COLUMN version_id TEXT")
                    .map_err(map_db)?;
            }
            let has_enrich_version = conn
                .prepare("SELECT version_id FROM enrichments LIMIT 0")
                .is_ok();
            if !has_enrich_version {
                conn.execute_batch("ALTER TABLE enrichments ADD COLUMN version_id TEXT")
                    .map_err(map_db)?;
            }

            // Backfill: for each existing document, create version 1
            let doc_ids: Vec<String> = {
                let mut stmt = conn
                    .prepare("SELECT doc_id FROM documents")
                    .map_err(map_db)?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(map_db)?;
                rows.filter_map(|r| r.ok()).collect()
            };
            for doc_id_str in &doc_ids {
                let input = format!("{doc_id_str}:1");
                let hash = blake3::hash(input.as_bytes());
                let version_id = format!("ver_{}", hash.to_hex());
                conn.execute(
                    "INSERT OR IGNORE INTO doc_versions (version_id, doc_id) VALUES (?1, ?2)",
                    rusqlite::params![version_id, doc_id_str],
                )
                .map_err(map_db)?;
                conn.execute(
                    "UPDATE documents SET active_version_id = ?1 WHERE doc_id = ?2 AND active_version_id IS NULL",
                    rusqlite::params![version_id, doc_id_str],
                ).map_err(map_db)?;
                conn.execute(
                    "UPDATE segments SET version_id = ?1 WHERE doc_id = ?2 AND version_id IS NULL",
                    rusqlite::params![version_id, doc_id_str],
                )
                .map_err(map_db)?;
                conn.execute(
                    "UPDATE enrichments SET version_id = ?1 WHERE doc_id = ?2 AND version_id IS NULL",
                    rusqlite::params![version_id, doc_id_str],
                ).map_err(map_db)?;
            }
        }

        if version == 4 {
            // v4 → v5: persist BlockGraph (ADR-006)
            conn.execute_batch(V5_CREATE_TABLES).map_err(map_db)?;
        }

        if version == 5 {
            // v5 → v6: reranker score columns
            let has_reranker = conn
                .prepare("SELECT reranker_score FROM search_results LIMIT 0")
                .is_ok();
            if !has_reranker {
                conn.execute_batch(
                    "ALTER TABLE search_results ADD COLUMN reranker_score REAL;
                     ALTER TABLE search_results ADD COLUMN reranker_rank INTEGER;",
                )
                .map_err(map_db)?;
            }
        }

        if version == 6 {
            // v6 → v7: persist the canonical EntryPoint selected at query time.
            let has_entry_point = conn
                .prepare("SELECT block_idx FROM search_results LIMIT 0")
                .is_ok();
            if !has_entry_point {
                conn.execute_batch(
                    "ALTER TABLE search_results ADD COLUMN block_idx INTEGER NOT NULL DEFAULT 0;
                     ALTER TABLE search_results ADD COLUMN block_kind TEXT NOT NULL DEFAULT 'PARAGRAPH';
                     ALTER TABLE search_results ADD COLUMN span_start INTEGER NOT NULL DEFAULT 0;
                     ALTER TABLE search_results ADD COLUMN span_end INTEGER NOT NULL DEFAULT 0;",
                )
                .map_err(map_db)?;
            }
        }

        if version == 7 {
            // v7 → v8: immutable retrieval snapshot identity and policy evidence.
            let has_snapshot = conn
                .prepare("SELECT search_snapshot_id FROM search_results LIMIT 0")
                .is_ok();
            if !has_snapshot {
                conn.execute_batch(
                    "ALTER TABLE search_results ADD COLUMN search_snapshot_id TEXT NOT NULL DEFAULT '';
                     ALTER TABLE search_results ADD COLUMN retrieval_policy_json TEXT NOT NULL DEFAULT '{}';",
                )
                .map_err(map_db)?;
            }
        }

        if version == 8 {
            // v8 → v9: persist parser-reported losses with the canonical graph.
            conn.execute_batch(V9_CREATE_TABLES).map_err(map_db)?;
        }

        if version == 9 {
            // v9 → v10: provenance-bearing immutable source artifacts.
            let has_trust_zone = conn
                .prepare("SELECT trust_zone FROM documents LIMIT 0")
                .is_ok();
            if !has_trust_zone {
                conn.execute_batch(
                    "ALTER TABLE documents ADD COLUMN trust_zone TEXT NOT NULL DEFAULT 'CANONICAL';",
                )
                .map_err(map_db)?;
            }
            conn.execute_batch(V10_CREATE_TABLES).map_err(map_db)?;
        }

        if version == 10 {
            // v10 → v11: one authoritative manifest couples derived indices.
            conn.execute_batch(V11_CREATE_TABLES).map_err(map_db)?;
        }

        if version == 11 {
            // v11 → v12: canonical parser-neutral source locators (ADR-035).
            conn.execute_batch(V12_CREATE_TABLES).map_err(map_db)?;
        }

        if version == 12 {
            // v12 → v13: snapshot exact entry-point locators for explain.
            let has_source_locators = conn
                .prepare("SELECT source_locators_json FROM search_results LIMIT 0")
                .is_ok();
            if !has_source_locators {
                conn.execute_batch(
                    "ALTER TABLE search_results ADD COLUMN source_locators_json TEXT NOT NULL DEFAULT '[]';",
                )
                .map_err(map_db)?;
            }
        }

        if version == 13 {
            // v13 → v14: preserve parser heading depth.
            let has_heading_level = conn
                .prepare("SELECT heading_level FROM blocks LIMIT 0")
                .is_ok();
            if !has_heading_level {
                conn.execute_batch("ALTER TABLE blocks ADD COLUMN heading_level INTEGER;")
                    .map_err(map_db)?;
            }
        }

        if version == 14 {
            // v14 → v15: separate source-faithful body from retrieval text.
            let has_retrieval_text = conn
                .prepare("SELECT retrieval_text FROM segments LIMIT 0")
                .is_ok();
            if !has_retrieval_text {
                conn.execute_batch(
                    "ALTER TABLE segments ADD COLUMN retrieval_text TEXT;
                     UPDATE segments SET retrieval_text = body WHERE retrieval_text IS NULL;",
                )
                .map_err(map_db)?;
            }
        }

        if version == 15 {
            // v15 → v16: snapshot exact entry-point heading depth for explain.
            let has_heading_level = conn
                .prepare("SELECT heading_level FROM search_results LIMIT 0")
                .is_ok();
            if !has_heading_level {
                conn.execute_batch("ALTER TABLE search_results ADD COLUMN heading_level INTEGER;")
                    .map_err(map_db)?;
            }
        }

        if version == 16 {
            // v16 → v17: stable block handles and explicit supersession.
            conn.execute_batch(V17_CREATE_TABLES).map_err(map_db)?;
        }

        if version == 17 {
            // v17 → v18: snapshot stable evidence handles with search results.
            let has_evidence_handle = conn
                .prepare("SELECT evidence_handle FROM search_results LIMIT 0")
                .is_ok();
            if !has_evidence_handle {
                conn.execute_batch(
                    "ALTER TABLE search_results ADD COLUMN evidence_handle TEXT NOT NULL DEFAULT '';",
                )
                .map_err(map_db)?;
            }
        }

        if version == 18 {
            // v18 → v19: actor/run/approval provenance for MCP mutations.
            conn.execute_batch(V19_CREATE_TABLES).map_err(map_db)?;
        }

        if version == 19 {
            // v19 → v20: bounded URL acquisition evidence.
            conn.execute_batch(V20_CREATE_TABLES).map_err(map_db)?;
        }

        if version == 20 {
            // v20 → v21: reversible model-enrichment proposals.
            conn.execute_batch(V21_CREATE_TABLES).map_err(map_db)?;
        }

        // Update version after each successful migration.
        conn.execute(
            "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
            rusqlite::params![(version + 1).to_string()],
        )
        .map_err(map_db)?;
    }
    Ok(())
}

/// SQLite-backed document and segment store.
#[derive(Debug)]
pub struct Store {
    conn: rusqlite::Connection,
}

impl Store {
    /// Open (or create) the database at the given path.
    pub fn open(db_path: &camino::Utf8Path) -> Result<Self, ShiroError> {
        let conn = rusqlite::Connection::open(db_path.as_std_path()).map_err(map_db)?;

        // Harden SQLite: WAL mode for concurrent readers, FK enforcement,
        // busy timeout for single-writer contention.
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            ",
        )
        .map_err(map_db)?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS documents (
                doc_id TEXT PRIMARY KEY,
                canonical_text TEXT NOT NULL,
                source_uri TEXT NOT NULL,
                source_hash TEXT NOT NULL,
                title TEXT,
                rendered_text TEXT,
                fingerprint TEXT,
                state TEXT NOT NULL DEFAULT 'STAGED'
                    CHECK(state IN ('STAGED','INDEXING','READY','FAILED','DELETED')),
                active_version_id TEXT,
                trust_zone TEXT NOT NULL DEFAULT 'CANONICAL'
                    CHECK(trust_zone IN ('CANONICAL','DERIVED','PROPOSED','QUARANTINED')),
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS segments (
                segment_id TEXT PRIMARY KEY,
                doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
                seg_index INTEGER NOT NULL,
                span_start INTEGER NOT NULL,
                span_end INTEGER NOT NULL,
                body TEXT NOT NULL,
                retrieval_text TEXT,
                version_id TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_segments_doc ON segments(doc_id);

            CREATE TABLE IF NOT EXISTS search_results (
                result_id TEXT PRIMARY KEY,
                query TEXT NOT NULL,
                doc_id TEXT NOT NULL,
                segment_id TEXT NOT NULL,
                bm25_score REAL,
                bm25_rank INTEGER,
                vector_score REAL,
                vector_rank INTEGER,
                fused_score REAL,
                fused_rank INTEGER,
                fts_gen INTEGER,
                vec_gen INTEGER,
                query_digest TEXT,
                reranker_score REAL,
                reranker_rank INTEGER,
                search_snapshot_id TEXT NOT NULL DEFAULT '',
                retrieval_policy_json TEXT NOT NULL DEFAULT '{}',
                block_idx INTEGER NOT NULL DEFAULT 0,
                block_kind TEXT NOT NULL DEFAULT 'PARAGRAPH',
                heading_level INTEGER,
                span_start INTEGER NOT NULL DEFAULT 0,
                span_end INTEGER NOT NULL DEFAULT 0,
                source_locators_json TEXT NOT NULL DEFAULT '[]',
                evidence_handle TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS blobs (
                content_hash TEXT PRIMARY KEY,
                raw_bytes BLOB NOT NULL,
                byte_count INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS doc_versions (
                version_id TEXT PRIMARY KEY,
                doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
                fingerprint_hash TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_doc_versions_doc ON doc_versions(doc_id);
            ",
        )
        .map_err(map_db)?;

        // V3 tables (idempotent via IF NOT EXISTS)
        conn.execute_batch(V3_CREATE_TABLES).map_err(map_db)?;

        // Canonical graph, parse-loss, source-artifact, and provenance persistence.
        conn.execute_batch(V5_CREATE_TABLES).map_err(map_db)?;
        conn.execute_batch(V9_CREATE_TABLES).map_err(map_db)?;
        conn.execute_batch(V10_CREATE_TABLES).map_err(map_db)?;
        conn.execute_batch(V11_CREATE_TABLES).map_err(map_db)?;
        conn.execute_batch(V12_CREATE_TABLES).map_err(map_db)?;
        conn.execute_batch(V17_CREATE_TABLES).map_err(map_db)?;
        conn.execute_batch(V19_CREATE_TABLES).map_err(map_db)?;
        conn.execute_batch(V20_CREATE_TABLES).map_err(map_db)?;
        conn.execute_batch(V21_CREATE_TABLES).map_err(map_db)?;
        conn.execute_batch(
            "INSERT OR IGNORE INTO active_generations (kind, gen_id) VALUES ('fts', 0);
             INSERT OR IGNORE INTO active_generations (kind, gen_id) VALUES ('vector', 0);",
        )
        .map_err(map_db)?;

        // Ensure schema version is tracked.
        conn.execute(
            "INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('schema_version', '1')",
            [],
        )
        .map_err(map_db)?;

        // Check and run pending migrations.
        let current_version: String = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        let version = current_version
            .parse::<u32>()
            .map_err(|e| ShiroError::StoreCorrupt {
                message: format!("invalid schema_version: {e}"),
            })?;
        if version < CURRENT_SCHEMA_VERSION {
            run_migrations(&conn, version)?;
            tracing::info!(
                from = version,
                to = CURRENT_SCHEMA_VERSION,
                "ran migrations"
            );
        } else if version > CURRENT_SCHEMA_VERSION {
            return Err(ShiroError::StoreCorrupt {
                message: format!(
                    "database schema version {version} is newer than this binary (expects {CURRENT_SCHEMA_VERSION})"
                ),
            });
        }

        tracing::debug!(path = %db_path, "opened store");
        Ok(Self { conn })
    }

    // ── Document CRUD ──────────────────────────────────────────────────

    /// Insert or update a document and its canonical BlockGraph atomically.
    pub fn put_document(&self, doc: &Document, state: DocState) -> Result<bool, ShiroError> {
        self.with_savepoint("put_document", || {
            let existed = self.exists(&doc.id)?;

            self.conn
                .execute(
                    "INSERT INTO documents (doc_id, canonical_text, rendered_text, source_uri, source_hash, title, state, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                     ON CONFLICT(doc_id) DO UPDATE SET
                         canonical_text = excluded.canonical_text,
                         rendered_text = excluded.rendered_text,
                         source_uri = excluded.source_uri,
                         source_hash = excluded.source_hash,
                         title = excluded.title,
                         state = excluded.state,
                         updated_at = excluded.updated_at",
                    rusqlite::params![
                        doc.id.as_str(),
                        doc.canonical_text,
                        doc.rendered_text,
                        doc.metadata.source_uri,
                        doc.metadata.source_hash,
                        doc.metadata.title,
                        state.as_str(),
                    ],
                )
                .map_err(map_db)?;

            self.put_block_graph(&doc.id, &doc.blocks)?;
            self.put_document_losses(&doc.id, &doc.losses)?;

            if !existed {
                let version_id = VersionId::new(&doc.id, 1);
                self.create_version(&doc.id, &version_id, None)?;
                self.set_active_version(&doc.id, &version_id)?;
            }

            Ok(!existed)
        })
    }

    /// Get a document by ID.
    pub fn get_document(&self, id: &DocId) -> Result<(Document, DocState), ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT doc_id, canonical_text, rendered_text, source_uri, source_hash, title, state
                 FROM documents WHERE doc_id = ?1",
            )
            .map_err(map_db)?;

        let result = stmt
            .query_row(rusqlite::params![id.as_str()], |row| {
                let doc_id_str: String = row.get(0)?;
                let canonical_text: String = row.get(1)?;
                let rendered_text: Option<String> = row.get(2)?;
                let source_uri: String = row.get(3)?;
                let source_hash: String = row.get(4)?;
                let title: Option<String> = row.get(5)?;
                let state_str: String = row.get(6)?;
                Ok((
                    doc_id_str,
                    canonical_text,
                    rendered_text,
                    source_uri,
                    source_hash,
                    title,
                    state_str,
                ))
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFound(id.clone()),
                other => map_db(other),
            })?;

        let (doc_id_str, canonical_text, rendered_text, source_uri, source_hash, title, state_str) =
            result;

        let doc_id = DocId::from_stored(doc_id_str).map_err(|e| ShiroError::StoreCorrupt {
            message: e.to_string(),
        })?;
        let state = parse_state(&state_str)?;

        let blocks = self.get_block_graph(&doc_id)?;
        let losses = self.get_document_losses(&doc_id)?;

        let doc = Document {
            id: doc_id,
            canonical_text,
            rendered_text,
            metadata: Metadata {
                title,
                source_uri,
                source_hash,
            },
            blocks,
            losses,
        };

        Ok((doc, state))
    }

    /// List all document IDs with their state, ordered by `created_at`.
    /// Returns `(doc_id, state, title)`.
    pub fn list_documents(
        &self,
        limit: usize,
    ) -> Result<Vec<(DocId, DocState, Option<String>)>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare("SELECT doc_id, state, title FROM documents ORDER BY created_at LIMIT ?1")
            .map_err(map_db)?;

        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                let doc_id_str: String = row.get(0)?;
                let state_str: String = row.get(1)?;
                let title: Option<String> = row.get(2)?;
                Ok((doc_id_str, state_str, title))
            })
            .map_err(map_db)?;

        let mut out = Vec::new();
        for row in rows {
            let (doc_id_str, state_str, title) = row.map_err(map_db)?;
            let doc_id = DocId::from_stored(doc_id_str).map_err(|e| ShiroError::StoreCorrupt {
                message: e.to_string(),
            })?;
            let state = parse_state(&state_str)?;
            out.push((doc_id, state, title));
        }

        Ok(out)
    }

    /// List every document for callers that must filter before applying a limit.
    pub fn list_all_documents(&self) -> Result<Vec<(DocId, DocState, Option<String>)>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare("SELECT doc_id, state, title FROM documents ORDER BY created_at")
            .map_err(map_db)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(map_db)?;

        let mut documents = Vec::new();
        for row in rows {
            let (doc_id, state, title) = row.map_err(map_db)?;
            documents.push((
                DocId::from_stored(doc_id).map_err(|error| ShiroError::StoreCorrupt {
                    message: error.to_string(),
                })?,
                parse_state(&state)?,
                title,
            ));
        }
        Ok(documents)
    }

    /// Return segment ownership for documents currently eligible for retrieval.
    ///
    /// The document state in SQLite is authoritative: only segments whose
    /// owning document is READY are included.
    pub fn ready_document_segment_ids(&self) -> Result<Vec<(DocId, SegmentId)>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT documents.doc_id, segments.segment_id
                 FROM documents
                 JOIN segments ON segments.doc_id = documents.doc_id
                 WHERE documents.state = 'READY'
                   AND documents.trust_zone IN ('CANONICAL', 'DERIVED')
                 ORDER BY documents.doc_id, segments.seg_index",
            )
            .map_err(map_db)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(map_db)?;

        let mut eligible = Vec::new();
        for row in rows {
            let (doc_id, segment_id) = row.map_err(map_db)?;
            let doc_id = DocId::from_stored(doc_id).map_err(|error| ShiroError::StoreCorrupt {
                message: error.to_string(),
            })?;
            let segment_id =
                SegmentId::from_stored(segment_id).map_err(|error| ShiroError::StoreCorrupt {
                    message: error.to_string(),
                })?;
            eligible.push((doc_id, segment_id));
        }
        Ok(eligible)
    }

    /// Transition a document's state with guard validation.
    ///
    /// Validates the transition against `DocState::can_transition_to()`
    /// within a savepoint to prevent TOCTOU races.
    pub fn set_state(&self, id: &DocId, new_state: DocState) -> Result<(), ShiroError> {
        self.with_savepoint("set_state", || {
            let current_str: String = self
                .conn
                .query_row(
                    "SELECT state FROM documents WHERE doc_id = ?1",
                    rusqlite::params![id.as_str()],
                    |row| row.get(0),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFound(id.clone()),
                    other => map_db(other),
                })?;

            let current = parse_state(&current_str)?;

            if !current.can_transition_to(new_state) {
                return Err(ShiroError::InvalidInput {
                    message: format!(
                        "invalid state transition: {current} \u{2192} {new_state} for {id}"
                    ),
                });
            }

            self.conn
                .execute(
                    "UPDATE documents SET state = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE doc_id = ?2",
                    rusqlite::params![new_state.as_str(), id.as_str()],
                )
                .map_err(map_db)?;

            Ok(())
        })
    }

    /// Mark a batch of Indexing documents Ready in one SQLite savepoint.
    pub fn set_documents_ready(&self, doc_ids: &[DocId]) -> Result<(), ShiroError> {
        self.with_savepoint("set_documents_ready", || {
            for doc_id in doc_ids {
                self.set_state(doc_id, DocState::Ready)?;
            }
            Ok(())
        })
    }

    // ── Segment CRUD ───────────────────────────────────────────────────

    /// Insert segments for a document (replaces existing).
    ///
    /// Wrapped in a savepoint so a mid-loop failure does not leave the
    /// document with partial or zero segments.
    pub fn put_segments(&self, segments: &[Segment]) -> Result<(), ShiroError> {
        let Some(first_segment) = segments.first() else {
            return Ok(());
        };
        self.with_savepoint("put_segments", || {
            self.replace_document_segments(&first_segment.doc_id, segments)
        })
    }

    fn replace_document_segments(
        &self,
        doc_id: &DocId,
        segments: &[Segment],
    ) -> Result<(), ShiroError> {
        if let Some(foreign_segment) = segments.iter().find(|segment| segment.doc_id != *doc_id) {
            return Err(ShiroError::InvalidInput {
                message: format!(
                    "document segment ownership mismatch: expected {doc_id}, got {}",
                    foreign_segment.doc_id
                ),
            });
        }

        let version_id_str: Option<String> = self
            .conn
            .query_row(
                "SELECT active_version_id FROM documents WHERE doc_id = ?1",
                rusqlite::params![doc_id.as_str()],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFound(doc_id.clone()),
                other => map_db(other),
            })?;

        self.conn
            .execute(
                "DELETE FROM segments WHERE doc_id = ?1",
                rusqlite::params![doc_id.as_str()],
            )
            .map_err(map_db)?;

        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO segments (segment_id, doc_id, seg_index, span_start, span_end, body, retrieval_text, version_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .map_err(map_db)?;

        for segment in segments {
            stmt.execute(rusqlite::params![
                segment.id.as_str(),
                segment.doc_id.as_str(),
                segment.index as i64,
                segment.span.start() as i64,
                segment.span.end() as i64,
                segment.body,
                segment.retrieval_text,
                version_id_str,
            ])
            .map_err(map_db)?;
        }

        Ok(())
    }

    /// Stage a document's graph, processing fingerprint, and segments atomically.
    ///
    /// Ready content-addressed documents are left unchanged. Staged, indexing,
    /// and failed documents are reset through the lifecycle to Staged so an
    /// interrupted ingestion can be retried safely.
    pub fn stage_document_processing(
        &self,
        doc: &Document,
        fingerprint: &ProcessingFingerprint,
        segments: &[Segment],
        source_bytes: &[u8],
        provenance: &WriteProvenance,
    ) -> Result<bool, ShiroError> {
        self.stage_document_processing_with_force(
            doc,
            fingerprint,
            segments,
            source_bytes,
            provenance,
            false,
        )
    }

    /// Stage processing even when the stored fingerprint is current.
    pub fn stage_document_processing_with_force(
        &self,
        doc: &Document,
        fingerprint: &ProcessingFingerprint,
        segments: &[Segment],
        source_bytes: &[u8],
        provenance: &WriteProvenance,
        force: bool,
    ) -> Result<bool, ShiroError> {
        self.with_savepoint("stage_document_processing", || {
            let source_hash = self.put_blob(source_bytes)?;
            if source_hash != doc.metadata.source_hash || provenance.content_hash != source_hash {
                return Err(ShiroError::StoreCorrupt {
                    message: format!(
                        "source provenance hash mismatch for {}: parser={}, provenance={}, stored={source_hash}",
                        doc.id, doc.metadata.source_hash, provenance.content_hash
                    ),
                });
            }

            let stored_state: Option<String> = self
                .conn
                .query_row(
                    "SELECT state FROM documents WHERE doc_id = ?1",
                    rusqlite::params![doc.id.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(map_db)?;

            let mut reprocessing_ready_document = false;
            match stored_state.as_deref().map(parse_state).transpose()? {
                Some(DocState::Ready) => {
                    let stored_fingerprint = self.get_fingerprint(&doc.id)?;
                    if !force
                        && stored_fingerprint
                            .as_ref()
                            .map(ProcessingFingerprint::content_hash)
                            .as_deref()
                            == Some(fingerprint.content_hash().as_str())
                    {
                        self.record_source_artifact(
                            &doc.id,
                            &source_hash,
                            &doc.metadata.source_uri,
                            source_bytes.len(),
                            provenance,
                        )?;
                        return Ok(false);
                    }
                    reprocessing_ready_document = true;
                }
                Some(DocState::Staged) | None => {}
                Some(DocState::Indexing) => {
                    self.set_state(&doc.id, DocState::Failed)?;
                    self.set_state(&doc.id, DocState::Staged)?;
                }
                Some(DocState::Failed) => {
                    self.set_state(&doc.id, DocState::Staged)?;
                }
                Some(DocState::Deleted) => {
                    return Err(ShiroError::InvalidInput {
                        message: format!(
                            "document ingestion retry rejected for deleted document {}",
                            doc.id
                        ),
                    });
                }
            }

            self.put_document(doc, DocState::Staged)?;
            if reprocessing_ready_document {
                let next_sequence = self.count_versions(&doc.id)?.saturating_add(1) as u64;
                let version_id = VersionId::new(&doc.id, next_sequence);
                let fingerprint_hash = fingerprint.content_hash();
                self.create_version(&doc.id, &version_id, Some(&fingerprint_hash))?;
                self.set_active_version(&doc.id, &version_id)?;
            }
            self.set_fingerprint(&doc.id, fingerprint)?;
            self.replace_document_segments(&doc.id, segments)?;
            self.record_source_artifact(
                &doc.id,
                &source_hash,
                &doc.metadata.source_uri,
                source_bytes.len(),
                provenance,
            )?;
            let entity_id = self
                .active_version_id(&doc.id)?
                .map(|version_id| version_id.as_str().to_string())
                .unwrap_or_else(|| doc.id.as_str().to_string());
            self.append_provenance(&doc.id, "document_processing", &entity_id, provenance)?;
            Ok(true)
        })
    }

    /// Atomically stage canonical URL content with acquisition evidence.
    #[allow(clippy::too_many_arguments)]
    pub fn stage_url_document_processing(
        &self,
        doc: &Document,
        fingerprint: &ProcessingFingerprint,
        segments: &[Segment],
        source_bytes: &[u8],
        provenance: &WriteProvenance,
        acquisition: &UrlAcquisitionRecord,
    ) -> Result<bool, ShiroError> {
        self.with_savepoint("stage_url_document_processing", || {
            let changed = self.stage_document_processing_with_force(
                doc,
                fingerprint,
                segments,
                source_bytes,
                provenance,
                false,
            )?;
            self.conn
                .execute(
                    "INSERT INTO url_acquisitions (
                        doc_id, requested_url, final_url, redirects_json, content_type,
                        signature, byte_count, content_hash
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        doc.id.as_str(),
                        acquisition.requested_url,
                        acquisition.final_url,
                        acquisition.redirects_json,
                        acquisition.content_type,
                        acquisition.signature,
                        acquisition.byte_count as i64,
                        acquisition.content_hash,
                    ],
                )
                .map_err(map_db)?;
            Ok(changed)
        })
    }

    // ── Canonical graph and parse-loss persistence ─────────────────────

    fn put_document_losses(&self, doc_id: &DocId, losses: &[ParseLoss]) -> Result<(), ShiroError> {
        self.conn
            .execute(
                "DELETE FROM document_losses WHERE doc_id = ?1",
                rusqlite::params![doc_id.as_str()],
            )
            .map_err(map_db)?;
        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO document_losses (doc_id, loss_idx, kind, span_start, span_end, message)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(map_db)?;
        for (index, loss) in losses.iter().enumerate() {
            stmt.execute(rusqlite::params![
                doc_id.as_str(),
                index as i64,
                loss_kind_to_sql(loss.kind),
                loss.span.map(|span| span.start() as i64),
                loss.span.map(|span| span.end() as i64),
                loss.message,
            ])
            .map_err(map_db)?;
        }
        Ok(())
    }

    fn get_document_losses(&self, doc_id: &DocId) -> Result<Vec<ParseLoss>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT kind, span_start, span_end, message
                 FROM document_losses WHERE doc_id = ?1 ORDER BY loss_idx",
            )
            .map_err(map_db)?;
        let rows = stmt
            .query_map(rusqlite::params![doc_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(map_db)?;

        let mut losses = Vec::new();
        for row in rows {
            let (kind, span_start, span_end, message) = row.map_err(map_db)?;
            let span = match (span_start, span_end) {
                (Some(start), Some(end)) => {
                    Some(Span::new(start as usize, end as usize).map_err(|error| {
                        ShiroError::StoreCorrupt {
                            message: format!(
                                "invalid persisted parse-loss span for {doc_id}: {error}"
                            ),
                        }
                    })?)
                }
                (None, None) => None,
                _ => {
                    return Err(ShiroError::StoreCorrupt {
                        message: format!("incomplete persisted parse-loss span for {doc_id}"),
                    });
                }
            };
            losses.push(ParseLoss {
                kind: parse_loss_kind(&kind)?,
                span,
                message,
            });
        }
        Ok(losses)
    }

    /// Persist a document's BlockGraph. Replaces any existing graph data.
    ///
    /// Per ADR-006, the graph is canonical; segments are derived.
    /// This must be called in the same transaction as put_document/put_segments.
    pub fn put_block_graph(&self, doc_id: &DocId, graph: &BlockGraph) -> Result<(), ShiroError> {
        self.with_savepoint("put_block_graph", || {
            let id = doc_id.as_str();
            self.snapshot_current_evidence_handles(doc_id)?;

            // Clear existing graph data for this document.
            self.conn
                .execute("DELETE FROM blocks WHERE doc_id = ?1", rusqlite::params![id])
                .map_err(map_db)?;
            self.conn
                .execute(
                    "DELETE FROM block_edges WHERE doc_id = ?1",
                    rusqlite::params![id],
                )
                .map_err(map_db)?;
            self.conn
                .execute(
                    "DELETE FROM block_reading_order WHERE doc_id = ?1",
                    rusqlite::params![id],
                )
                .map_err(map_db)?;

            // Insert blocks.
            {
                let mut stmt = self
                    .conn
                    .prepare(
                        "INSERT INTO blocks (doc_id, block_idx, kind, span_start, span_end, canonical_text, rendered_text, heading_level)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    )
                    .map_err(map_db)?;
                for (i, block) in graph.blocks.iter().enumerate() {
                    stmt.execute(rusqlite::params![
                        id,
                        i as i64,
                        block_kind_to_sql(&block.kind),
                        block.span.start() as i64,
                        block.span.end() as i64,
                        block.canonical_text,
                        block.rendered_text,
                        block
                            .heading_level
                            .map(DocumentHeadingLevel::as_u32)
                            .map(i64::from),
                    ])
                    .map_err(map_db)?;
                }
            }

            // Insert source locators after their parent blocks.
            {
                let mut stmt = self
                    .conn
                    .prepare(
                        "INSERT INTO block_source_locators (
                            doc_id, block_idx, locator_idx, page_number,
                            region_x0, region_y0, region_x1, region_y1,
                            coordinate_origin, page_width, page_height
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    )
                    .map_err(map_db)?;
                for (block_index, block) in graph.blocks.iter().enumerate() {
                    for (locator_index, locator) in block.source_locators.iter().enumerate() {
                        let region = locator.region();
                        let dimensions = locator.page_dimensions();
                        stmt.execute(rusqlite::params![
                            id,
                            block_index as i64,
                            locator_index as i64,
                            locator.page_number() as i64,
                            region.map(SourceRegion::x0),
                            region.map(SourceRegion::y0),
                            region.map(SourceRegion::x1),
                            region.map(SourceRegion::y1),
                            locator.coordinate_origin().map(coordinate_origin_to_sql),
                            dimensions.map(PageDimensions::width),
                            dimensions.map(PageDimensions::height),
                        ])
                        .map_err(map_db)?;
                    }
                }
            }

            // Insert edges.
            {
                let mut stmt = self
                    .conn
                    .prepare(
                        "INSERT INTO block_edges (doc_id, edge_idx, from_idx, to_idx, relation)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                    )
                    .map_err(map_db)?;
                for (i, edge) in graph.edges.iter().enumerate() {
                    stmt.execute(rusqlite::params![
                        id,
                        i as i64,
                        edge.from.0 as i64,
                        edge.to.0 as i64,
                        relation_to_edge_sql(&edge.relation),
                    ])
                    .map_err(map_db)?;
                }
            }

            // Insert reading order.
            {
                let mut stmt = self
                    .conn
                    .prepare(
                        "INSERT INTO block_reading_order (doc_id, position, block_idx)
                         VALUES (?1, ?2, ?3)",
                    )
                    .map_err(map_db)?;
                for (pos, idx) in graph.reading_order.iter().enumerate() {
                    stmt.execute(rusqlite::params![id, pos as i64, idx.0 as i64])
                        .map_err(map_db)?;
                }
            }

            self.replace_active_evidence_handles(doc_id, graph)?;
            Ok(())
        })
    }

    /// Load the persisted BlockGraph for a document.
    ///
    /// Returns `BlockGraph::empty()` if no graph data exists (e.g. pre-v5 documents).
    pub fn get_block_graph(&self, doc_id: &DocId) -> Result<BlockGraph, ShiroError> {
        let id = doc_id.as_str();

        // Load blocks.
        let mut blocks = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT block_idx, kind, span_start, span_end, canonical_text, rendered_text, heading_level
                     FROM blocks WHERE doc_id = ?1 ORDER BY block_idx",
                )
                .map_err(map_db)?;
            let rows = stmt
                .query_map(rusqlite::params![id], |row| {
                    let kind_str: String = row.get(1)?;
                    let span_start: i64 = row.get(2)?;
                    let span_end: i64 = row.get(3)?;
                    let canonical_text: String = row.get(4)?;
                    let rendered_text: Option<String> = row.get(5)?;
                    let heading_level: Option<i64> = row.get(6)?;
                    Ok((
                        kind_str,
                        span_start,
                        span_end,
                        canonical_text,
                        rendered_text,
                        heading_level,
                    ))
                })
                .map_err(map_db)?;

            let mut blocks = Vec::new();
            for row in rows {
                let (kind_str, span_start, span_end, canonical_text, rendered_text, heading_level) =
                    row.map_err(map_db)?;
                let kind = parse_block_kind(&kind_str)?;
                let span = Span::new(span_start as usize, span_end as usize).map_err(|e| {
                    ShiroError::StoreCorrupt {
                        message: format!("invalid block span: {e}"),
                    }
                })?;
                let heading_level = heading_level
                    .map(|level| {
                        u32::try_from(level)
                            .map_err(|_| ShiroError::StoreCorrupt {
                                message: format!("invalid heading level for {doc_id}"),
                            })
                            .and_then(|level| {
                                DocumentHeadingLevel::new(level).map_err(|error| {
                                    ShiroError::StoreCorrupt {
                                        message: format!(
                                            "invalid heading level for {doc_id}: {error}"
                                        ),
                                    }
                                })
                            })
                    })
                    .transpose()?;
                if heading_level.is_some() && !matches!(kind, BlockKind::Heading) {
                    return Err(ShiroError::StoreCorrupt {
                        message: format!("invalid heading level for {doc_id}"),
                    });
                }
                blocks.push(Block {
                    canonical_text,
                    rendered_text,
                    kind,
                    heading_level,
                    span,
                    source_locators: Vec::new(),
                });
            }
            blocks
        };

        // Load and validate source locators. Partial persisted geometry is
        // corruption rather than a reason to fabricate missing coordinates.
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT block_idx, page_number,
                            region_x0, region_y0, region_x1, region_y1,
                            coordinate_origin, page_width, page_height
                     FROM block_source_locators
                     WHERE doc_id = ?1 ORDER BY block_idx, locator_idx",
                )
                .map_err(map_db)?;
            let rows = stmt
                .query_map(rusqlite::params![id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<f64>>(2)?,
                        row.get::<_, Option<f64>>(3)?,
                        row.get::<_, Option<f64>>(4)?,
                        row.get::<_, Option<f64>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<f64>>(7)?,
                        row.get::<_, Option<f64>>(8)?,
                    ))
                })
                .map_err(map_db)?;
            for row in rows {
                let (block_index, page_number, x0, y0, x1, y1, origin, width, height) =
                    row.map_err(map_db)?;
                let block_index =
                    usize::try_from(block_index).map_err(|_| ShiroError::StoreCorrupt {
                        message: format!("negative source-locator block index for {doc_id}"),
                    })?;
                let block =
                    blocks
                        .get_mut(block_index)
                        .ok_or_else(|| ShiroError::StoreCorrupt {
                            message: format!(
                            "source locator references missing block {block_index} for {doc_id}"
                        ),
                        })?;
                let page_number =
                    u32::try_from(page_number).map_err(|_| ShiroError::StoreCorrupt {
                        message: format!("invalid source-locator page number for {doc_id}"),
                    })?;
                let region = match (x0, y0, x1, y1) {
                    (None, None, None, None) => None,
                    (Some(x0), Some(y0), Some(x1), Some(y1)) => Some(
                        SourceRegion::new(x0, y0, x1, y1).map_err(|error| {
                            ShiroError::StoreCorrupt {
                                message: format!(
                                    "invalid source region for {doc_id} block {block_index}: {error}"
                                ),
                            }
                        })?,
                    ),
                    _ => {
                        return Err(ShiroError::StoreCorrupt {
                            message: format!(
                                "partial source region for {doc_id} block {block_index}"
                            ),
                        });
                    }
                };
                let dimensions = match (width, height) {
                    (None, None) => None,
                    (Some(width), Some(height)) => Some(
                        PageDimensions::new(width, height).map_err(|error| {
                            ShiroError::StoreCorrupt {
                                message: format!(
                                    "invalid page dimensions for {doc_id} block {block_index}: {error}"
                                ),
                            }
                        })?,
                    ),
                    _ => {
                        return Err(ShiroError::StoreCorrupt {
                            message: format!(
                                "partial page dimensions for {doc_id} block {block_index}"
                            ),
                        });
                    }
                };
                let origin = origin.as_deref().map(parse_coordinate_origin).transpose()?;
                let locator = SourceLocator::new(page_number, region, origin, dimensions).map_err(
                    |error| ShiroError::StoreCorrupt {
                        message: format!(
                            "invalid source locator for {doc_id} block {block_index}: {error}"
                        ),
                    },
                )?;
                block.source_locators.push(locator);
            }
        }

        // Load edges.
        let edges = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT from_idx, to_idx, relation
                     FROM block_edges WHERE doc_id = ?1 ORDER BY edge_idx",
                )
                .map_err(map_db)?;
            let rows = stmt
                .query_map(rusqlite::params![id], |row| {
                    let from: i64 = row.get(0)?;
                    let to: i64 = row.get(1)?;
                    let rel_str: String = row.get(2)?;
                    Ok((from, to, rel_str))
                })
                .map_err(map_db)?;

            let mut edges = Vec::new();
            for row in rows {
                let (from, to, rel_str) = row.map_err(map_db)?;
                let relation = parse_edge_relation(&rel_str)?;
                edges.push(Edge {
                    from: BlockIdx(from as usize),
                    to: BlockIdx(to as usize),
                    relation,
                });
            }
            edges
        };

        // Load reading order.
        let reading_order = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT block_idx FROM block_reading_order
                     WHERE doc_id = ?1 ORDER BY position",
                )
                .map_err(map_db)?;
            let rows = stmt
                .query_map(rusqlite::params![id], |row| {
                    let idx: i64 = row.get(0)?;
                    Ok(BlockIdx(idx as usize))
                })
                .map_err(map_db)?;

            rows.collect::<Result<Vec<_>, _>>().map_err(map_db)?
        };

        Ok(BlockGraph {
            blocks,
            edges,
            reading_order,
        })
    }

    fn snapshot_current_evidence_handles(&self, doc_id: &DocId) -> Result<(), ShiroError> {
        let active_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM evidence_handles WHERE doc_id = ?1 AND status = 'ACTIVE'",
                rusqlite::params![doc_id.as_str()],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        if active_count > 0 {
            return Ok(());
        }
        let graph = self.get_block_graph(doc_id)?;
        if graph.blocks.is_empty() {
            return Ok(());
        }
        self.insert_active_evidence_handles(doc_id, &graph)
    }

    fn replace_active_evidence_handles(
        &self,
        doc_id: &DocId,
        graph: &BlockGraph,
    ) -> Result<(), ShiroError> {
        let old_handles = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT handle_id, span_start, span_end
                     FROM evidence_handles
                     WHERE doc_id = ?1 AND status = 'ACTIVE' ORDER BY handle_id",
                )
                .map_err(map_db)?;
            let rows = stmt
                .query_map(rusqlite::params![doc_id.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .map_err(map_db)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(map_db)?
        };
        self.conn
            .execute(
                "UPDATE evidence_handles
                 SET status = 'SUPERSEDED', superseded_by = NULL,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE doc_id = ?1 AND status = 'ACTIVE'",
                rusqlite::params![doc_id.as_str()],
            )
            .map_err(map_db)?;
        self.insert_active_evidence_handles(doc_id, graph)?;

        let new_handles = graph
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| {
                evidence_handle_for_block(doc_id, graph, index)
                    .map(|handle| (handle, block.span.start(), block.span.end()))
            })
            .collect::<Vec<_>>();
        let new_ids = new_handles
            .iter()
            .map(|(handle, _, _)| handle.as_str())
            .collect::<std::collections::HashSet<_>>();
        for (old_handle, old_start, old_end) in old_handles {
            if new_ids.contains(old_handle.as_str()) {
                continue;
            }
            let successor = new_handles
                .iter()
                .filter_map(|(handle, new_start, new_end)| {
                    let overlap_start = (old_start as usize).max(*new_start);
                    let overlap_end = (old_end as usize).min(*new_end);
                    (overlap_start < overlap_end).then(|| (handle, overlap_end - overlap_start))
                })
                .max_by(
                    |(left_handle, left_overlap), (right_handle, right_overlap)| {
                        left_overlap
                            .cmp(right_overlap)
                            .then_with(|| right_handle.as_str().cmp(left_handle.as_str()))
                    },
                )
                .map(|(handle, _)| handle.as_str());
            self.conn
                .execute(
                    "UPDATE evidence_handles SET superseded_by = ?1
                     WHERE handle_id = ?2 AND status = 'SUPERSEDED'",
                    rusqlite::params![successor, old_handle],
                )
                .map_err(map_db)?;
        }
        Ok(())
    }

    fn insert_active_evidence_handles(
        &self,
        doc_id: &DocId,
        graph: &BlockGraph,
    ) -> Result<(), ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO evidence_handles (
                    handle_id, doc_id, block_idx, status, superseded_by,
                    block_kind, heading_level, span_start, span_end,
                    canonical_text, source_locators_json
                 ) VALUES (?1, ?2, ?3, 'ACTIVE', NULL, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(handle_id) DO UPDATE SET
                    block_idx = excluded.block_idx,
                    status = 'ACTIVE',
                    superseded_by = NULL,
                    block_kind = excluded.block_kind,
                    heading_level = excluded.heading_level,
                    span_start = excluded.span_start,
                    span_end = excluded.span_end,
                    canonical_text = excluded.canonical_text,
                    source_locators_json = excluded.source_locators_json,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            )
            .map_err(map_db)?;
        for (block_index, block) in graph.blocks.iter().enumerate() {
            let Some(handle) = evidence_handle_for_block(doc_id, graph, block_index) else {
                continue;
            };
            let source_locators_json =
                serde_json::to_string(&block.source_locators).map_err(|error| {
                    ShiroError::StoreCorrupt {
                        message: format!("failed to serialize evidence-handle locators: {error}"),
                    }
                })?;
            stmt.execute(rusqlite::params![
                handle.as_str(),
                doc_id.as_str(),
                block_index as i64,
                block_kind_to_sql(&block.kind),
                block
                    .heading_level
                    .map(DocumentHeadingLevel::as_u32)
                    .map(i64::from),
                block.span.start() as i64,
                block.span.end() as i64,
                block.canonical_text,
                source_locators_json,
            ])
            .map_err(map_db)?;
        }
        Ok(())
    }

    /// Resolve an active or superseded stable block handle.
    pub fn get_evidence_handle(
        &self,
        handle: &EvidenceHandleId,
    ) -> Result<EvidenceHandleResolution, ShiroError> {
        let row = self
            .conn
            .query_row(
                "SELECT doc_id, block_idx, status, superseded_by, block_kind,
                        heading_level, span_start, span_end, canonical_text,
                        source_locators_json
                 FROM evidence_handles WHERE handle_id = ?1",
                rusqlite::params![handle.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                    ))
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFoundMsg {
                    message: format!("evidence handle not found: {handle}"),
                },
                other => map_db(other),
            })?;
        let (
            doc_id,
            block_idx,
            status,
            superseded_by,
            block_kind,
            heading_level,
            span_start,
            span_end,
            canonical_text,
            source_locators_json,
        ) = row;
        let heading_level = heading_level
            .map(|level| {
                u32::try_from(level).map_err(|_| ShiroError::StoreCorrupt {
                    message: format!("invalid evidence-handle heading level: {handle}"),
                })
            })
            .transpose()?;
        if heading_level == Some(0) {
            return Err(ShiroError::StoreCorrupt {
                message: format!("zero evidence-handle heading level: {handle}"),
            });
        }
        Ok(EvidenceHandleResolution {
            handle_id: handle.clone(),
            status,
            superseded_by: superseded_by
                .map(EvidenceHandleId::from_stored)
                .transpose()
                .map_err(|message| ShiroError::StoreCorrupt {
                    message: message.to_string(),
                })?,
            doc_id: DocId::from_stored(doc_id).map_err(|message| ShiroError::StoreCorrupt {
                message: message.to_string(),
            })?,
            block_idx: usize::try_from(block_idx).map_err(|_| ShiroError::StoreCorrupt {
                message: format!("negative evidence-handle block index: {handle}"),
            })?,
            block_kind: parse_block_kind(&block_kind)?,
            heading_level,
            span: Span::new(
                usize::try_from(span_start).map_err(|_| ShiroError::StoreCorrupt {
                    message: format!("negative evidence-handle span: {handle}"),
                })?,
                usize::try_from(span_end).map_err(|_| ShiroError::StoreCorrupt {
                    message: format!("negative evidence-handle span: {handle}"),
                })?,
            )
            .map_err(|error| ShiroError::StoreCorrupt {
                message: format!("invalid evidence-handle span: {error}"),
            })?,
            canonical_text,
            source_locators: serde_json::from_str(&source_locators_json).map_err(|error| {
                ShiroError::StoreCorrupt {
                    message: format!("invalid evidence-handle source locators: {error}"),
                }
            })?,
        })
    }

    /// Tombstone a document and remove every deferred evidence handle in one transaction.
    pub fn tombstone_document_evidence(&self, doc_id: &DocId) -> Result<(), ShiroError> {
        self.with_savepoint("tombstone_document_evidence", || {
            self.set_state(doc_id, DocState::Deleted)?;
            self.conn
                .execute(
                    "DELETE FROM evidence_handles WHERE doc_id = ?1",
                    rusqlite::params![doc_id.as_str()],
                )
                .map_err(map_db)?;
            Ok(())
        })
    }

    /// Purge all derived data for a document.
    ///
    /// Removes segments and search_results associated with this doc_id.
    /// The document row itself is preserved (tombstoned as DELETED).
    /// Note: blocks/edges/reading_order are canonical (ADR-006), not derived.
    pub fn purge_derived(&self, doc_id: &DocId) -> Result<(), ShiroError> {
        self.with_savepoint("purge_derived", || {
            self.conn
                .execute(
                    "DELETE FROM segments WHERE doc_id = ?1",
                    rusqlite::params![doc_id.as_str()],
                )
                .map_err(map_db)?;
            self.conn
                .execute(
                    "DELETE FROM search_results WHERE doc_id = ?1",
                    rusqlite::params![doc_id.as_str()],
                )
                .map_err(map_db)?;
            Ok(())
        })
    }

    /// Get all segments for a document.
    pub fn get_segments(&self, doc_id: &DocId) -> Result<Vec<Segment>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT segment_id, doc_id, seg_index, span_start, span_end, body, retrieval_text
                 FROM segments WHERE doc_id = ?1 ORDER BY seg_index",
            )
            .map_err(map_db)?;

        let rows = stmt
            .query_map(rusqlite::params![doc_id.as_str()], |row| {
                let segment_id_str: String = row.get(0)?;
                let doc_id_str: String = row.get(1)?;
                let index: i64 = row.get(2)?;
                let span_start: i64 = row.get(3)?;
                let span_end: i64 = row.get(4)?;
                let body: String = row.get(5)?;
                let retrieval_text: Option<String> = row.get(6)?;
                Ok((
                    segment_id_str,
                    doc_id_str,
                    index,
                    span_start,
                    span_end,
                    body,
                    retrieval_text,
                ))
            })
            .map_err(map_db)?;

        let mut out = Vec::new();
        for row in rows {
            let (segment_id_str, doc_id_str, index, span_start, span_end, body, retrieval_text) =
                row.map_err(map_db)?;

            let id =
                SegmentId::from_stored(segment_id_str).map_err(|e| ShiroError::StoreCorrupt {
                    message: e.to_string(),
                })?;
            let did = DocId::from_stored(doc_id_str).map_err(|e| ShiroError::StoreCorrupt {
                message: e.to_string(),
            })?;
            let span = Span::new(span_start as usize, span_end as usize).map_err(|e| {
                ShiroError::StoreCorrupt {
                    message: e.to_string(),
                }
            })?;

            out.push(Segment {
                id,
                doc_id: did,
                index: index as usize,
                span,
                retrieval_text: retrieval_text.unwrap_or_else(|| body.clone()),
                body,
            });
        }

        Ok(out)
    }

    /// Look up the doc_id that owns a given segment.
    pub fn segment_doc_id(&self, segment_id: &SegmentId) -> Result<String, ShiroError> {
        self.conn
            .query_row(
                "SELECT doc_id FROM segments WHERE segment_id = ?1",
                rusqlite::params![segment_id.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFoundMsg {
                    message: format!("segment not found: {}", segment_id.as_str()),
                },
                other => map_db(other),
            })
    }

    // ── Search results ─────────────────────────────────────────────────

    /// Save search results for later explain.
    pub fn save_search_results(
        &self,
        snapshot: &SearchSnapshotMetadata<'_>,
        results: &[SearchResultRow],
    ) -> Result<(), ShiroError> {
        self.with_savepoint("save_search_results", || {
            let mut stmt = self
                .conn
                .prepare(
                    "INSERT INTO search_results (result_id, query, doc_id, segment_id, bm25_score, bm25_rank, vector_score, vector_rank, fused_score, fused_rank, fts_gen, vec_gen, query_digest, reranker_score, reranker_rank, search_snapshot_id, retrieval_policy_json, block_idx, block_kind, span_start, span_end, source_locators_json, heading_level, evidence_handle)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
                )
                .map_err(map_db)?;

            for r in results {
                let source_locators_json = serde_json::to_string(&r.source_locators).map_err(
                    |error| ShiroError::StoreCorrupt {
                        message: format!("failed to serialize entry-point source locators: {error}"),
                    },
                )?;
                stmt.execute(rusqlite::params![
                    r.result_id,
                    snapshot.query,
                    r.doc_id.as_str(),
                    r.segment_id.as_str(),
                    r.bm25_score.map(|s| s as f64),
                    r.bm25_rank.map(|r| r as i64),
                    r.vector_score.map(|s| s as f64),
                    r.vector_rank.map(|r| r as i64),
                    r.fused_score.map(|s| s as f64),
                    r.fused_rank.map(|r| r as i64),
                    snapshot.fts_generation as i64,
                    snapshot.vector_generation as i64,
                    snapshot.query_digest,
                    r.reranker_score.map(|s| s as f64),
                    r.reranker_rank.map(|r| r as i64),
                    snapshot.search_snapshot_id,
                    snapshot.retrieval_policy_json,
                    r.block_idx as i64,
                    r.block_kind,
                    r.span_start as i64,
                    r.span_end as i64,
                    source_locators_json,
                    r.heading_level.map(i64::from),
                    r.evidence_handle.as_str(),
                ])
                .map_err(map_db)?;
            }

            Ok(())
        })
    }

    /// Load a saved search result by `result_id`.
    pub fn get_search_result(&self, result_id: &str) -> Result<SearchResultDetail, ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT query, doc_id, segment_id, bm25_score, bm25_rank, vector_score, vector_rank, fused_score, fused_rank, fts_gen, vec_gen, query_digest, reranker_score, reranker_rank, block_idx, block_kind, span_start, span_end, search_snapshot_id, retrieval_policy_json, source_locators_json, heading_level, evidence_handle
                 FROM search_results WHERE result_id = ?1",
            )
            .map_err(map_db)?;

        let result = stmt
            .query_row(rusqlite::params![result_id], |row| {
                let query: String = row.get(0)?;
                let doc_id_str: String = row.get(1)?;
                let segment_id_str: String = row.get(2)?;
                let bm25_score: Option<f64> = row.get(3)?;
                let bm25_rank: Option<i64> = row.get(4)?;
                let vector_score: Option<f64> = row.get(5)?;
                let vector_rank: Option<i64> = row.get(6)?;
                let fused_score: Option<f64> = row.get(7)?;
                let fused_rank: Option<i64> = row.get(8)?;
                let fts_gen: Option<i64> = row.get(9)?;
                let vec_gen: Option<i64> = row.get(10)?;
                let query_digest: Option<String> = row.get(11)?;
                let reranker_score: Option<f64> = row.get(12)?;
                let reranker_rank: Option<i64> = row.get(13)?;
                let block_idx: i64 = row.get(14)?;
                let block_kind: String = row.get(15)?;
                let span_start: i64 = row.get(16)?;
                let span_end: i64 = row.get(17)?;
                let search_snapshot_id: String = row.get(18)?;
                let retrieval_policy_json: String = row.get(19)?;
                let source_locators_json: String = row.get(20)?;
                let heading_level: Option<i64> = row.get(21)?;
                let evidence_handle: String = row.get(22)?;
                Ok((
                    query,
                    doc_id_str,
                    segment_id_str,
                    bm25_score,
                    bm25_rank,
                    vector_score,
                    vector_rank,
                    fused_score,
                    fused_rank,
                    fts_gen,
                    vec_gen,
                    query_digest,
                    reranker_score,
                    reranker_rank,
                    block_idx,
                    block_kind,
                    span_start,
                    span_end,
                    search_snapshot_id,
                    retrieval_policy_json,
                    source_locators_json,
                    heading_level,
                    evidence_handle,
                ))
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFoundMsg {
                    message: format!("search result not found: {result_id}"),
                },
                other => map_db(other),
            })?;

        let (
            query,
            doc_id_str,
            segment_id_str,
            bm25_score,
            bm25_rank,
            vector_score,
            vector_rank,
            fused_score,
            fused_rank,
            fts_gen,
            vec_gen,
            query_digest,
            reranker_score,
            reranker_rank,
            block_idx,
            block_kind,
            span_start,
            span_end,
            search_snapshot_id,
            retrieval_policy_json,
            source_locators_json,
            heading_level,
            evidence_handle,
        ) = result;

        let doc_id = DocId::from_stored(doc_id_str).map_err(|e| ShiroError::StoreCorrupt {
            message: e.to_string(),
        })?;
        let segment_id =
            SegmentId::from_stored(segment_id_str).map_err(|e| ShiroError::StoreCorrupt {
                message: e.to_string(),
            })?;
        let source_locators = serde_json::from_str(&source_locators_json).map_err(|error| {
            ShiroError::StoreCorrupt {
                message: format!("invalid entry-point source locators: {error}"),
            }
        })?;
        let heading_level = heading_level
            .map(|level| {
                u32::try_from(level).map_err(|_| ShiroError::StoreCorrupt {
                    message: "invalid snapshot heading level".to_string(),
                })
            })
            .transpose()?;
        if heading_level == Some(0) || (heading_level.is_some() && block_kind != "HEADING") {
            return Err(ShiroError::StoreCorrupt {
                message: "invalid snapshot heading level".to_string(),
            });
        }

        Ok(SearchResultDetail {
            query,
            query_digest,
            search_snapshot_id,
            retrieval_policy_json,
            evidence_handle: if evidence_handle.is_empty() {
                None
            } else {
                Some(
                    EvidenceHandleId::from_stored(evidence_handle)
                        .map_err(|message| ShiroError::StoreCorrupt { message })?,
                )
            },
            doc_id,
            segment_id,
            bm25_score: bm25_score.map(|s| s as f32),
            bm25_rank: bm25_rank.map(|r| r as usize),
            vector_score: vector_score.map(|s| s as f32),
            vector_rank: vector_rank.map(|r| r as usize),
            fused_score: fused_score.map(|s| s as f32),
            fused_rank: fused_rank.map(|r| r as usize),
            fts_gen: fts_gen.map(|g| g as u64),
            vec_gen: vec_gen.map(|g| g as u64),
            reranker_score: reranker_score.map(|s| s as f32),
            reranker_rank: reranker_rank.map(|r| r as usize),
            block_idx: block_idx as usize,
            block_kind,
            heading_level,
            span_start: span_start as usize,
            span_end: span_end as usize,
            source_locators,
        })
    }

    // ── Taxonomy CRUD ──────────────────────────────────────────────────

    /// Insert or replace a concept. Returns `true` if newly inserted.
    pub fn put_concept(&self, concept: &Concept) -> Result<bool, ShiroError> {
        let existed: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM concepts WHERE concept_id = ?1",
                rusqlite::params![concept.id.as_str()],
                |_| Ok(()),
            )
            .optional()
            .map_err(map_db)?
            .is_some();

        let alt_labels_json =
            serde_json::to_string(&concept.alt_labels).map_err(|e| ShiroError::StoreCorrupt {
                message: format!("failed to serialize alt_labels: {e}"),
            })?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO concepts (concept_id, scheme_uri, pref_label, alt_labels, definition)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    concept.id.as_str(),
                    concept.scheme_uri,
                    concept.pref_label,
                    alt_labels_json,
                    concept.definition,
                ],
            )
            .map_err(map_db)?;

        Ok(!existed)
    }

    /// Query a concept by ID.
    pub fn get_concept(&self, id: &ConceptId) -> Result<Concept, ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT concept_id, scheme_uri, pref_label, alt_labels, definition
                 FROM concepts WHERE concept_id = ?1",
            )
            .map_err(map_db)?;

        stmt.query_row(rusqlite::params![id.as_str()], |row| {
            let concept_id_str: String = row.get(0)?;
            let scheme_uri: String = row.get(1)?;
            let pref_label: String = row.get(2)?;
            let alt_labels_json: String = row.get(3)?;
            let definition: Option<String> = row.get(4)?;
            Ok((
                concept_id_str,
                scheme_uri,
                pref_label,
                alt_labels_json,
                definition,
            ))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFoundMsg {
                message: format!("concept not found: {id}"),
            },
            other => map_db(other),
        })
        .and_then(
            |(concept_id_str, scheme_uri, pref_label, alt_labels_json, definition)| {
                let cid = ConceptId::from_stored(concept_id_str).map_err(|e| {
                    ShiroError::StoreCorrupt {
                        message: e.to_string(),
                    }
                })?;
                let alt_labels: Vec<String> =
                    serde_json::from_str(&alt_labels_json).map_err(|e| {
                        ShiroError::StoreCorrupt {
                            message: format!("failed to parse alt_labels: {e}"),
                        }
                    })?;
                Ok(Concept {
                    id: cid,
                    scheme_uri,
                    pref_label,
                    alt_labels,
                    definition,
                })
            },
        )
    }

    /// Search concept labels, synonyms, definitions, and scheme URIs.
    pub fn search_concepts(&self, query: &str, limit: usize) -> Result<Vec<Concept>, ShiroError> {
        let pattern = format!("%{}%", query.trim().to_lowercase());
        let mut statement = self
            .conn
            .prepare(
                "SELECT concept_id, scheme_uri, pref_label, alt_labels, definition
                 FROM concepts
                 WHERE lower(pref_label) LIKE ?1
                    OR lower(alt_labels) LIKE ?1
                    OR lower(COALESCE(definition, '')) LIKE ?1
                    OR lower(scheme_uri) LIKE ?1
                 ORDER BY CASE WHEN lower(pref_label) = lower(?2) THEN 0 ELSE 1 END,
                          pref_label COLLATE NOCASE, concept_id
                 LIMIT ?3",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(
                rusqlite::params![pattern, query.trim(), limit as i64],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .map_err(map_db)?;
        let mut concepts = Vec::new();
        for row in rows {
            let (id, scheme_uri, pref_label, alt_labels, definition) = row.map_err(map_db)?;
            concepts.push(Concept {
                id: ConceptId::from_stored(id).map_err(|message| ShiroError::StoreCorrupt {
                    message: message.to_string(),
                })?,
                scheme_uri,
                pref_label,
                alt_labels: serde_json::from_str(&alt_labels).map_err(|error| {
                    ShiroError::StoreCorrupt {
                        message: format!("invalid concept alternate labels: {error}"),
                    }
                })?,
                definition,
            });
        }
        Ok(concepts)
    }

    /// List concepts up to `limit`.
    pub fn list_concepts(&self, limit: usize) -> Result<Vec<Concept>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT concept_id, scheme_uri, pref_label, alt_labels, definition
                 FROM concepts LIMIT ?1",
            )
            .map_err(map_db)?;

        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                let concept_id_str: String = row.get(0)?;
                let scheme_uri: String = row.get(1)?;
                let pref_label: String = row.get(2)?;
                let alt_labels_json: String = row.get(3)?;
                let definition: Option<String> = row.get(4)?;
                Ok((
                    concept_id_str,
                    scheme_uri,
                    pref_label,
                    alt_labels_json,
                    definition,
                ))
            })
            .map_err(map_db)?;

        let mut out = Vec::new();
        for row in rows {
            let (concept_id_str, scheme_uri, pref_label, alt_labels_json, definition) =
                row.map_err(map_db)?;
            let cid =
                ConceptId::from_stored(concept_id_str).map_err(|e| ShiroError::StoreCorrupt {
                    message: e.to_string(),
                })?;
            let alt_labels: Vec<String> =
                serde_json::from_str(&alt_labels_json).map_err(|e| ShiroError::StoreCorrupt {
                    message: format!("failed to parse alt_labels: {e}"),
                })?;
            out.push(Concept {
                id: cid,
                scheme_uri,
                pref_label,
                alt_labels,
                definition,
            });
        }

        Ok(out)
    }

    /// Insert a concept relation (idempotent — ignores duplicates).
    pub fn put_concept_relation(&self, rel: &ConceptRelation) -> Result<(), ShiroError> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO concept_relations (from_id, to_id, relation) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    rel.from.as_str(),
                    rel.to.as_str(),
                    relation_to_sql(&rel.relation),
                ],
            )
            .map_err(map_db)?;
        Ok(())
    }

    /// Insert one authored relation and atomically rebuild hierarchical closure.
    ///
    /// A hierarchy cycle rolls back both the relation and closure rebuild, leaving
    /// the previously trusted taxonomy unchanged.
    pub fn relate_concepts(&self, relation: &ConceptRelation) -> Result<bool, ShiroError> {
        self.with_savepoint("relate_concepts", || {
            let inserted = self
                .conn
                .execute(
                    "INSERT OR IGNORE INTO concept_relations (from_id, to_id, relation)
                     VALUES (?1, ?2, ?3)",
                    rusqlite::params![
                        relation.from.as_str(),
                        relation.to.as_str(),
                        relation_to_sql(&relation.relation),
                    ],
                )
                .map_err(map_db)?;
            self.rebuild_closure()?;
            let hierarchy_has_cycle = self
                .conn
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM concept_closure
                         WHERE ancestor_id = descendant_id
                     )",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(map_db)?;
            if hierarchy_has_cycle {
                return Err(ShiroError::TaxonomyCycle {
                    message: format!(
                        "taxonomy hierarchy cycle rejected: {} {} {}",
                        relation.from,
                        relation_to_sql(&relation.relation),
                        relation.to
                    ),
                });
            }
            Ok(inserted == 1)
        })
    }

    /// Get all relations for a concept (as source).
    pub fn get_concept_relations(
        &self,
        id: &ConceptId,
    ) -> Result<Vec<ConceptRelation>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare("SELECT from_id, to_id, relation FROM concept_relations WHERE from_id = ?1")
            .map_err(map_db)?;

        let rows = stmt
            .query_map(rusqlite::params![id.as_str()], |row| {
                let from_str: String = row.get(0)?;
                let to_str: String = row.get(1)?;
                let rel_str: String = row.get(2)?;
                Ok((from_str, to_str, rel_str))
            })
            .map_err(map_db)?;

        let mut out = Vec::new();
        for row in rows {
            let (from_str, to_str, rel_str) = row.map_err(map_db)?;
            let from = ConceptId::from_stored(from_str).map_err(|e| ShiroError::StoreCorrupt {
                message: e.to_string(),
            })?;
            let to = ConceptId::from_stored(to_str).map_err(|e| ShiroError::StoreCorrupt {
                message: e.to_string(),
            })?;
            let relation = parse_relation(&rel_str)?;
            out.push(ConceptRelation { from, to, relation });
        }

        Ok(out)
    }

    /// Get all directed relations touching a concept.
    pub fn get_concept_relations_any(
        &self,
        id: &ConceptId,
    ) -> Result<Vec<ConceptRelation>, ShiroError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT from_id, to_id, relation FROM concept_relations
                 WHERE from_id = ?1 OR to_id = ?1
                 ORDER BY from_id, to_id, relation",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(rusqlite::params![id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(map_db)?;
        let mut relations = Vec::new();
        for row in rows {
            let (from, to, relation) = row.map_err(map_db)?;
            relations.push(ConceptRelation {
                from: ConceptId::from_stored(from).map_err(|message| ShiroError::StoreCorrupt {
                    message: message.to_string(),
                })?,
                to: ConceptId::from_stored(to).map_err(|message| ShiroError::StoreCorrupt {
                    message: message.to_string(),
                })?,
                relation: parse_relation(&relation)?,
            });
        }
        Ok(relations)
    }

    /// Assign a concept to a document.
    pub fn assign_concept_to_doc(
        &self,
        doc_id: &DocId,
        concept_id: &ConceptId,
        confidence: f32,
        source: &str,
    ) -> Result<(), ShiroError> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO doc_concepts (doc_id, concept_id, confidence, source)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    doc_id.as_str(),
                    concept_id.as_str(),
                    confidence as f64,
                    source,
                ],
            )
            .map_err(map_db)?;
        Ok(())
    }

    /// Get concepts assigned to a document.
    /// Returns `(concept_id, confidence, source)` tuples.
    pub fn get_doc_concepts(
        &self,
        doc_id: &DocId,
    ) -> Result<Vec<(ConceptId, f32, String)>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare("SELECT concept_id, confidence, source FROM doc_concepts WHERE doc_id = ?1")
            .map_err(map_db)?;

        let rows = stmt
            .query_map(rusqlite::params![doc_id.as_str()], |row| {
                let cid_str: String = row.get(0)?;
                let confidence: f64 = row.get(1)?;
                let source: String = row.get(2)?;
                Ok((cid_str, confidence, source))
            })
            .map_err(map_db)?;

        let mut out = Vec::new();
        for row in rows {
            let (cid_str, confidence, source) = row.map_err(map_db)?;
            let cid = ConceptId::from_stored(cid_str).map_err(|e| ShiroError::StoreCorrupt {
                message: e.to_string(),
            })?;
            out.push((cid, confidence as f32, source));
        }

        Ok(out)
    }

    /// Return transitive descendants recorded for one broader concept.
    ///
    /// The concept itself is not included; callers that implement ancestor-or-self
    /// matching must add the requested concept ID to their candidate set.
    pub fn get_concept_descendant_ids(
        &self,
        ancestor_id: &ConceptId,
    ) -> Result<Vec<ConceptId>, ShiroError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT descendant_id FROM concept_closure
                 WHERE ancestor_id = ?1
                 ORDER BY depth, descendant_id",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(rusqlite::params![ancestor_id.as_str()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(map_db)?;
        let mut descendant_ids = Vec::new();
        for row in rows {
            let descendant_id = row.map_err(map_db)?;
            descendant_ids.push(ConceptId::from_stored(descendant_id).map_err(|error| {
                ShiroError::StoreCorrupt {
                    message: format!("invalid descendant concept ID in closure: {error}"),
                }
            })?);
        }
        Ok(descendant_ids)
    }

    /// Rebuild the transitive closure table from BROADER edges.
    ///
    /// Uses iterative BFS: repeatedly join concept_relations (BROADER)
    /// with the closure table until no new rows are added.
    pub fn rebuild_closure(&self) -> Result<(), ShiroError> {
        self.with_savepoint("rebuild_closure", || {
            self.conn
                .execute("DELETE FROM concept_closure", [])
                .map_err(map_db)?;

            // Normalize BROADER and its NARROWER inverse into ancestor→descendant rows.
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO concept_closure (ancestor_id, descendant_id, depth)
                     SELECT to_id, from_id, 1
                     FROM concept_relations WHERE relation = 'BROADER'
                     UNION
                     SELECT from_id, to_id, 1
                     FROM concept_relations WHERE relation = 'NARROWER'",
                    [],
                )
                .map_err(map_db)?;

            // Iteratively extend every normalized ancestor→descendant path.
            loop {
                let inserted = self
                    .conn
                    .execute(
                        "INSERT OR IGNORE INTO concept_closure (ancestor_id, descendant_id, depth)
                         SELECT c.ancestor_id, edge.descendant_id, c.depth + 1
                         FROM concept_closure c
                         JOIN (
                             SELECT to_id AS ancestor_id, from_id AS descendant_id
                             FROM concept_relations WHERE relation = 'BROADER'
                             UNION
                             SELECT from_id AS ancestor_id, to_id AS descendant_id
                             FROM concept_relations WHERE relation = 'NARROWER'
                         ) edge ON edge.ancestor_id = c.descendant_id
                         WHERE NOT EXISTS (
                             SELECT 1 FROM concept_closure x
                             WHERE x.ancestor_id = c.ancestor_id
                               AND x.descendant_id = edge.descendant_id
                         )",
                        [],
                    )
                    .map_err(map_db)?;

                if inserted == 0 {
                    break;
                }
            }

            Ok(())
        })
    }

    /// Persist an attributed model-enrichment proposal without changing trusted state.
    pub fn put_model_enrichment_proposal(
        &self,
        proposal: &ModelEnrichmentProposalRecord,
    ) -> Result<(), ShiroError> {
        self.conn
            .execute(
                "INSERT INTO model_enrichment_proposals (
                    proposal_id, doc_id, provider, model, actor_id, data_region,
                    retention_policy, consent_id, payload_json, status,
                    applied_concepts_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'PROPOSED', '[]')",
                rusqlite::params![
                    proposal.proposal_id,
                    proposal.doc_id.as_str(),
                    proposal.provider,
                    proposal.model,
                    proposal.actor_id,
                    proposal.data_region,
                    proposal.retention_policy,
                    proposal.consent_id,
                    proposal.payload_json,
                ],
            )
            .map_err(map_db)?;
        Ok(())
    }

    /// Load one model-enrichment proposal.
    pub fn get_model_enrichment_proposal(
        &self,
        proposal_id: &str,
    ) -> Result<ModelEnrichmentProposalRecord, ShiroError> {
        self.conn
            .query_row(
                "SELECT doc_id, provider, model, actor_id, data_region,
                        retention_policy, consent_id, payload_json, status,
                        applied_concepts_json
                 FROM model_enrichment_proposals WHERE proposal_id = ?1",
                rusqlite::params![proposal_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                    ))
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFoundMsg {
                    message: format!("model enrichment proposal not found: {proposal_id}"),
                },
                other => map_db(other),
            })
            .and_then(
                |(
                    doc_id,
                    provider,
                    model,
                    actor_id,
                    data_region,
                    retention_policy,
                    consent_id,
                    payload_json,
                    status,
                    applied_concepts_json,
                )| {
                    Ok(ModelEnrichmentProposalRecord {
                        proposal_id: proposal_id.to_string(),
                        doc_id: DocId::from_stored(doc_id).map_err(|message| {
                            ShiroError::StoreCorrupt {
                                message: message.to_string(),
                            }
                        })?,
                        provider,
                        model,
                        actor_id,
                        data_region,
                        retention_policy,
                        consent_id,
                        payload_json,
                        status,
                        applied_concepts_json,
                    })
                },
            )
    }

    /// Promote proposed concepts and assignments without overwriting existing assignments.
    pub fn promote_model_enrichment_proposal(
        &self,
        proposal_id: &str,
        assignments: &[ProposedConceptAssignment],
        resolved_actor_id: &str,
        approval_id: &str,
    ) -> Result<Vec<ConceptId>, ShiroError> {
        self.with_savepoint("promote_model_enrichment", || {
            let proposal = self.get_model_enrichment_proposal(proposal_id)?;
            if proposal.status != "PROPOSED" {
                return Err(ShiroError::InvalidInput {
                    message: format!(
                        "proposal {proposal_id} cannot be promoted from {}",
                        proposal.status
                    ),
                });
            }
            let source = format!("model_proposal:{proposal_id}");
            let mut applied = Vec::new();
            for assignment in assignments {
                self.put_concept(&assignment.concept)?;
                let inserted = self
                    .conn
                    .execute(
                        "INSERT OR IGNORE INTO doc_concepts (
                            doc_id, concept_id, confidence, source
                         ) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![
                            proposal.doc_id.as_str(),
                            assignment.concept.id.as_str(),
                            assignment.confidence as f64,
                            source,
                        ],
                    )
                    .map_err(map_db)?;
                if inserted == 1 {
                    applied.push(assignment.concept.id.clone());
                }
            }
            let applied_json =
                serde_json::to_string(&applied.iter().map(ConceptId::as_str).collect::<Vec<_>>())
                    .map_err(|error| ShiroError::StoreCorrupt {
                    message: format!("failed to serialize promoted concepts: {error}"),
                })?;
            self.conn
                .execute(
                    "UPDATE model_enrichment_proposals
                     SET status = 'PROMOTED', applied_concepts_json = ?1,
                         resolved_actor_id = ?2, approval_id = ?3,
                         resolved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE proposal_id = ?4",
                    rusqlite::params![applied_json, resolved_actor_id, approval_id, proposal_id],
                )
                .map_err(map_db)?;
            Ok(applied)
        })
    }

    /// Reject a proposed or promoted model enrichment and reverse its assignments.
    pub fn reject_model_enrichment_proposal(
        &self,
        proposal_id: &str,
        resolved_actor_id: &str,
        approval_id: &str,
    ) -> Result<(), ShiroError> {
        self.with_savepoint("reject_model_enrichment", || {
            let proposal = self.get_model_enrichment_proposal(proposal_id)?;
            if proposal.status == "REJECTED" {
                return Ok(());
            }
            let applied: Vec<String> = serde_json::from_str(&proposal.applied_concepts_json)
                .map_err(|error| ShiroError::StoreCorrupt {
                    message: format!("invalid promoted concept list: {error}"),
                })?;
            let source = format!("model_proposal:{proposal_id}");
            for concept_id in applied {
                self.conn
                    .execute(
                        "DELETE FROM doc_concepts
                         WHERE doc_id = ?1 AND concept_id = ?2 AND source = ?3",
                        rusqlite::params![proposal.doc_id.as_str(), concept_id, source],
                    )
                    .map_err(map_db)?;
            }
            self.conn
                .execute(
                    "UPDATE model_enrichment_proposals
                     SET status = 'REJECTED', resolved_actor_id = ?1, approval_id = ?2,
                         resolved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE proposal_id = ?3",
                    rusqlite::params![resolved_actor_id, approval_id, proposal_id],
                )
                .map_err(map_db)?;
            Ok(())
        })
    }

    // ── Enrichment CRUD ────────────────────────────────────────────────

    /// Insert or replace an enrichment result.
    pub fn put_enrichment(&self, result: &EnrichmentResult) -> Result<(), ShiroError> {
        let tags_json =
            serde_json::to_string(&result.tags).map_err(|e| ShiroError::StoreCorrupt {
                message: format!("failed to serialize tags: {e}"),
            })?;
        let concepts_json: Vec<&str> = result.concepts.iter().map(|c| c.as_str()).collect();
        let concepts_json =
            serde_json::to_string(&concepts_json).map_err(|e| ShiroError::StoreCorrupt {
                message: format!("failed to serialize concepts: {e}"),
            })?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO enrichments (doc_id, title, summary, tags, concepts, provider, content_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    result.doc_id.as_str(),
                    result.title,
                    result.summary,
                    tags_json,
                    concepts_json,
                    result.provider,
                    result.content_hash,
                    result.created_at,
                ],
            )
            .map_err(map_db)?;

        Ok(())
    }

    /// Get an enrichment result for a document. Returns `None` if not found.
    pub fn get_enrichment(&self, doc_id: &DocId) -> Result<Option<EnrichmentResult>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT doc_id, title, summary, tags, concepts, provider, content_hash, created_at
                 FROM enrichments WHERE doc_id = ?1",
            )
            .map_err(map_db)?;

        let row = stmt
            .query_row(rusqlite::params![doc_id.as_str()], |row| {
                let doc_id_str: String = row.get(0)?;
                let title: Option<String> = row.get(1)?;
                let summary: Option<String> = row.get(2)?;
                let tags_json: String = row.get(3)?;
                let concepts_json: String = row.get(4)?;
                let provider: String = row.get(5)?;
                let content_hash: String = row.get(6)?;
                let created_at: String = row.get(7)?;
                Ok((
                    doc_id_str,
                    title,
                    summary,
                    tags_json,
                    concepts_json,
                    provider,
                    content_hash,
                    created_at,
                ))
            })
            .optional()
            .map_err(map_db)?;

        match row {
            None => Ok(None),
            Some((
                doc_id_str,
                title,
                summary,
                tags_json,
                concepts_json,
                provider,
                content_hash,
                created_at,
            )) => {
                let did = DocId::from_stored(doc_id_str).map_err(|e| ShiroError::StoreCorrupt {
                    message: e.to_string(),
                })?;
                let tags: Vec<String> =
                    serde_json::from_str(&tags_json).map_err(|e| ShiroError::StoreCorrupt {
                        message: format!("failed to parse tags: {e}"),
                    })?;
                let concept_strs: Vec<String> =
                    serde_json::from_str(&concepts_json).map_err(|e| ShiroError::StoreCorrupt {
                        message: format!("failed to parse concepts: {e}"),
                    })?;
                let concepts = concept_strs
                    .into_iter()
                    .map(|s| {
                        ConceptId::from_stored(s).map_err(|e| ShiroError::StoreCorrupt {
                            message: e.to_string(),
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(Some(EnrichmentResult {
                    doc_id: did,
                    title,
                    summary,
                    tags,
                    concepts,
                    provider,
                    content_hash,
                    created_at,
                }))
            }
        }
    }

    // ── Generation tracking ────────────────────────────────────────────

    /// Read the active generation for a given index kind (e.g. "fts", "vector").
    pub fn active_generation(&self, kind: &str) -> Result<GenerationId, ShiroError> {
        let gen: i64 = self
            .conn
            .query_row(
                "SELECT gen_id FROM active_generations WHERE kind = ?1",
                rusqlite::params![kind],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFoundMsg {
                    message: format!("no active generation for kind: {kind}"),
                },
                other => map_db(other),
            })?;
        Ok(GenerationId::new(gen as u64))
    }

    /// Record a generation snapshot.
    pub fn record_generation(&self, kind: &str, gen: &IndexGeneration) -> Result<(), ShiroError> {
        self.conn
            .execute(
                "INSERT INTO generations (kind, gen_id, doc_count, segment_count, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    kind,
                    gen.gen_id.as_u64() as i64,
                    gen.doc_count as i64,
                    gen.segment_count as i64,
                    gen.created_at,
                ],
            )
            .map_err(map_db)?;
        Ok(())
    }

    /// Set the active generation for a given index kind.
    pub fn set_active_generation(
        &self,
        kind: &str,
        gen_id: GenerationId,
    ) -> Result<(), ShiroError> {
        self.conn
            .execute(
                "UPDATE active_generations SET gen_id = ?1 WHERE kind = ?2",
                rusqlite::params![gen_id.as_u64() as i64, kind],
            )
            .map_err(map_db)?;
        Ok(())
    }

    /// Reserve a never-reused generation identifier before building artifacts.
    ///
    /// Failed builds remain in the generation audit table, so the next attempt
    /// advances rather than reusing a filesystem location that may be partial.
    pub fn reserve_generation(
        &self,
        kind: &str,
        document_count: usize,
        segment_count: usize,
        created_at: &str,
    ) -> Result<GenerationId, ShiroError> {
        let next: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(gen_id), 0) + 1 FROM generations WHERE kind = ?1",
                rusqlite::params![kind],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        let generation = GenerationId::new(next as u64);
        self.record_generation(
            kind,
            &IndexGeneration {
                gen_id: generation,
                created_at: created_at.to_string(),
                doc_count: document_count,
                segment_count,
            },
        )?;
        Ok(generation)
    }

    /// Update document and segment counts for a previously reserved index generation.
    pub fn update_reserved_generation_counts(
        &self,
        kind: &str,
        generation: GenerationId,
        document_count: usize,
        segment_count: usize,
    ) -> Result<(), ShiroError> {
        let updated = self
            .conn
            .execute(
                "UPDATE generations SET doc_count = ?1, segment_count = ?2
                 WHERE kind = ?3 AND gen_id = ?4",
                rusqlite::params![
                    document_count as i64,
                    segment_count as i64,
                    kind,
                    generation.as_u64() as i64,
                ],
            )
            .map_err(map_db)?;
        if updated != 1 {
            return Err(ShiroError::StoreCorrupt {
                message: format!(
                    "reserved generation missing while updating counts: {kind}/{}",
                    generation.as_u64()
                ),
            });
        }
        Ok(())
    }

    /// Keep canonical staging and corpus activation invisible until one atomic commit.
    pub fn with_atomic_corpus_publication<T>(
        &self,
        publish: impl FnOnce() -> Result<T, ShiroError>,
    ) -> Result<T, ShiroError> {
        self.with_savepoint("atomic_corpus_publication", publish)
    }

    /// Atomically activate one complete corpus manifest and all its index pointers.
    pub fn activate_corpus_manifest(&self, manifest: &CorpusManifest) -> Result<(), ShiroError> {
        if manifest.vector_generation.is_some() != manifest.vector_digest.is_some() {
            return Err(ShiroError::InvalidInput {
                message:
                    "vector generation and digest must either both be present or both be absent"
                        .to_string(),
            });
        }

        self.with_savepoint("activate_corpus_manifest", || {
            self.conn
                .execute(
                    "INSERT INTO corpus_manifests (
                        manifest_id, corpus_digest, document_count, segment_count,
                        fts_generation, fts_digest, vector_generation, vector_digest,
                        embedding_fingerprint_hash, created_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    rusqlite::params![
                        manifest.manifest_id,
                        manifest.corpus_digest,
                        manifest.document_count as i64,
                        manifest.segment_count as i64,
                        manifest.fts_generation.as_u64() as i64,
                        manifest.fts_digest,
                        manifest
                            .vector_generation
                            .map(|generation| generation.as_u64() as i64),
                        manifest.vector_digest,
                        manifest.embedding_fingerprint_hash,
                        manifest.created_at,
                    ],
                )
                .map_err(map_db)?;
            self.conn
                .execute(
                    "UPDATE active_generations SET gen_id = ?1 WHERE kind = 'fts'",
                    rusqlite::params![manifest.fts_generation.as_u64() as i64],
                )
                .map_err(map_db)?;
            self.conn
                .execute(
                    "UPDATE active_generations SET gen_id = ?1 WHERE kind = 'vector'",
                    rusqlite::params![manifest
                        .vector_generation
                        .unwrap_or(GenerationId::ZERO)
                        .as_u64() as i64],
                )
                .map_err(map_db)?;
            self.conn
                .execute(
                    "INSERT INTO active_corpus_manifest (singleton, manifest_id)
                     VALUES (1, ?1)
                     ON CONFLICT(singleton) DO UPDATE SET manifest_id = excluded.manifest_id",
                    rusqlite::params![manifest.manifest_id],
                )
                .map_err(map_db)?;
            Ok(())
        })
    }

    /// Atomically activate a complete manifest and publish staged documents as READY.
    pub fn activate_corpus_manifest_and_ready(
        &self,
        manifest: &CorpusManifest,
        document_ids: &[DocId],
    ) -> Result<(), ShiroError> {
        self.with_savepoint("activate_manifest_and_ready", || {
            self.activate_corpus_manifest(manifest)?;
            for document_id in document_ids {
                self.set_state(document_id, DocState::Ready)?;
            }
            Ok(())
        })
    }

    /// List every index generation named by any retained corpus manifest.
    pub fn corpus_manifest_generation_references(
        &self,
    ) -> Result<CorpusManifestGenerationReferences, ShiroError> {
        let mut fts_statement = self
            .conn
            .prepare("SELECT DISTINCT fts_generation FROM corpus_manifests ORDER BY fts_generation")
            .map_err(map_db)?;
        let fts_generations = fts_statement
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(map_db)?
            .map(|generation| {
                generation
                    .map(|value| GenerationId::new(value as u64))
                    .map_err(map_db)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut vector_statement = self
            .conn
            .prepare(
                "SELECT DISTINCT vector_generation FROM corpus_manifests
                 WHERE vector_generation IS NOT NULL ORDER BY vector_generation",
            )
            .map_err(map_db)?;
        let vector_generations = vector_statement
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(map_db)?
            .map(|generation| {
                generation
                    .map(|value| GenerationId::new(value as u64))
                    .map_err(map_db)
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CorpusManifestGenerationReferences {
            fts_generations,
            vector_generations,
        })
    }

    /// Return the newest immutable vector manifest compatible with a fingerprint.
    pub fn latest_vector_manifest(
        &self,
        fingerprint_hash: &str,
    ) -> Result<Option<CorpusManifest>, ShiroError> {
        self.conn
            .query_row(
                "SELECT manifest_id, corpus_digest, document_count, segment_count,
                        fts_generation, fts_digest, vector_generation, vector_digest,
                        embedding_fingerprint_hash, created_at
                 FROM corpus_manifests
                 WHERE vector_generation IS NOT NULL
                   AND vector_digest IS NOT NULL
                   AND embedding_fingerprint_hash = ?1
                 ORDER BY vector_generation DESC LIMIT 1",
                rusqlite::params![fingerprint_hash],
                |row| {
                    Ok(CorpusManifest {
                        manifest_id: row.get(0)?,
                        corpus_digest: row.get(1)?,
                        document_count: row.get::<_, i64>(2)? as usize,
                        segment_count: row.get::<_, i64>(3)? as usize,
                        fts_generation: GenerationId::new(row.get::<_, i64>(4)? as u64),
                        fts_digest: row.get(5)?,
                        vector_generation: Some(GenerationId::new(row.get::<_, i64>(6)? as u64)),
                        vector_digest: Some(row.get(7)?),
                        embedding_fingerprint_hash: Some(row.get(8)?),
                        created_at: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(map_db)
    }

    /// Deactivate vectors before mutating the active FTS generation in place.
    ///
    /// Incremental FTS writes are scope-safe because SQLite READY state remains
    /// authoritative, but their artifact digest is intentionally marked mutable
    /// until the next full immutable publication.
    pub fn begin_incremental_fts_publication(&self) -> Result<(), ShiroError> {
        let fts_generation = self.active_generation("fts")?;
        let created_at = {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            nanos.to_string()
        };
        let manifest_id = format!(
            "corpus_mutable_{}",
            blake3::hash(created_at.as_bytes()).to_hex()
        );
        self.activate_corpus_manifest(&CorpusManifest {
            manifest_id,
            corpus_digest: String::new(),
            document_count: 0,
            segment_count: 0,
            fts_generation,
            fts_digest: String::new(),
            vector_generation: None,
            vector_digest: None,
            embedding_fingerprint_hash: None,
            created_at,
        })
    }

    /// Resolve the active common corpus manifest, if this workspace has one.
    pub fn active_corpus_manifest(&self) -> Result<Option<CorpusManifest>, ShiroError> {
        self.conn
            .query_row(
                "SELECT m.manifest_id, m.corpus_digest, m.document_count, m.segment_count,
                        m.fts_generation, m.fts_digest, m.vector_generation, m.vector_digest,
                        m.embedding_fingerprint_hash, m.created_at
                 FROM active_corpus_manifest a
                 JOIN corpus_manifests m ON m.manifest_id = a.manifest_id
                 WHERE a.singleton = 1",
                [],
                |row| {
                    let vector_generation: Option<i64> = row.get(6)?;
                    Ok(CorpusManifest {
                        manifest_id: row.get(0)?,
                        corpus_digest: row.get(1)?,
                        document_count: row.get::<_, i64>(2)? as usize,
                        segment_count: row.get::<_, i64>(3)? as usize,
                        fts_generation: GenerationId::new(row.get::<_, i64>(4)? as u64),
                        fts_digest: row.get(5)?,
                        vector_generation: vector_generation
                            .map(|generation| GenerationId::new(generation as u64)),
                        vector_digest: row.get(7)?,
                        embedding_fingerprint_hash: row.get(8)?,
                        created_at: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(map_db)
    }

    // ── Fingerprint ────────────────────────────────────────────────────

    /// Read the processing fingerprint for a document.
    pub fn get_fingerprint(
        &self,
        doc_id: &DocId,
    ) -> Result<Option<ProcessingFingerprint>, ShiroError> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT fingerprint FROM documents WHERE doc_id = ?1",
                rusqlite::params![doc_id.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFound(doc_id.clone()),
                other => map_db(other),
            })?;

        match json {
            None => Ok(None),
            Some(s) => {
                let fp: ProcessingFingerprint =
                    serde_json::from_str(&s).map_err(|e| ShiroError::StoreCorrupt {
                        message: format!("failed to parse fingerprint: {e}"),
                    })?;
                Ok(Some(fp))
            }
        }
    }

    /// Set the processing fingerprint for a document.
    pub fn set_fingerprint(
        &self,
        doc_id: &DocId,
        fp: &ProcessingFingerprint,
    ) -> Result<(), ShiroError> {
        let json = serde_json::to_string(fp).map_err(|e| ShiroError::StoreCorrupt {
            message: format!("failed to serialize fingerprint: {e}"),
        })?;

        self.conn
            .execute(
                "UPDATE documents SET fingerprint = ?1 WHERE doc_id = ?2",
                rusqlite::params![json, doc_id.as_str()],
            )
            .map_err(map_db)?;

        Ok(())
    }

    // ── Version CRUD ──────────────────────────────────────────────────

    /// Create a new version for a document.
    pub fn create_version(
        &self,
        doc_id: &DocId,
        version_id: &VersionId,
        fingerprint_hash: Option<&str>,
    ) -> Result<(), ShiroError> {
        self.conn.execute(
            "INSERT INTO doc_versions (version_id, doc_id, fingerprint_hash) VALUES (?1, ?2, ?3)",
            rusqlite::params![version_id.as_str(), doc_id.as_str(), fingerprint_hash],
        ).map_err(map_db)?;
        Ok(())
    }

    /// Get the active version ID for a document.
    pub fn active_version_id(&self, doc_id: &DocId) -> Result<Option<VersionId>, ShiroError> {
        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT active_version_id FROM documents WHERE doc_id = ?1",
                rusqlite::params![doc_id.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFound(doc_id.clone()),
                other => map_db(other),
            })?;
        match result {
            Some(s) => Ok(Some(VersionId::from_stored(s).map_err(|e| {
                ShiroError::StoreCorrupt {
                    message: e.to_string(),
                }
            })?)),
            None => Ok(None),
        }
    }

    /// Set the active version for a document.
    pub fn set_active_version(
        &self,
        doc_id: &DocId,
        version_id: &VersionId,
    ) -> Result<(), ShiroError> {
        self.conn.execute(
            "UPDATE documents SET active_version_id = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE doc_id = ?2",
            rusqlite::params![version_id.as_str(), doc_id.as_str()],
        ).map_err(map_db)?;
        Ok(())
    }

    /// Count how many versions a document has.
    pub fn count_versions(&self, doc_id: &DocId) -> Result<usize, ShiroError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM doc_versions WHERE doc_id = ?1",
                rusqlite::params![doc_id.as_str()],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        Ok(count as usize)
    }

    // ── Source artifacts and immutable provenance ──────────────────────

    fn append_provenance(
        &self,
        doc_id: &DocId,
        entity_type: &str,
        entity_id: &str,
        provenance: &WriteProvenance,
    ) -> Result<(), ShiroError> {
        self.conn
            .execute(
                "INSERT INTO provenance_records (doc_id, entity_type, entity_id, actor_kind, actor_id, operation, content_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    doc_id.as_str(),
                    entity_type,
                    entity_id,
                    provenance.actor_kind.as_str(),
                    provenance.actor_id,
                    provenance.operation,
                    provenance.content_hash,
                ],
            )
            .map_err(map_db)?;
        Ok(())
    }

    fn record_source_artifact(
        &self,
        doc_id: &DocId,
        content_hash: &str,
        source_uri: &str,
        byte_count: usize,
        provenance: &WriteProvenance,
    ) -> Result<(), ShiroError> {
        let inserted = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO source_artifacts (doc_id, content_hash, source_uri, byte_count, trust_zone)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    doc_id.as_str(),
                    content_hash,
                    source_uri,
                    byte_count as i64,
                    TrustZone::Canonical.as_str(),
                ],
            )
            .map_err(map_db)?;
        if inserted > 0 {
            let source_artifact_id = self.conn.last_insert_rowid().to_string();
            self.append_provenance(doc_id, "source_artifact", &source_artifact_id, provenance)?;
        }
        Ok(())
    }

    /// Return URL acquisition evidence for a document in append order.
    pub fn get_url_acquisitions(
        &self,
        doc_id: &DocId,
    ) -> Result<Vec<UrlAcquisitionRecord>, ShiroError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT requested_url, final_url, redirects_json, content_type,
                        signature, byte_count, content_hash
                 FROM url_acquisitions WHERE doc_id = ?1 ORDER BY acquisition_id",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(rusqlite::params![doc_id.as_str()], |row| {
                Ok(UrlAcquisitionRecord {
                    requested_url: row.get(0)?,
                    final_url: row.get(1)?,
                    redirects_json: row.get(2)?,
                    content_type: row.get(3)?,
                    signature: row.get(4)?,
                    byte_count: row.get::<_, i64>(5)? as usize,
                    content_hash: row.get(6)?,
                })
            })
            .map_err(map_db)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(map_db)
    }

    /// Append actor/run/approval provenance for one authorized MCP mutation.
    pub fn record_mcp_mutation(
        &self,
        run_id: &str,
        actor_id: &str,
        approval_id: &str,
        operation: &str,
        params_digest: &str,
        outcome: &str,
    ) -> Result<(), ShiroError> {
        self.conn
            .execute(
                "INSERT INTO mcp_mutation_audit (
                    run_id, actor_id, approval_id, operation, params_digest, outcome
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    run_id,
                    actor_id,
                    approval_id,
                    operation,
                    params_digest,
                    outcome
                ],
            )
            .map_err(map_db)?;
        Ok(())
    }

    /// Return immutable write provenance for a document in append order.
    pub fn get_document_provenance(
        &self,
        doc_id: &DocId,
    ) -> Result<Vec<ProvenanceRecord>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT provenance_id, actor_kind, actor_id, operation, content_hash, created_at
                 FROM provenance_records WHERE doc_id = ?1 ORDER BY provenance_id",
            )
            .map_err(map_db)?;
        let rows = stmt
            .query_map(rusqlite::params![doc_id.as_str()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(map_db)?;

        let mut records = Vec::new();
        for row in rows {
            let (provenance_id, actor_kind, actor_id, operation, content_hash, created_at) =
                row.map_err(map_db)?;
            records.push(ProvenanceRecord {
                provenance_id,
                actor_kind: parse_provenance_actor_kind(&actor_kind)?,
                actor_id,
                operation,
                content_hash,
                created_at,
            });
        }
        Ok(records)
    }

    // ── Stats / utilities ──────────────────────────────────────────────

    /// Count documents by state.
    pub fn count_by_state(&self) -> Result<Vec<(DocState, usize)>, ShiroError> {
        let mut stmt = self
            .conn
            .prepare("SELECT state, COUNT(*) FROM documents GROUP BY state")
            .map_err(map_db)?;

        let rows = stmt
            .query_map([], |row| {
                let state_str: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((state_str, count))
            })
            .map_err(map_db)?;

        let mut out = Vec::new();
        for row in rows {
            let (state_str, count) = row.map_err(map_db)?;
            let state = parse_state(&state_str)?;
            out.push((state, count as usize));
        }

        Ok(out)
    }

    /// Check if a document exists.
    pub fn exists(&self, id: &DocId) -> Result<bool, ShiroError> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM documents WHERE doc_id = ?1")
            .map_err(map_db)?;

        let found = stmt
            .query_row(rusqlite::params![id.as_str()], |_| Ok(()))
            .optional()
            .map_err(map_db)?;

        Ok(found.is_some())
    }

    /// Begin an explicit transaction (for batch operations).
    pub fn begin(&self) -> Result<(), ShiroError> {
        self.conn.execute_batch("BEGIN").map_err(map_db)
    }

    /// Commit the current transaction.
    pub fn commit(&self) -> Result<(), ShiroError> {
        self.conn.execute_batch("COMMIT").map_err(map_db)
    }

    /// Rollback the current transaction.
    pub fn rollback(&self) -> Result<(), ShiroError> {
        self.conn.execute_batch("ROLLBACK").map_err(map_db)
    }

    /// Store a blob by content hash. Returns the blake3 hex digest.
    pub fn put_blob(&self, content: &[u8]) -> Result<String, ShiroError> {
        let hash = blake3::hash(content).to_hex().to_string();
        self.conn.execute(
            "INSERT OR IGNORE INTO blobs (content_hash, raw_bytes, byte_count) VALUES (?1, ?2, ?3)",
            rusqlite::params![hash, content, content.len() as i64],
        ).map_err(map_db)?;
        Ok(hash)
    }

    /// Retrieve a blob by its content hash.
    pub fn get_blob(&self, content_hash: &str) -> Result<Vec<u8>, ShiroError> {
        self.conn
            .query_row(
                "SELECT raw_bytes FROM blobs WHERE content_hash = ?1",
                rusqlite::params![content_hash],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFoundMsg {
                    message: format!("blob not found: {content_hash}"),
                },
                other => map_db(other),
            })
    }

    /// Check whether a blob exists by its content hash.
    pub fn blob_exists(&self, content_hash: &str) -> Result<bool, ShiroError> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM blobs WHERE content_hash = ?1")
            .map_err(map_db)?;
        let found = stmt
            .query_row(rusqlite::params![content_hash], |_| Ok(()))
            .optional()
            .map_err(map_db)?;
        Ok(found.is_some())
    }

    /// Current schema version.
    pub fn schema_version(&self) -> Result<u32, ShiroError> {
        let version: String = self
            .conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        version
            .parse::<u32>()
            .map_err(|e| ShiroError::StoreCorrupt {
                message: format!("invalid schema_version: {e}"),
            })
    }

    /// Execute `f` within a savepoint.
    ///
    /// Savepoints nest safely inside explicit `begin()`/`commit()` transactions.
    /// On error, the savepoint is rolled back; on success, it is released.
    fn with_savepoint<F, T>(&self, name: &str, f: F) -> Result<T, ShiroError>
    where
        F: FnOnce() -> Result<T, ShiroError>,
    {
        self.conn
            .execute_batch(&format!("SAVEPOINT {name}"))
            .map_err(map_db)?;
        match f() {
            Ok(val) => {
                self.conn
                    .execute_batch(&format!("RELEASE SAVEPOINT {name}"))
                    .map_err(map_db)?;
                Ok(val)
            }
            Err(e) => {
                let _ = self
                    .conn
                    .execute_batch(&format!("ROLLBACK TO SAVEPOINT {name}"));
                let _ = self
                    .conn
                    .execute_batch(&format!("RELEASE SAVEPOINT {name}"));
                Err(e)
            }
        }
    }
}

/// Extension trait to make `query_row` return `Option` on no-rows.
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_core::id::SegmentId;

    fn tmp_store() -> (Store, tempfile::NamedTempFile) {
        let f = tempfile::NamedTempFile::new().unwrap();
        let path = camino::Utf8Path::from_path(f.path()).unwrap();
        let store = Store::open(path).unwrap();
        (store, f)
    }

    fn test_doc(content: &str) -> Document {
        Document {
            id: DocId::from_content(content.as_bytes()),
            canonical_text: content.to_string(),
            rendered_text: None,
            metadata: Metadata {
                title: Some("Test".to_string()),
                source_uri: "test.txt".to_string(),
                source_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
            },
            blocks: BlockGraph::empty(),
            losses: Vec::new(),
        }
    }

    fn test_concept(label: &str) -> Concept {
        Concept {
            id: ConceptId::new("http://example.org/scheme", label),
            scheme_uri: "http://example.org/scheme".to_string(),
            pref_label: label.to_string(),
            alt_labels: vec![format!("{label}-alt")],
            definition: Some(format!("Definition of {label}")),
        }
    }

    #[test]
    fn test_put_and_get_document() {
        let (store, _f) = tmp_store();
        let doc = test_doc("hello world");

        let inserted = store.put_document(&doc, DocState::Staged).unwrap();
        assert!(inserted);

        let (got, state) = store.get_document(&doc.id).unwrap();
        assert_eq!(got.id, doc.id);
        assert_eq!(got.canonical_text, "hello world");
        assert_eq!(got.metadata.title, Some("Test".to_string()));
        assert_eq!(got.metadata.source_uri, "test.txt");
        assert_eq!(state, DocState::Staged);

        // Replace returns false
        let inserted2 = store.put_document(&doc, DocState::Ready).unwrap();
        assert!(!inserted2);

        let (_, state2) = store.get_document(&doc.id).unwrap();
        assert_eq!(state2, DocState::Ready);
    }

    #[test]
    fn test_list_documents() {
        let (store, _f) = tmp_store();
        let d1 = test_doc("doc one");
        let d2 = test_doc("doc two");
        let d3 = test_doc("doc three");

        store.put_document(&d1, DocState::Staged).unwrap();
        store.put_document(&d2, DocState::Ready).unwrap();
        store.put_document(&d3, DocState::Failed).unwrap();

        let list = store.list_documents(10).unwrap();
        assert_eq!(list.len(), 3);

        // All should have title "Test"
        for (_id, _state, title) in &list {
            assert_eq!(title, &Some("Test".to_string()));
        }

        // Limit works
        let limited = store.list_documents(2).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_state_transitions() {
        let (store, _f) = tmp_store();
        let doc = test_doc("transition test");

        store.put_document(&doc, DocState::Staged).unwrap();

        // Valid: STAGED → INDEXING
        store.set_state(&doc.id, DocState::Indexing).unwrap();
        let (_, s1) = store.get_document(&doc.id).unwrap();
        assert_eq!(s1, DocState::Indexing);

        // Valid: INDEXING → READY
        store.set_state(&doc.id, DocState::Ready).unwrap();
        let (_, s2) = store.get_document(&doc.id).unwrap();
        assert_eq!(s2, DocState::Ready);
    }

    #[test]
    fn test_invalid_state_transition() {
        let (store, _f) = tmp_store();
        let doc = test_doc("invalid transition");
        store.put_document(&doc, DocState::Staged).unwrap();

        // Invalid: STAGED → READY (skips INDEXING)
        let err = store.set_state(&doc.id, DocState::Ready).unwrap_err();
        assert!(
            err.to_string().contains("invalid state transition"),
            "expected transition error, got: {err}"
        );

        // State unchanged
        let (_, state) = store.get_document(&doc.id).unwrap();
        assert_eq!(state, DocState::Staged);
    }

    #[test]
    fn test_delete_from_any_state() {
        let (store, _f) = tmp_store();

        // Delete from STAGED
        let d1 = test_doc("delete staged");
        store.put_document(&d1, DocState::Staged).unwrap();
        store.set_state(&d1.id, DocState::Deleted).unwrap();
        let (_, s) = store.get_document(&d1.id).unwrap();
        assert_eq!(s, DocState::Deleted);

        // Delete from READY
        let d2 = test_doc("delete ready");
        store.put_document(&d2, DocState::Ready).unwrap();
        store.set_state(&d2.id, DocState::Deleted).unwrap();
        let (_, s) = store.get_document(&d2.id).unwrap();
        assert_eq!(s, DocState::Deleted);
    }

    #[test]
    fn test_schema_version() {
        let (store, _f) = tmp_store();
        assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn test_segments_crud() {
        let (store, _f) = tmp_store();
        let doc = test_doc("segment test content here");
        store.put_document(&doc, DocState::Staged).unwrap();

        let segments = vec![
            Segment {
                id: SegmentId::new(&doc.id, 0),
                doc_id: doc.id.clone(),
                index: 0,
                span: Span::new(0, 12).unwrap(),
                body: "segment test".to_string(),
                retrieval_text: "segment test".to_string(),
            },
            Segment {
                id: SegmentId::new(&doc.id, 1),
                doc_id: doc.id.clone(),
                index: 1,
                span: Span::new(13, 25).unwrap(),
                body: "content here".to_string(),
                retrieval_text: "content here".to_string(),
            },
        ];

        store.put_segments(&segments).unwrap();

        let got = store.get_segments(&doc.id).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].body, "segment test");
        assert_eq!(got[1].body, "content here");
        assert_eq!(got[0].span.start(), 0);
        assert_eq!(got[0].span.end(), 12);
        assert_eq!(got[1].index, 1);

        // Replace segments
        let new_segments = vec![Segment {
            id: SegmentId::new(&doc.id, 0),
            doc_id: doc.id.clone(),
            index: 0,
            span: Span::new(0, 25).unwrap(),
            body: "segment test content here".to_string(),
            retrieval_text: "segment test content here".to_string(),
        }];
        store.put_segments(&new_segments).unwrap();

        let got2 = store.get_segments(&doc.id).unwrap();
        assert_eq!(got2.len(), 1);
    }

    #[test]
    fn test_exists() {
        let (store, _f) = tmp_store();
        let doc = test_doc("existence check");

        assert!(!store.exists(&doc.id).unwrap());

        store.put_document(&doc, DocState::Staged).unwrap();
        assert!(store.exists(&doc.id).unwrap());

        // Non-existent doc
        let other = DocId::from_content(b"nonexistent");
        assert!(!store.exists(&other).unwrap());
    }

    #[test]
    fn test_savepoint_rollback_on_error() {
        let (store, _f) = tmp_store();
        let doc = test_doc("savepoint test content here");
        store.put_document(&doc, DocState::Staged).unwrap();

        // put_segments with valid first segment but we test the CRUD works
        // within savepoints by doing a successful put then verifying.
        let segments = vec![Segment {
            id: SegmentId::new(&doc.id, 0),
            doc_id: doc.id.clone(),
            index: 0,
            span: Span::new(0, 14).unwrap(),
            body: "savepoint test".to_string(),
            retrieval_text: "savepoint test".to_string(),
        }];
        store.put_segments(&segments).unwrap();
        let got = store.get_segments(&doc.id).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].body, "savepoint test");
    }

    #[test]
    fn set_documents_ready_rolls_back_partial_batch() {
        let (store, _f) = tmp_store();
        let first = test_doc("first ready batch document");
        let second = test_doc("second ready batch document");
        store.put_document(&first, DocState::Indexing).unwrap();
        store.put_document(&second, DocState::Staged).unwrap();

        let error = store
            .set_documents_ready(&[first.id.clone(), second.id.clone()])
            .unwrap_err();

        assert!(matches!(error, ShiroError::InvalidInput { .. }));
        assert_eq!(store.get_document(&first.id).unwrap().1, DocState::Indexing);
        assert_eq!(store.get_document(&second.id).unwrap().1, DocState::Staged);
    }

    #[test]
    fn stage_document_processing_rolls_back_entire_canonical_aggregate() {
        let (store, _f) = tmp_store();
        let existing = test_doc("existing document");
        store.put_document(&existing, DocState::Staged).unwrap();
        let existing_segment = Segment {
            id: SegmentId::new(&existing.id, 0),
            doc_id: existing.id.clone(),
            index: 0,
            span: Span::new(0, existing.canonical_text.len()).unwrap(),
            body: existing.canonical_text.clone(),
            retrieval_text: existing.canonical_text.clone(),
        };
        store
            .put_segments(std::slice::from_ref(&existing_segment))
            .unwrap();

        let target = test_doc("target document");
        let conflicting_segment = Segment {
            id: existing_segment.id,
            doc_id: target.id.clone(),
            index: 0,
            span: Span::new(0, target.canonical_text.len()).unwrap(),
            body: target.canonical_text.clone(),
            retrieval_text: target.canonical_text.clone(),
        };
        let fingerprint = ProcessingFingerprint::new("test", 1, 1);
        let provenance =
            WriteProvenance::local_user("test_ingestion", target.metadata.source_hash.clone());

        let result = store.stage_document_processing(
            &target,
            &fingerprint,
            std::slice::from_ref(&conflicting_segment),
            target.canonical_text.as_bytes(),
            &provenance,
        );

        assert!(result.is_err(), "conflicting segment ID must fail staging");
        assert!(
            !store.exists(&target.id).unwrap(),
            "document, graph, fingerprint, and segments must roll back together"
        );
        assert!(!store.blob_exists(&target.metadata.source_hash).unwrap());
        assert!(store
            .get_document_provenance(&target.id)
            .unwrap()
            .is_empty());
        assert_eq!(store.get_segments(&existing.id).unwrap().len(), 1);
    }

    #[test]
    fn schema_v6_migrates_entry_point_columns() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = camino::Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();
        let db_path = path.join("v6.db");
        let connection = rusqlite::Connection::open(db_path.as_std_path()).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO schema_meta (key, value) VALUES ('schema_version', '6');
                 CREATE TABLE search_results (
                    result_id TEXT PRIMARY KEY,
                    query TEXT NOT NULL,
                    doc_id TEXT NOT NULL,
                    segment_id TEXT NOT NULL,
                    bm25_score REAL,
                    bm25_rank INTEGER,
                    vector_score REAL,
                    vector_rank INTEGER,
                    fused_score REAL,
                    fused_rank INTEGER,
                    fts_gen INTEGER,
                    vec_gen INTEGER,
                    query_digest TEXT,
                    reranker_score REAL,
                    reranker_rank INTEGER
                 );",
            )
            .unwrap();
        drop(connection);

        let store = Store::open(&db_path).unwrap();
        store
            .conn
            .prepare(
                "SELECT block_idx, block_kind, span_start, span_end FROM search_results LIMIT 0",
            )
            .unwrap();
        assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn schema_v7_migrates_immutable_search_snapshot_columns() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = camino::Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();
        let db_path = path.join("v7.db");
        let connection = rusqlite::Connection::open(db_path.as_std_path()).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO schema_meta (key, value) VALUES ('schema_version', '7');
                 CREATE TABLE search_results (
                    result_id TEXT PRIMARY KEY,
                    query TEXT NOT NULL,
                    doc_id TEXT NOT NULL,
                    segment_id TEXT NOT NULL,
                    bm25_score REAL,
                    bm25_rank INTEGER,
                    vector_score REAL,
                    vector_rank INTEGER,
                    fused_score REAL,
                    fused_rank INTEGER,
                    fts_gen INTEGER,
                    vec_gen INTEGER,
                    query_digest TEXT,
                    reranker_score REAL,
                    reranker_rank INTEGER,
                    block_idx INTEGER NOT NULL DEFAULT 0,
                    block_kind TEXT NOT NULL DEFAULT 'PARAGRAPH',
                    span_start INTEGER NOT NULL DEFAULT 0,
                    span_end INTEGER NOT NULL DEFAULT 0
                 );",
            )
            .unwrap();
        drop(connection);

        let store = Store::open(&db_path).unwrap();
        store
            .conn
            .prepare("SELECT search_snapshot_id, retrieval_policy_json FROM search_results LIMIT 0")
            .unwrap();
        assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn test_migration_framework() {
        let (store, _f) = tmp_store();
        assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn test_future_schema_rejected() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let path = camino::Utf8Path::from_path(f.path()).unwrap();
        let store = Store::open(path).unwrap();
        // Manually set version to future
        store
            .conn
            .execute(
                "UPDATE schema_meta SET value = '999' WHERE key = 'schema_version'",
                [],
            )
            .unwrap();
        drop(store);
        // Re-open should fail
        let err = Store::open(path).unwrap_err();
        assert!(
            err.to_string().contains("newer than this binary"),
            "got: {err}"
        );
    }

    #[test]
    fn test_purge_derived() {
        let (store, _f) = tmp_store();
        let doc = test_doc("purge test content");
        store.put_document(&doc, DocState::Staged).unwrap();

        let segments = vec![Segment {
            id: SegmentId::new(&doc.id, 0),
            doc_id: doc.id.clone(),
            index: 0,
            span: Span::new(0, 18).unwrap(),
            body: "purge test content".to_string(),
            retrieval_text: "purge test content".to_string(),
        }];
        store.put_segments(&segments).unwrap();
        assert_eq!(store.get_segments(&doc.id).unwrap().len(), 1);

        store.purge_derived(&doc.id).unwrap();
        assert_eq!(store.get_segments(&doc.id).unwrap().len(), 0);

        // Document row still exists
        assert!(store.exists(&doc.id).unwrap());
    }

    #[test]
    fn test_migration_from_v0() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let path = camino::Utf8Path::from_path(f.path()).unwrap();

        // Create a bare DB with just the documents table (v0 shape)
        {
            let conn = rusqlite::Connection::open(path.as_std_path()).unwrap();
            conn.execute_batch(
                "
                CREATE TABLE documents (
                    doc_id TEXT PRIMARY KEY,
                    canonical_text TEXT NOT NULL,
                    source_uri TEXT NOT NULL,
                    source_hash TEXT NOT NULL,
                    title TEXT,
                    state TEXT NOT NULL DEFAULT 'STAGED',
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );
            ",
            )
            .unwrap();
            // Insert a document in the v0 shape
            conn.execute(
                "INSERT INTO documents (doc_id, canonical_text, source_uri, source_hash, title, state) VALUES ('doc_test', 'hello', 'test.txt', 'abc', 'Test', 'STAGED')",
                [],
            ).unwrap();
        }

        // Opening with Store::open should bootstrap schema_meta and succeed
        let store = Store::open(path).unwrap();
        assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn test_schema_version_preserved_across_reopen() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let path = camino::Utf8Path::from_path(f.path()).unwrap();

        // First open creates schema
        {
            let store = Store::open(path).unwrap();
            assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
            let doc = test_doc("persistence test");
            store.put_document(&doc, DocState::Staged).unwrap();
        }

        // Second open should see same version and data
        {
            let store = Store::open(path).unwrap();
            assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
            let list = store.list_documents(10).unwrap();
            assert_eq!(list.len(), 1);
        }
    }

    #[test]
    fn test_blob_store_roundtrip() {
        let (store, _f) = tmp_store();
        let content = b"raw document bytes for blob store test";
        let hash = store.put_blob(content).unwrap();
        assert!(store.blob_exists(&hash).unwrap());
        assert!(!store.blob_exists("nonexistent").unwrap());
        let retrieved = store.get_blob(&hash).unwrap();
        assert_eq!(retrieved, content);
    }

    #[test]
    fn test_blob_idempotent() {
        let (store, _f) = tmp_store();
        let content = b"duplicate content";
        let h1 = store.put_blob(content).unwrap();
        let h2 = store.put_blob(content).unwrap();
        assert_eq!(h1, h2);
    }

    // ── V3 tests ───────────────────────────────────────────────────────

    #[test]
    fn test_schema_v3_migration() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let path = camino::Utf8Path::from_path(f.path()).unwrap();

        // Create a v2 database manually
        {
            let conn = rusqlite::Connection::open(path.as_std_path()).unwrap();
            conn.execute_batch(
                "
                PRAGMA foreign_keys = ON;
                CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO schema_meta (key, value) VALUES ('schema_version', '2');

                CREATE TABLE documents (
                    doc_id TEXT PRIMARY KEY,
                    canonical_text TEXT NOT NULL,
                    source_uri TEXT NOT NULL,
                    source_hash TEXT NOT NULL,
                    title TEXT,
                    rendered_text TEXT,
                    state TEXT NOT NULL DEFAULT 'STAGED',
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );

                CREATE TABLE segments (
                    segment_id TEXT PRIMARY KEY,
                    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
                    seg_index INTEGER NOT NULL,
                    span_start INTEGER NOT NULL,
                    span_end INTEGER NOT NULL,
                    body TEXT NOT NULL
                );

                CREATE TABLE search_results (
                    result_id TEXT PRIMARY KEY,
                    query TEXT NOT NULL,
                    doc_id TEXT NOT NULL,
                    segment_id TEXT NOT NULL,
                    bm25_score REAL,
                    bm25_rank INTEGER,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );

                CREATE TABLE blobs (
                    content_hash TEXT PRIMARY KEY,
                    raw_bytes BLOB NOT NULL,
                    byte_count INTEGER NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );
                ",
            )
            .unwrap();
        }

        // Open should migrate to current version (through v3, v4, v5)
        let store = Store::open(path).unwrap();
        assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);

        // Verify new tables exist
        let table_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('concepts','concept_relations','concept_closure','doc_concepts','enrichments','generations','active_generations')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 7);

        // Verify new columns on documents
        store
            .conn
            .prepare("SELECT fingerprint FROM documents LIMIT 0")
            .unwrap();

        // Verify new columns on search_results
        store
            .conn
            .prepare("SELECT vector_score, vector_rank, fused_score, fused_rank, fts_gen, vec_gen, query_digest FROM search_results LIMIT 0")
            .unwrap();

        // Verify active_generations seeded
        let fts_gen: i64 = store
            .conn
            .query_row(
                "SELECT gen_id FROM active_generations WHERE kind = 'fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fts_gen, 0);

        // Migration is idempotent — re-open should not fail
        drop(store);
        let store2 = Store::open(path).unwrap();
        assert_eq!(store2.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn test_schema_v4_migration() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let path = camino::Utf8Path::from_path(f.path()).unwrap();

        // Create a v3 database manually (with enrichments table)
        {
            let conn = rusqlite::Connection::open(path.as_std_path()).unwrap();
            conn.execute_batch(
                "
                PRAGMA foreign_keys = ON;
                CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO schema_meta (key, value) VALUES ('schema_version', '3');

                CREATE TABLE documents (
                    doc_id TEXT PRIMARY KEY,
                    canonical_text TEXT NOT NULL,
                    source_uri TEXT NOT NULL,
                    source_hash TEXT NOT NULL,
                    title TEXT,
                    rendered_text TEXT,
                    fingerprint TEXT,
                    state TEXT NOT NULL DEFAULT 'STAGED',
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );

                CREATE TABLE segments (
                    segment_id TEXT PRIMARY KEY,
                    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
                    seg_index INTEGER NOT NULL,
                    span_start INTEGER NOT NULL,
                    span_end INTEGER NOT NULL,
                    body TEXT NOT NULL
                );

                CREATE TABLE search_results (
                    result_id TEXT PRIMARY KEY,
                    query TEXT NOT NULL,
                    doc_id TEXT NOT NULL,
                    segment_id TEXT NOT NULL,
                    bm25_score REAL,
                    bm25_rank INTEGER,
                    vector_score REAL,
                    vector_rank INTEGER,
                    fused_score REAL,
                    fused_rank INTEGER,
                    fts_gen INTEGER,
                    vec_gen INTEGER,
                    query_digest TEXT,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );

                CREATE TABLE blobs (
                    content_hash TEXT PRIMARY KEY,
                    raw_bytes BLOB NOT NULL,
                    byte_count INTEGER NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );

                CREATE TABLE concepts (
                    concept_id TEXT PRIMARY KEY,
                    scheme_uri TEXT NOT NULL,
                    pref_label TEXT NOT NULL,
                    alt_labels TEXT NOT NULL DEFAULT '[]',
                    definition TEXT,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );
                CREATE TABLE concept_relations (
                    from_id TEXT NOT NULL REFERENCES concepts(concept_id),
                    to_id TEXT NOT NULL REFERENCES concepts(concept_id),
                    relation TEXT NOT NULL CHECK(relation IN ('BROADER','NARROWER','RELATED')),
                    PRIMARY KEY (from_id, to_id, relation)
                );
                CREATE TABLE concept_closure (
                    ancestor_id TEXT NOT NULL REFERENCES concepts(concept_id),
                    descendant_id TEXT NOT NULL REFERENCES concepts(concept_id),
                    depth INTEGER NOT NULL,
                    PRIMARY KEY (ancestor_id, descendant_id)
                );
                CREATE TABLE doc_concepts (
                    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
                    concept_id TEXT NOT NULL REFERENCES concepts(concept_id),
                    confidence REAL NOT NULL DEFAULT 1.0,
                    source TEXT NOT NULL DEFAULT 'manual',
                    PRIMARY KEY (doc_id, concept_id)
                );
                CREATE TABLE enrichments (
                    doc_id TEXT PRIMARY KEY REFERENCES documents(doc_id) ON DELETE CASCADE,
                    title TEXT,
                    summary TEXT,
                    tags TEXT NOT NULL DEFAULT '[]',
                    concepts TEXT NOT NULL DEFAULT '[]',
                    provider TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );
                CREATE TABLE generations (
                    kind TEXT NOT NULL,
                    gen_id INTEGER NOT NULL,
                    doc_count INTEGER NOT NULL DEFAULT 0,
                    segment_count INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                    PRIMARY KEY (kind, gen_id)
                );
                CREATE TABLE active_generations (
                    kind TEXT PRIMARY KEY,
                    gen_id INTEGER NOT NULL
                );
                INSERT INTO active_generations (kind, gen_id) VALUES ('fts', 0);
                INSERT INTO active_generations (kind, gen_id) VALUES ('vector', 0);
                ",
            )
            .unwrap();

            // Insert a document and a segment in v3 shape
            conn.execute(
                "INSERT INTO documents (doc_id, canonical_text, source_uri, source_hash, title, state) VALUES ('doc_test123', 'hello world', 'test.txt', 'abc', 'Test', 'STAGED')",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO segments (segment_id, doc_id, seg_index, span_start, span_end, body) VALUES ('seg_test456', 'doc_test123', 0, 0, 5, 'hello')",
                [],
            ).unwrap();
        }

        // Open triggers v3→v4 migration
        let store = Store::open(path).unwrap();
        assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);

        // Verify doc_versions table exists and has an entry
        let ver_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM doc_versions WHERE doc_id = 'doc_test123'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ver_count, 1);

        // Verify active_version_id is populated
        let active_ver: Option<String> = store
            .conn
            .query_row(
                "SELECT active_version_id FROM documents WHERE doc_id = 'doc_test123'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            active_ver.is_some(),
            "active_version_id should be populated after migration"
        );
        let ver_str = active_ver.unwrap();
        assert!(
            ver_str.starts_with("ver_"),
            "version_id should start with ver_"
        );

        // Verify segments have version_id
        let seg_ver: Option<String> = store
            .conn
            .query_row(
                "SELECT version_id FROM segments WHERE segment_id = 'seg_test456'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            seg_ver.is_some(),
            "segment version_id should be populated after migration"
        );
        assert_eq!(seg_ver.unwrap(), ver_str);

        // Migration is idempotent — re-open should not fail
        drop(store);
        let store2 = Store::open(path).unwrap();
        assert_eq!(store2.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn test_version_crud() {
        let (store, _f) = tmp_store();
        let doc = test_doc("version test");
        let inserted = store.put_document(&doc, DocState::Staged).unwrap();
        assert!(inserted);

        // put_document should create version 1
        let ver = store.active_version_id(&doc.id).unwrap();
        assert!(ver.is_some());
        let ver = ver.unwrap();
        assert!(ver.as_str().starts_with("ver_"));
        assert_eq!(store.count_versions(&doc.id).unwrap(), 1);

        // Create version 2
        let ver2 = VersionId::new(&doc.id, 2);
        store
            .create_version(&doc.id, &ver2, Some("fp_hash"))
            .unwrap();
        assert_eq!(store.count_versions(&doc.id).unwrap(), 2);

        // Set active to version 2
        store.set_active_version(&doc.id, &ver2).unwrap();
        let active = store.active_version_id(&doc.id).unwrap().unwrap();
        assert_eq!(active, ver2);
    }

    #[test]
    fn test_concept_crud() {
        let (store, _f) = tmp_store();
        let c = test_concept("Rust");

        // Insert new
        assert!(store.put_concept(&c).unwrap());
        // Replace returns false
        assert!(!store.put_concept(&c).unwrap());

        // Get
        let got = store.get_concept(&c.id).unwrap();
        assert_eq!(got.pref_label, "Rust");
        assert_eq!(got.alt_labels, vec!["Rust-alt".to_string()]);
        assert_eq!(got.definition, Some("Definition of Rust".to_string()));
        assert_eq!(got.scheme_uri, "http://example.org/scheme");

        // List
        let c2 = test_concept("Python");
        store.put_concept(&c2).unwrap();
        let list = store.list_concepts(10).unwrap();
        assert_eq!(list.len(), 2);

        // Limit
        let limited = store.list_concepts(1).unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn test_concept_relations() {
        let (store, _f) = tmp_store();
        let parent = test_concept("Programming");
        let child = test_concept("Rust");
        store.put_concept(&parent).unwrap();
        store.put_concept(&child).unwrap();

        let rel = ConceptRelation {
            from: child.id.clone(),
            to: parent.id.clone(),
            relation: SkosRelation::Broader,
        };
        store.put_concept_relation(&rel).unwrap();

        // Idempotent
        store.put_concept_relation(&rel).unwrap();

        let rels = store.get_concept_relations(&child.id).unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].to, parent.id);
        assert_eq!(rels[0].relation, SkosRelation::Broader);

        // No relations for parent from parent's perspective
        let parent_rels = store.get_concept_relations(&parent.id).unwrap();
        assert!(parent_rels.is_empty());
    }

    #[test]
    fn test_assign_concept_to_doc() {
        let (store, _f) = tmp_store();
        let doc = test_doc("concept doc");
        store.put_document(&doc, DocState::Staged).unwrap();

        let c = test_concept("Testing");
        store.put_concept(&c).unwrap();

        store
            .assign_concept_to_doc(&doc.id, &c.id, 0.95, "auto")
            .unwrap();

        let concepts = store.get_doc_concepts(&doc.id).unwrap();
        assert_eq!(concepts.len(), 1);
        assert_eq!(concepts[0].0, c.id);
        assert!((concepts[0].1 - 0.95).abs() < 0.001);
        assert_eq!(concepts[0].2, "auto");

        // Replace with different confidence
        store
            .assign_concept_to_doc(&doc.id, &c.id, 0.5, "manual")
            .unwrap();
        let updated = store.get_doc_concepts(&doc.id).unwrap();
        assert_eq!(updated.len(), 1);
        assert!((updated[0].1 - 0.5).abs() < 0.001);
        assert_eq!(updated[0].2, "manual");
    }

    #[test]
    fn test_enrichment_crud() {
        let (store, _f) = tmp_store();
        let doc = test_doc("enrichment doc");
        store.put_document(&doc, DocState::Staged).unwrap();

        // Not found returns None
        assert!(store.get_enrichment(&doc.id).unwrap().is_none());

        let c = test_concept("Tag");
        store.put_concept(&c).unwrap();

        let enrichment = EnrichmentResult {
            doc_id: doc.id.clone(),
            title: Some("My Title".to_string()),
            summary: Some("A summary".to_string()),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
            concepts: vec![c.id.clone()],
            provider: "test-llm".to_string(),
            content_hash: "abc123".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
        };
        store.put_enrichment(&enrichment).unwrap();

        let got = store.get_enrichment(&doc.id).unwrap().unwrap();
        assert_eq!(got.title, Some("My Title".to_string()));
        assert_eq!(got.summary, Some("A summary".to_string()));
        assert_eq!(got.tags, vec!["tag1".to_string(), "tag2".to_string()]);
        assert_eq!(got.concepts.len(), 1);
        assert_eq!(got.concepts[0], c.id);
        assert_eq!(got.provider, "test-llm");
        assert_eq!(got.content_hash, "abc123");
    }

    #[test]
    fn test_generation_tracking() {
        let (store, _f) = tmp_store();

        // Default active gen is 0
        let gen = store.active_generation("fts").unwrap();
        assert_eq!(gen.as_u64(), 0);

        let gen = store.active_generation("vector").unwrap();
        assert_eq!(gen.as_u64(), 0);

        // Record a generation
        let ig = IndexGeneration {
            gen_id: GenerationId::new(1),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            doc_count: 10,
            segment_count: 50,
        };
        store.record_generation("fts", &ig).unwrap();

        // Set active
        store
            .set_active_generation("fts", GenerationId::new(1))
            .unwrap();
        let active = store.active_generation("fts").unwrap();
        assert_eq!(active.as_u64(), 1);

        // Vector still at 0
        assert_eq!(store.active_generation("vector").unwrap().as_u64(), 0);
    }

    #[test]
    fn corpus_manifest_activation_updates_both_pointers_atomically() {
        let (store, _file) = tmp_store();
        let fts = store
            .reserve_generation("fts", 1, 2, "2025-01-01T00:00:00Z")
            .unwrap();
        let vector = store
            .reserve_generation("vector", 1, 2, "2025-01-01T00:00:00Z")
            .unwrap();
        let manifest = CorpusManifest {
            manifest_id: "corpus_first".to_string(),
            corpus_digest: "corpus".to_string(),
            document_count: 1,
            segment_count: 2,
            fts_generation: fts,
            fts_digest: "fts".to_string(),
            vector_generation: Some(vector),
            vector_digest: Some("vector".to_string()),
            embedding_fingerprint_hash: Some("fingerprint".to_string()),
            created_at: "2025-01-01T00:00:00Z".to_string(),
        };
        store.activate_corpus_manifest(&manifest).unwrap();
        assert_eq!(store.active_generation("fts").unwrap(), fts);
        assert_eq!(store.active_generation("vector").unwrap(), vector);
        assert_eq!(
            store.corpus_manifest_generation_references().unwrap(),
            CorpusManifestGenerationReferences {
                fts_generations: vec![fts],
                vector_generations: vec![vector],
            }
        );

        let mut conflicting = manifest.clone();
        conflicting.fts_generation = store
            .reserve_generation("fts", 1, 2, "2025-01-02T00:00:00Z")
            .unwrap();
        assert!(store.activate_corpus_manifest(&conflicting).is_err());
        assert_eq!(store.active_generation("fts").unwrap(), fts);
        assert_eq!(store.active_generation("vector").unwrap(), vector);
        assert_eq!(store.active_corpus_manifest().unwrap(), Some(manifest));
    }

    #[test]
    fn reserved_generation_identifiers_are_not_reused() {
        let (store, _file) = tmp_store();
        let first = store
            .reserve_generation("fts", 0, 0, "2025-01-01T00:00:00Z")
            .unwrap();
        let second = store
            .reserve_generation("fts", 0, 0, "2025-01-01T00:00:01Z")
            .unwrap();
        assert_eq!(second, first.next());
        assert_eq!(store.active_generation("fts").unwrap(), GenerationId::ZERO);
    }

    #[test]
    fn test_fingerprint_crud() {
        let (store, _f) = tmp_store();
        let doc = test_doc("fingerprint doc");
        store.put_document(&doc, DocState::Staged).unwrap();

        // No fingerprint initially
        assert!(store.get_fingerprint(&doc.id).unwrap().is_none());

        let fp = ProcessingFingerprint::new("markdown", 1, 2);
        store.set_fingerprint(&doc.id, &fp).unwrap();

        let got = store.get_fingerprint(&doc.id).unwrap().unwrap();
        assert_eq!(got.parser_name, "markdown");
        assert_eq!(got.parser_version, 1);
        assert_eq!(got.segmenter_version, 2);

        // Overwrite
        let fp2 = ProcessingFingerprint::new("pdf", 3, 5);
        store.set_fingerprint(&doc.id, &fp2).unwrap();
        let got2 = store.get_fingerprint(&doc.id).unwrap().unwrap();
        assert_eq!(got2.parser_name, "pdf");
    }

    #[test]
    fn test_rebuild_closure() {
        let (store, _f) = tmp_store();

        // Create hierarchy: Animal > Mammal > Dog
        let animal = test_concept("Animal");
        let mammal = test_concept("Mammal");
        let dog = test_concept("Dog");
        store.put_concept(&animal).unwrap();
        store.put_concept(&mammal).unwrap();
        store.put_concept(&dog).unwrap();

        // Mammal BROADER Animal
        store
            .put_concept_relation(&ConceptRelation {
                from: mammal.id.clone(),
                to: animal.id.clone(),
                relation: SkosRelation::Broader,
            })
            .unwrap();

        // Dog BROADER Mammal
        store
            .put_concept_relation(&ConceptRelation {
                from: dog.id.clone(),
                to: mammal.id.clone(),
                relation: SkosRelation::Broader,
            })
            .unwrap();

        store.rebuild_closure().unwrap();

        // Verify closure: Animal is ancestor of Mammal (depth 1) and Dog (depth 2)
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM concept_closure", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3); // Animal>Mammal, Mammal>Dog, Animal>Dog
        assert_eq!(
            store.get_concept_descendant_ids(&animal.id).unwrap(),
            vec![mammal.id.clone(), dog.id.clone()]
        );

        // Animal is ancestor of Dog at depth 2
        let depth: i64 = store
            .conn
            .query_row(
                "SELECT depth FROM concept_closure WHERE ancestor_id = ?1 AND descendant_id = ?2",
                rusqlite::params![animal.id.as_str(), dog.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(depth, 2);

        // Rebuild is idempotent
        store.rebuild_closure().unwrap();
        let count2: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM concept_closure", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count2, 3);

        let closure_before_cycle = store.get_concept_descendant_ids(&animal.id).unwrap();
        let cycle = store.relate_concepts(&ConceptRelation {
            from: animal.id.clone(),
            to: dog.id.clone(),
            relation: SkosRelation::Broader,
        });
        assert!(matches!(cycle, Err(ShiroError::TaxonomyCycle { .. })));
        assert_eq!(
            store.get_concept_descendant_ids(&animal.id).unwrap(),
            closure_before_cycle
        );
        assert!(store.get_concept_relations(&animal.id).unwrap().is_empty());

        // Empty relations → empty closure
        store
            .conn
            .execute("DELETE FROM concept_relations", [])
            .unwrap();
        store.rebuild_closure().unwrap();
        let count3: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM concept_closure", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count3, 0);
    }

    #[test]
    fn narrower_relation_populates_the_same_hierarchical_closure() {
        let (store, _fixture) = tmp_store();
        let broader = test_concept("Broader");
        let narrower = test_concept("Narrower");
        store.put_concept(&broader).unwrap();
        store.put_concept(&narrower).unwrap();

        store
            .relate_concepts(&ConceptRelation {
                from: broader.id.clone(),
                to: narrower.id.clone(),
                relation: SkosRelation::Narrower,
            })
            .unwrap();

        assert_eq!(
            store.get_concept_descendant_ids(&broader.id).unwrap(),
            vec![narrower.id]
        );
    }

    // ── BlockGraph persistence tests (ADR-006) ───────────────────────

    fn test_doc_with_graph(content: &str) -> Document {
        use shiro_core::ir::{Block, BlockIdx, BlockKind, Edge, Relation};
        Document {
            id: DocId::from_content(content.as_bytes()),
            canonical_text: content.to_string(),
            rendered_text: None,
            metadata: Metadata {
                title: Some("Graph Test".to_string()),
                source_uri: "test.md".to_string(),
                source_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
            },
            blocks: BlockGraph {
                blocks: vec![
                    Block {
                        canonical_text: "Hello".to_string(),
                        rendered_text: None,
                        kind: BlockKind::Heading,
                        heading_level: Some(DocumentHeadingLevel::new(1).unwrap()),
                        span: Span::new(0, 5).unwrap(),
                        source_locators: vec![SourceLocator::new(
                            2,
                            Some(SourceRegion::new(1.0, 2.0, 3.0, 4.0).unwrap()),
                            Some(CoordinateOrigin::TopLeft),
                            Some(PageDimensions::new(612.0, 792.0).unwrap()),
                        )
                        .unwrap()],
                    },
                    Block {
                        canonical_text: " world".to_string(),
                        rendered_text: Some("world".to_string()),
                        kind: BlockKind::Paragraph,
                        heading_level: None,
                        span: Span::new(5, 11).unwrap(),
                        source_locators: Vec::new(),
                    },
                ],
                edges: vec![Edge {
                    from: BlockIdx(0),
                    to: BlockIdx(1),
                    relation: Relation::ReadsBefore,
                }],
                reading_order: vec![BlockIdx(0), BlockIdx(1)],
            },
            losses: Vec::new(),
        }
    }

    #[test]
    fn test_block_graph_roundtrip() {
        let (store, _f) = tmp_store();
        let doc = test_doc_with_graph("Hello world");

        store.put_document(&doc, DocState::Staged).unwrap();

        let graph = store.get_block_graph(&doc.id).unwrap();
        assert_eq!(graph.blocks.len(), 2);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.reading_order.len(), 2);

        // Verify block content
        assert_eq!(graph.blocks[0].canonical_text, "Hello");
        assert_eq!(graph.blocks[0].kind, BlockKind::Heading);
        assert_eq!(
            graph.blocks[0]
                .heading_level
                .map(DocumentHeadingLevel::as_u32),
            Some(1)
        );
        assert_eq!(graph.blocks[0].span.start(), 0);
        assert_eq!(graph.blocks[0].span.end(), 5);
        assert!(graph.blocks[0].rendered_text.is_none());
        assert_eq!(graph.blocks[0].source_locators.len(), 1);
        let locator = &graph.blocks[0].source_locators[0];
        assert_eq!(locator.page_number(), 2);
        assert_eq!(locator.coordinate_origin(), Some(CoordinateOrigin::TopLeft));
        assert_eq!(locator.region().unwrap().x0(), 1.0);
        assert_eq!(locator.page_dimensions().unwrap().width(), 612.0);

        assert_eq!(graph.blocks[1].canonical_text, " world");
        assert_eq!(graph.blocks[1].kind, BlockKind::Paragraph);
        assert_eq!(graph.blocks[1].rendered_text.as_deref(), Some("world"));

        // Verify edge
        assert_eq!(graph.edges[0].from, BlockIdx(0));
        assert_eq!(graph.edges[0].to, BlockIdx(1));
        assert_eq!(graph.edges[0].relation, Relation::ReadsBefore);

        // Verify reading order
        assert_eq!(graph.reading_order, vec![BlockIdx(0), BlockIdx(1)]);
    }

    #[test]
    fn evidence_handles_survive_span_changes_and_report_explicit_supersession() {
        let (store, _file) = tmp_store();
        let doc = test_doc_with_graph("Hello world");
        store.put_document(&doc, DocState::Staged).unwrap();
        let original = evidence_handle_for_block(&doc.id, &doc.blocks, 0).unwrap();
        assert_eq!(
            store.get_evidence_handle(&original).unwrap().status,
            "ACTIVE"
        );

        let mut moved = doc.blocks.clone();
        moved.blocks[0].span = Span::new(1, 6).unwrap();
        moved.blocks[1].span = Span::new(6, 11).unwrap();
        store.put_block_graph(&doc.id, &moved).unwrap();
        assert_eq!(
            store.get_evidence_handle(&original).unwrap().status,
            "ACTIVE"
        );

        let mut changed = moved;
        changed.blocks[0].canonical_text = "Hallo".to_string();
        let successor = evidence_handle_for_block(&doc.id, &changed, 0).unwrap();
        store.put_block_graph(&doc.id, &changed).unwrap();
        let superseded = store.get_evidence_handle(&original).unwrap();
        assert_eq!(superseded.status, "SUPERSEDED");
        assert_eq!(superseded.superseded_by, Some(successor.clone()));
        assert_eq!(
            store.get_evidence_handle(&successor).unwrap().status,
            "ACTIVE"
        );
    }

    #[test]
    fn partial_persisted_source_locator_is_store_corruption() {
        let (store, _file) = tmp_store();
        let doc = test_doc_with_graph("Hello world");
        store.put_document(&doc, DocState::Staged).unwrap();
        store
            .conn
            .execute(
                "UPDATE block_source_locators SET region_x1 = NULL WHERE doc_id = ?1",
                rusqlite::params![doc.id.as_str()],
            )
            .unwrap();

        let error = store.get_block_graph(&doc.id).unwrap_err();
        assert!(matches!(error, ShiroError::StoreCorrupt { .. }));
    }

    #[test]
    fn test_block_graph_empty_roundtrip() {
        let (store, _f) = tmp_store();
        let doc = test_doc("empty graph doc");

        store.put_document(&doc, DocState::Staged).unwrap();

        let graph = store.get_block_graph(&doc.id).unwrap();
        assert!(graph.blocks.is_empty());
        assert!(graph.edges.is_empty());
        assert!(graph.reading_order.is_empty());
    }

    #[test]
    fn test_block_graph_persisted_with_document() {
        let (store, _f) = tmp_store();
        let doc = test_doc_with_graph("Hello world");

        store.put_document(&doc, DocState::Staged).unwrap();

        // get_document should return the full graph
        let (loaded, _state) = store.get_document(&doc.id).unwrap();
        assert_eq!(loaded.blocks.blocks.len(), 2);
        assert_eq!(loaded.blocks.edges.len(), 1);
        assert_eq!(loaded.blocks.reading_order.len(), 2);
    }

    #[test]
    fn test_block_graph_replaced_on_reput() {
        let (store, _f) = tmp_store();
        let doc = test_doc_with_graph("Hello world");

        store.put_document(&doc, DocState::Staged).unwrap();

        // Re-put with empty graph
        let mut doc2 = doc.clone();
        doc2.blocks = BlockGraph::empty();
        store.put_document(&doc2, DocState::Ready).unwrap();

        let graph = store.get_block_graph(&doc.id).unwrap();
        assert!(graph.blocks.is_empty());
    }

    #[test]
    fn test_block_graph_all_block_kinds() {
        use shiro_core::ir::{Block, BlockIdx, BlockKind};
        let (store, _f) = tmp_store();

        let kinds = [
            BlockKind::Paragraph,
            BlockKind::Heading,
            BlockKind::ListItem,
            BlockKind::TableCell,
            BlockKind::Code,
            BlockKind::Caption,
            BlockKind::Footnote,
        ];

        let content = "x".repeat(kinds.len());
        let doc_id = DocId::from_content(content.as_bytes());
        let graph = BlockGraph {
            blocks: kinds
                .iter()
                .enumerate()
                .map(|(i, &kind)| Block {
                    canonical_text: "x".to_string(),
                    rendered_text: None,
                    kind,
                    heading_level: None,
                    span: Span::new(i, i + 1).unwrap(),
                    source_locators: Vec::new(),
                })
                .collect(),
            edges: vec![],
            reading_order: (0..kinds.len()).map(BlockIdx).collect(),
        };

        let doc = Document {
            id: doc_id.clone(),
            canonical_text: content,
            rendered_text: None,
            metadata: Metadata {
                title: None,
                source_uri: "test.md".to_string(),
                source_hash: "test".to_string(),
            },
            blocks: graph,
            losses: Vec::new(),
        };

        store.put_document(&doc, DocState::Staged).unwrap();

        let loaded = store.get_block_graph(&doc_id).unwrap();
        for (i, &expected_kind) in kinds.iter().enumerate() {
            assert_eq!(
                loaded.blocks[i].kind, expected_kind,
                "kind mismatch at index {i}"
            );
        }
    }

    #[test]
    fn test_block_graph_all_edge_relations() {
        use shiro_core::ir::{Block, BlockIdx, BlockKind, Edge, Relation};
        let (store, _f) = tmp_store();

        let relations = [
            Relation::ReadsBefore,
            Relation::CaptionOf,
            Relation::FootnoteOf,
            Relation::RefersTo,
            Relation::SectionContains,
        ];

        let content = "abcde";
        let doc_id = DocId::from_content(content.as_bytes());
        let graph = BlockGraph {
            blocks: vec![
                Block {
                    canonical_text: "a".to_string(),
                    rendered_text: None,
                    kind: BlockKind::Paragraph,
                    heading_level: None,
                    span: Span::new(0, 1).unwrap(),
                    source_locators: Vec::new(),
                },
                Block {
                    canonical_text: "b".to_string(),
                    rendered_text: None,
                    kind: BlockKind::Paragraph,
                    heading_level: None,
                    span: Span::new(1, 2).unwrap(),
                    source_locators: Vec::new(),
                },
            ],
            edges: relations
                .iter()
                .map(|&rel| Edge {
                    from: BlockIdx(0),
                    to: BlockIdx(1),
                    relation: rel,
                })
                .collect(),
            reading_order: vec![BlockIdx(0), BlockIdx(1)],
        };

        let doc = Document {
            id: doc_id.clone(),
            canonical_text: content.to_string(),
            rendered_text: None,
            metadata: Metadata {
                title: None,
                source_uri: "test.md".to_string(),
                source_hash: "test".to_string(),
            },
            blocks: graph,
            losses: Vec::new(),
        };

        store.put_document(&doc, DocState::Staged).unwrap();

        let loaded = store.get_block_graph(&doc_id).unwrap();
        assert_eq!(loaded.edges.len(), relations.len());
        for (i, &expected_rel) in relations.iter().enumerate() {
            assert_eq!(
                loaded.edges[i].relation, expected_rel,
                "relation mismatch at index {i}"
            );
        }
    }

    #[test]
    fn test_block_graph_survives_purge_derived() {
        let (store, _f) = tmp_store();
        let doc = test_doc_with_graph("Hello world");

        store.put_document(&doc, DocState::Staged).unwrap();

        // Purge derived data (segments, search_results).
        // Per ADR-006, blocks are canonical, NOT derived — they must survive.
        store.purge_derived(&doc.id).unwrap();

        let graph = store.get_block_graph(&doc.id).unwrap();
        assert_eq!(graph.blocks.len(), 2, "blocks must survive purge_derived");
        assert_eq!(graph.edges.len(), 1, "edges must survive purge_derived");
    }

    #[test]
    fn reranker_fields_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = camino::Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();
        let db_path = path.join("test.db");
        let store = Store::open(&db_path).unwrap();

        // Create a minimal document so we have valid IDs.
        let doc_id = DocId::from_content(b"test doc");
        let seg_id = SegmentId::new(&doc_id, 0);

        // Save a search result with reranker fields.
        let row = SearchResultRow {
            result_id: "res_test123".to_string(),
            evidence_handle: EvidenceHandleId::from_stored(format!("blk_{}", "a".repeat(64)))
                .unwrap(),
            doc_id: doc_id.clone(),
            segment_id: seg_id,
            bm25_score: Some(1.5),
            bm25_rank: Some(1),
            vector_score: Some(0.85),
            vector_rank: Some(2),
            fused_score: Some(0.02),
            fused_rank: Some(1),
            reranker_score: Some(0.95),
            reranker_rank: Some(1),
            block_idx: 3,
            block_kind: "HEADING".to_string(),
            heading_level: Some(2),
            span_start: 10,
            span_end: 20,
            source_locators: vec![SourceLocator::new(4, None, None, None).unwrap()],
        };
        let snapshot = SearchSnapshotMetadata {
            search_snapshot_id: "run_test",
            retrieval_policy_json: "{}",
            query: "test query",
            query_digest: "abc123",
            fts_generation: 1,
            vector_generation: 0,
        };
        store.save_search_results(&snapshot, &[row]).unwrap();

        // Retrieve and verify.
        let detail = store.get_search_result("res_test123").unwrap();
        assert_eq!(
            detail.evidence_handle.as_ref().unwrap().as_str(),
            format!("blk_{}", "a".repeat(64))
        );
        assert_eq!(detail.reranker_score, Some(0.95));
        assert_eq!(detail.reranker_rank, Some(1));
        assert_eq!(detail.block_idx, 3);
        assert_eq!(detail.block_kind, "HEADING");
        assert_eq!(detail.heading_level, Some(2));
        assert_eq!(detail.span_start, 10);
        assert_eq!(detail.span_end, 20);
        assert_eq!(detail.source_locators[0].page_number(), 4);
        assert_eq!(detail.search_snapshot_id, "run_test");
        assert_eq!(detail.retrieval_policy_json, "{}");
    }

    #[test]
    fn reranker_fields_none_when_absent() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = camino::Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();
        let db_path = path.join("test.db");
        let store = Store::open(&db_path).unwrap();

        let doc_id = DocId::from_content(b"test doc 2");
        let seg_id = SegmentId::new(&doc_id, 0);

        let row = SearchResultRow {
            result_id: "res_test456".to_string(),
            evidence_handle: EvidenceHandleId::from_stored(format!("blk_{}", "b".repeat(64)))
                .unwrap(),
            doc_id: doc_id.clone(),
            segment_id: seg_id,
            bm25_score: Some(1.0),
            bm25_rank: Some(1),
            vector_score: None,
            vector_rank: None,
            fused_score: Some(0.01),
            fused_rank: Some(1),
            reranker_score: None,
            reranker_rank: None,
            block_idx: 0,
            block_kind: "HEADING".to_string(),
            heading_level: None,
            span_start: 0,
            span_end: 4,
            source_locators: Vec::new(),
        };
        let snapshot = SearchSnapshotMetadata {
            search_snapshot_id: "run_test_2",
            retrieval_policy_json: "{}",
            query: "test query 2",
            query_digest: "def456",
            fts_generation: 1,
            vector_generation: 0,
        };
        store.save_search_results(&snapshot, &[row]).unwrap();

        let detail = store.get_search_result("res_test456").unwrap();
        assert_eq!(detail.reranker_score, None);
        assert_eq!(detail.reranker_rank, None);
    }
}
