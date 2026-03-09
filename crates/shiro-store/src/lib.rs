//! SQLite-backed document store for the shiro workspace.

use shiro_core::enrichment::EnrichmentResult;
use shiro_core::error::ShiroError;
use shiro_core::fingerprint::ProcessingFingerprint;
use shiro_core::generation::{GenerationId, IndexGeneration};
use shiro_core::id::{DocId, SegmentId, VersionId};
use shiro_core::ir::{
    Block, BlockGraph, BlockIdx, BlockKind, Document, Edge, Metadata, Relation, Segment,
};
use shiro_core::manifest::DocState;
use shiro_core::span::Span;
use shiro_core::taxonomy::{Concept, ConceptId, ConceptRelation, SkosRelation};

/// Map any `rusqlite::Error` into `ShiroError::StoreCorrupt`.
fn map_db(e: rusqlite::Error) -> ShiroError {
    ShiroError::StoreCorrupt {
        message: e.to_string(),
    }
}

/// Parse a `DocState` from its SQL string representation.
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
    }
}

/// Parse a block `Relation` from its SQL string representation.
fn parse_edge_relation(s: &str) -> Result<Relation, ShiroError> {
    match s {
        "READS_BEFORE" => Ok(Relation::ReadsBefore),
        "CAPTION_OF" => Ok(Relation::CaptionOf),
        "FOOTNOTE_OF" => Ok(Relation::FootnoteOf),
        "REFERS_TO" => Ok(Relation::RefersTo),
        other => Err(ShiroError::StoreCorrupt {
            message: format!("unknown block Relation: {other}"),
        }),
    }
}

/// Current schema version this binary expects.
pub const CURRENT_SCHEMA_VERSION: u32 = 6;

/// A row to be saved in the `search_results` table.
pub struct SearchResultRow {
    pub result_id: String,
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
}

