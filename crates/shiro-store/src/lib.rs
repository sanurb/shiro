//! SQLite-backed document store for the shiro workspace.

use shiro_core::error::ShiroError;
use shiro_core::id::{DocId, SegmentId};
use shiro_core::ir::{Document, Metadata, Segment};
use shiro_core::manifest::DocState;
use shiro_core::span::Span;

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

/// SQLite-backed document and segment store.
#[derive(Debug)]
pub struct Store {
    conn: rusqlite::Connection,
}

impl Store {
    /// Open (or create) the database at the given path.
    pub fn open(db_path: &camino::Utf8Path) -> Result<Self, ShiroError> {
        let conn = rusqlite::Connection::open(db_path.as_std_path()).map_err(map_db)?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS documents (
                doc_id TEXT PRIMARY KEY,
                canonical_text TEXT NOT NULL,
                source_uri TEXT NOT NULL,
                source_hash TEXT NOT NULL,
                title TEXT,
                state TEXT NOT NULL DEFAULT 'STAGED',
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS segments (
                segment_id TEXT PRIMARY KEY,
                doc_id TEXT NOT NULL REFERENCES documents(doc_id),
                seg_index INTEGER NOT NULL,
                span_start INTEGER NOT NULL,
                span_end INTEGER NOT NULL,
                body TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_segments_doc ON segments(doc_id);

            CREATE TABLE IF NOT EXISTS search_results (
                result_id TEXT PRIMARY KEY,
                query TEXT NOT NULL,
                doc_id TEXT NOT NULL,
                segment_id TEXT NOT NULL,
                bm25_score REAL,
                bm25_rank INTEGER,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );
            ",
        )
        .map_err(map_db)?;

        tracing::debug!(path = %db_path, "opened store");
        Ok(Self { conn })
    }

    /// Insert or update a document. Returns `true` if newly inserted.
    pub fn put_document(&self, doc: &Document, state: DocState) -> Result<bool, ShiroError> {
        // Check existence first to determine insert vs replace.
        let existed = self.exists(&doc.id)?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO documents (doc_id, canonical_text, source_uri, source_hash, title, state, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                rusqlite::params![
                    doc.id.as_str(),
                    doc.canonical_text,
                    doc.metadata.source_uri,
                    doc.metadata.source_hash,
                    doc.metadata.title,
                    state.as_str(),
                ],
            )
            .map_err(map_db)?;

        Ok(!existed)
    }

    /// Get a document by ID.
    pub fn get_document(&self, id: &DocId) -> Result<(Document, DocState), ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT doc_id, canonical_text, source_uri, source_hash, title, state
                 FROM documents WHERE doc_id = ?1",
            )
            .map_err(map_db)?;

        let result = stmt
            .query_row(rusqlite::params![id.as_str()], |row| {
                let doc_id_str: String = row.get(0)?;
                let canonical_text: String = row.get(1)?;
                let source_uri: String = row.get(2)?;
                let source_hash: String = row.get(3)?;
                let title: Option<String> = row.get(4)?;
                let state_str: String = row.get(5)?;
                Ok((
                    doc_id_str,
                    canonical_text,
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

        let (doc_id_str, canonical_text, source_uri, source_hash, title, state_str) = result;

        let doc_id = DocId::from_stored(doc_id_str).map_err(|e| ShiroError::StoreCorrupt {
            message: e.to_string(),
        })?;
        let state = parse_state(&state_str)?;

        let doc = Document {
            id: doc_id,
            canonical_text,
            metadata: Metadata {
                title,
                source_uri,
                source_hash,
            },
            blocks: None,
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

    /// Transition a document's state.
    pub fn set_state(&self, id: &DocId, state: DocState) -> Result<(), ShiroError> {
        let changed = self
            .conn
            .execute(
                "UPDATE documents SET state = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE doc_id = ?2",
                rusqlite::params![state.as_str(), id.as_str()],
            )
            .map_err(map_db)?;

        if changed == 0 {
            return Err(ShiroError::NotFound(id.clone()));
        }
        Ok(())
    }

    /// Insert segments for a document (replaces existing).
    pub fn put_segments(&self, segments: &[Segment]) -> Result<(), ShiroError> {
        if segments.is_empty() {
            return Ok(());
        }

        let doc_id = &segments[0].doc_id;
        self.conn
            .execute(
                "DELETE FROM segments WHERE doc_id = ?1",
                rusqlite::params![doc_id.as_str()],
            )
            .map_err(map_db)?;

        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO segments (segment_id, doc_id, seg_index, span_start, span_end, body)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
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
            ])
            .map_err(map_db)?;
        }

        Ok(())
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

    /// Save search results for later explain.
    /// Each tuple: `(result_id, doc_id, segment_id, bm25_score, bm25_rank)`.
    pub fn save_search_results(
        &self,
        query: &str,
        results: &[(String, DocId, SegmentId, f32, usize)],
    ) -> Result<(), ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO search_results (result_id, query, doc_id, segment_id, bm25_score, bm25_rank)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(map_db)?;

        for (result_id, doc_id, segment_id, score, rank) in results {
            stmt.execute(rusqlite::params![
                result_id,
                query,
                doc_id.as_str(),
                segment_id.as_str(),
                *score as f64,
                *rank as i64,
            ])
            .map_err(map_db)?;
        }

        Ok(())
    }

    /// Load a saved search result by `result_id`.
    /// Returns `(query, doc_id, segment_id, bm25_score, bm25_rank)`.
    pub fn get_search_result(
        &self,
        result_id: &str,
    ) -> Result<(String, DocId, SegmentId, f32, usize), ShiroError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT query, doc_id, segment_id, bm25_score, bm25_rank
                 FROM search_results WHERE result_id = ?1",
            )
            .map_err(map_db)?;

        let result = stmt
            .query_row(rusqlite::params![result_id], |row| {
                let query: String = row.get(0)?;
                let doc_id_str: String = row.get(1)?;
                let segment_id_str: String = row.get(2)?;
                let score: f64 = row.get(3)?;
                let rank: i64 = row.get(4)?;
                Ok((query, doc_id_str, segment_id_str, score, rank))
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ShiroError::NotFoundMsg {
                    message: format!("search result not found: {result_id}"),
                },
                other => map_db(other),
            })?;

        let (query, doc_id_str, segment_id_str, score, rank) = result;
        let doc_id = DocId::from_stored(doc_id_str).map_err(|e| ShiroError::StoreCorrupt {
            message: e.to_string(),
        })?;
        let segment_id =
            SegmentId::from_stored(segment_id_str).map_err(|e| ShiroError::StoreCorrupt {
                message: e.to_string(),
            })?;

        Ok((query, doc_id, segment_id, score as f32, rank as usize))
    }

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
            metadata: Metadata {
                title: Some("Test".to_string()),
                source_uri: "test.txt".to_string(),
                source_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
            },
            blocks: None,
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

        store.set_state(&doc.id, DocState::Indexing).unwrap();
        let (_, s1) = store.get_document(&doc.id).unwrap();
        assert_eq!(s1, DocState::Indexing);

        store.set_state(&doc.id, DocState::Ready).unwrap();
        let (_, s2) = store.get_document(&doc.id).unwrap();
        assert_eq!(s2, DocState::Ready);
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
}