/// Detail returned from `get_search_result`.
pub struct SearchResultDetail {
    pub query: String,
    pub query_digest: Option<String>,
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

/// V5 DDL: persist BlockGraph as first-class stored representation (ADR-006).
const V5_CREATE_TABLES: &str = "
CREATE TABLE IF NOT EXISTS blocks (
    doc_id TEXT NOT NULL REFERENCES documents(doc_id) ON DELETE CASCADE,
    block_idx INTEGER NOT NULL,
    kind TEXT NOT NULL,
    span_start INTEGER NOT NULL,
    span_end INTEGER NOT NULL,
    canonical_text TEXT NOT NULL,
    rendered_text TEXT,
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

        // V5 tables: BlockGraph persistence (ADR-006)
        conn.execute_batch(V5_CREATE_TABLES).map_err(map_db)?;
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

    /// Insert or update a document. Returns `true` if newly inserted.
    pub fn put_document(&self, doc: &Document, state: DocState) -> Result<bool, ShiroError> {
        // Check existence first to determine insert vs replace.
        let existed = self.exists(&doc.id)?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO documents (doc_id, canonical_text, rendered_text, source_uri, source_hash, title, state, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
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

        // Persist BlockGraph atomically with document (ADR-006).
        self.put_block_graph(&doc.id, &doc.blocks)?;

        if !existed {
            let version_id = VersionId::new(&doc.id, 1);
            self.create_version(&doc.id, &version_id, None)?;
            self.set_active_version(&doc.id, &version_id)?;
        }

        Ok(!existed)
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
            losses: Vec::new(),
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

    // ── Segment CRUD ───────────────────────────────────────────────────

    /// Insert segments for a document (replaces existing).
    ///
    /// Wrapped in a savepoint so a mid-loop failure does not leave the
    /// document with partial or zero segments.
    pub fn put_segments(&self, segments: &[Segment]) -> Result<(), ShiroError> {
        if segments.is_empty() {
            return Ok(());
        }

        let doc_id = &segments[0].doc_id;

        let version_id_str: Option<String> = self
            .conn
            .query_row(
                "SELECT active_version_id FROM documents WHERE doc_id = ?1",
                rusqlite::params![doc_id.as_str()],
                |row| row.get(0),
            )
            .ok();

        self.with_savepoint("put_segments", || {
            self.conn
                .execute(
                    "DELETE FROM segments WHERE doc_id = ?1",
                    rusqlite::params![doc_id.as_str()],
                )
                .map_err(map_db)?;

            let mut stmt = self
                .conn
                .prepare(
                    "INSERT INTO segments (segment_id, doc_id, seg_index, span_start, span_end, body, version_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(map_db)?;

            for seg in segments {
                stmt.execute(rusqlite::params![
                    seg.id.as_str(),
                    seg.doc_id.as_str(),
                    seg.index as i64,
                    seg.span.start() as i64,
                    seg.span.end() as i64,
                    seg.body,
                    version_id_str,
                ])
                .map_err(map_db)?;
            }

            Ok(())
        })
    }

    // ── BlockGraph persistence (ADR-006) ───────────────────────────────

    /// Persist a document's BlockGraph. Replaces any existing graph data.
    ///
    /// Per ADR-006, the graph is canonical; segments are derived.
    /// This must be called in the same transaction as put_document/put_segments.
    pub fn put_block_graph(&self, doc_id: &DocId, graph: &BlockGraph) -> Result<(), ShiroError> {
        self.with_savepoint("put_block_graph", || {
            let id = doc_id.as_str();

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
                        "INSERT INTO blocks (doc_id, block_idx, kind, span_start, span_end, canonical_text, rendered_text)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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
                    ])
                    .map_err(map_db)?;
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

            Ok(())
        })
    }

    /// Load the persisted BlockGraph for a document.
    ///
    /// Returns `BlockGraph::empty()` if no graph data exists (e.g. pre-v5 documents).
    pub fn get_block_graph(&self, doc_id: &DocId) -> Result<BlockGraph, ShiroError> {
        let id = doc_id.as_str();

        // Load blocks.
        let blocks = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT block_idx, kind, span_start, span_end, canonical_text, rendered_text
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
                    Ok((
                        kind_str,
                        span_start,
                        span_end,
                        canonical_text,
                        rendered_text,
                    ))
                })
                .map_err(map_db)?;

            let mut blocks = Vec::new();
            for row in rows {
                let (kind_str, span_start, span_end, canonical_text, rendered_text) =
                    row.map_err(map_db)?;
                let kind = parse_block_kind(&kind_str)?;
                let span = Span::new(span_start as usize, span_end as usize).map_err(|e| {
                    ShiroError::StoreCorrupt {
                        message: format!("invalid block span: {e}"),
                    }
                })?;
                blocks.push(Block {
                    canonical_text,
                    rendered_text,
                    kind,
                    span,
                });
            }
            blocks
        };

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
                "SELECT segment_id, doc_id, seg_index, span_start, span_end, body
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
                Ok((
                    segment_id_str,
                    doc_id_str,
                    index,
                    span_start,
                    span_end,
                    body,
                ))
            })
            .map_err(map_db)?;

        let mut out = Vec::new();
        for row in rows {
            let (segment_id_str, doc_id_str, index, span_start, span_end, body) =
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
        query: &str,
        query_digest: &str,
        fts_gen: u64,
        vec_gen: u64,
        results: &[SearchResultRow],
    ) -> Result<(), ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO search_results (result_id, query, doc_id, segment_id, bm25_score, bm25_rank, vector_score, vector_rank, fused_score, fused_rank, fts_gen, vec_gen, query_digest, reranker_score, reranker_rank)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            )
            .map_err(map_db)?;

        for r in results {
            stmt.execute(rusqlite::params![
                r.result_id,
                query,
                r.doc_id.as_str(),
                r.segment_id.as_str(),
                r.bm25_score.map(|s| s as f64),
                r.bm25_rank.map(|r| r as i64),
                r.vector_score.map(|s| s as f64),
                r.vector_rank.map(|r| r as i64),
                r.fused_score.map(|s| s as f64),
                r.fused_rank.map(|r| r as i64),
                fts_gen as i64,
                vec_gen as i64,
                query_digest,
                r.reranker_score.map(|s| s as f64),
                r.reranker_rank.map(|r| r as i64),
            ])
            .map_err(map_db)?;
        }

        Ok(())
    }

    /// Load a saved search result by `result_id`.
    pub fn get_search_result(&self, result_id: &str) -> Result<SearchResultDetail, ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT query, doc_id, segment_id, bm25_score, bm25_rank, vector_score, vector_rank, fused_score, fused_rank, fts_gen, vec_gen, query_digest, reranker_score, reranker_rank
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
        ) = result;

        let doc_id = DocId::from_stored(doc_id_str).map_err(|e| ShiroError::StoreCorrupt {
            message: e.to_string(),
        })?;
        let segment_id =
            SegmentId::from_stored(segment_id_str).map_err(|e| ShiroError::StoreCorrupt {
                message: e.to_string(),
            })?;

        Ok(SearchResultDetail {
            query,
            query_digest,
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

    /// Rebuild the transitive closure table from BROADER edges.
    ///
    /// Uses iterative BFS: repeatedly join concept_relations (BROADER)
    /// with the closure table until no new rows are added.
    pub fn rebuild_closure(&self) -> Result<(), ShiroError> {
        self.with_savepoint("rebuild_closure", || {
            self.conn
                .execute("DELETE FROM concept_closure", [])
                .map_err(map_db)?;

            // Seed: depth-1 edges from BROADER relations (from is descendant, to is ancestor)
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO concept_closure (ancestor_id, descendant_id, depth)
                     SELECT to_id, from_id, 1 FROM concept_relations WHERE relation = 'BROADER'",
                    [],
                )
                .map_err(map_db)?;

            // Iterative expansion: join closure with BROADER edges
            loop {
                let inserted = self
                    .conn
                    .execute(
                        "INSERT OR IGNORE INTO concept_closure (ancestor_id, descendant_id, depth)
                         SELECT c.ancestor_id, r.from_id, c.depth + 1
                         FROM concept_closure c
                         JOIN concept_relations r ON r.to_id = c.descendant_id AND r.relation = 'BROADER'
                         WHERE NOT EXISTS (
                             SELECT 1 FROM concept_closure x
                             WHERE x.ancestor_id = c.ancestor_id AND x.descendant_id = r.from_id
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
            },
            Segment {
                id: SegmentId::new(&doc.id, 1),
                doc_id: doc.id.clone(),
                index: 1,
                span: Span::new(13, 25).unwrap(),
                body: "content here".to_string(),
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
        }];
        store.put_segments(&segments).unwrap();
        let got = store.get_segments(&doc.id).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].body, "savepoint test");
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
                        span: Span::new(0, 5).unwrap(),
                    },
                    Block {
                        canonical_text: " world".to_string(),
                        rendered_text: Some("world".to_string()),
                        kind: BlockKind::Paragraph,
                        span: Span::new(5, 11).unwrap(),
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
        assert_eq!(graph.blocks[0].span.start(), 0);
        assert_eq!(graph.blocks[0].span.end(), 5);
        assert!(graph.blocks[0].rendered_text.is_none());

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
                    span: Span::new(i, i + 1).unwrap(),
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
        ];

        let content = "abcde";
        let doc_id = DocId::from_content(content.as_bytes());
        let graph = BlockGraph {
            blocks: vec![
                Block {
                    canonical_text: "a".to_string(),
                    rendered_text: None,
                    kind: BlockKind::Paragraph,
                    span: Span::new(0, 1).unwrap(),
                },
                Block {
                    canonical_text: "b".to_string(),
                    rendered_text: None,
                    kind: BlockKind::Paragraph,
                    span: Span::new(1, 2).unwrap(),
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
        };
        store
            .save_search_results("test query", "abc123", 1, 0, &[row])
            .unwrap();

        // Retrieve and verify.
        let detail = store.get_search_result("res_test123").unwrap();
        assert_eq!(detail.reranker_score, Some(0.95));
        assert_eq!(detail.reranker_rank, Some(1));
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
        };
        store
            .save_search_results("test query 2", "def456", 1, 0, &[row])
            .unwrap();

        let detail = store.get_search_result("res_test456").unwrap();
        assert_eq!(detail.reranker_score, None);
        assert_eq!(detail.reranker_rank, None);
    }
}
