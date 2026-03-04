//! Full-text search index backed by Tantivy (BM25 ranking).

use shiro_core::error::ShiroError;
use shiro_core::id::DocId;
use shiro_core::ir::Segment;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::document::TantivyDocument;
use tantivy::schema::{Schema, Value, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, Term};

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn map_tantivy(e: tantivy::TantivyError) -> ShiroError {
    ShiroError::IndexBuildFts {
        message: e.to_string(),
    }
}

fn map_tantivy_search(e: tantivy::TantivyError) -> ShiroError {
    ShiroError::SearchFailed {
        message: e.to_string(),
    }
}

fn map_query_parse(e: tantivy::query::QueryParserError) -> ShiroError {
    ShiroError::SearchFailed {
        message: e.to_string(),
    }
}

// ---------------------------------------------------------------------------
// FtsHit
// ---------------------------------------------------------------------------

/// A search hit from the FTS index with BM25 score.
#[derive(Debug, Clone)]
pub struct FtsHit {
    pub doc_id: String,
    pub segment_id: String,
    pub seg_index: usize,
    pub body: String,
    pub span_start: usize,
    pub span_end: usize,
    pub bm25_score: f32,
    pub bm25_rank: usize,
}

// ---------------------------------------------------------------------------
// FtsIndex
// ---------------------------------------------------------------------------

/// Tantivy-backed full-text search index with BM25 ranking.
pub struct FtsIndex {
    index: Index,
    reader: IndexReader,
    f_doc_id: tantivy::schema::Field,
    f_segment_id: tantivy::schema::Field,
    f_seg_index: tantivy::schema::Field,
    f_body: tantivy::schema::Field,
    f_span_start: tantivy::schema::Field,
    f_span_end: tantivy::schema::Field,
}

impl std::fmt::Debug for FtsIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FtsIndex")
            .field("f_doc_id", &self.f_doc_id)
            .field("f_segment_id", &self.f_segment_id)
            .field("f_seg_index", &self.f_seg_index)
            .field("f_body", &self.f_body)
            .field("f_span_start", &self.f_span_start)
            .field("f_span_end", &self.f_span_end)
            .finish_non_exhaustive()
    }
}

impl FtsIndex {
    /// Open or create a Tantivy index in the given directory.
    pub fn open(dir: &camino::Utf8Path) -> Result<Self, ShiroError> {
        let mut builder = Schema::builder();
        let f_doc_id = builder.add_text_field("doc_id", STRING | STORED);
        let f_segment_id = builder.add_text_field("segment_id", STRING | STORED);
        let f_seg_index = builder.add_u64_field("seg_index", STORED);
        let f_body = builder.add_text_field("body", TEXT | STORED);
        let f_span_start = builder.add_u64_field("span_start", STORED);
        let f_span_end = builder.add_u64_field("span_end", STORED);
        let schema = builder.build();

        std::fs::create_dir_all(dir.as_std_path())?;

        let mmap_dir = tantivy::directory::MmapDirectory::open(dir.as_std_path()).map_err(|e| {
            ShiroError::IndexBuildFts {
                message: e.to_string(),
            }
        })?;
        let index = Index::open_or_create(mmap_dir, schema).map_err(map_tantivy)?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(map_tantivy)?;

        Ok(Self {
            index,
            reader,
            f_doc_id,
            f_segment_id,
            f_seg_index,
            f_body,
            f_span_start,
            f_span_end,
        })
    }

    /// Index a batch of segments. This is additive.
    pub fn index_segments(&self, segments: &[Segment]) -> Result<(), ShiroError> {
        let mut writer: IndexWriter = self.index.writer(50_000_000).map_err(map_tantivy)?;

        for seg in segments {
            let tantivy_doc = doc!(
                self.f_doc_id => seg.doc_id.as_str(),
                self.f_segment_id => seg.id.as_str(),
                self.f_seg_index => seg.index as u64,
                self.f_body => seg.body.as_str(),
                self.f_span_start => seg.span.start() as u64,
                self.f_span_end => seg.span.end() as u64,
            );
            writer.add_document(tantivy_doc).map_err(map_tantivy)?;
        }

        writer.commit().map_err(map_tantivy)?;
        self.reader.reload().map_err(map_tantivy)?;
        Ok(())
    }

    /// Delete all segments for a given doc_id.
    pub fn delete_doc(&self, doc_id: &DocId) -> Result<(), ShiroError> {
        let mut writer: IndexWriter = self.index.writer(50_000_000).map_err(map_tantivy)?;
        let term = Term::from_field_text(self.f_doc_id, doc_id.as_str());
        writer.delete_term(term);
        writer.commit().map_err(map_tantivy)?;
        self.reader.reload().map_err(map_tantivy)?;
        Ok(())
    }

    /// Search with BM25 ranking.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<FtsHit>, ShiroError> {
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.f_body]);

        let parsed = query_parser.parse_query(query).map_err(map_query_parse)?;
        let top_docs = searcher
            .search(&parsed, &TopDocs::with_limit(limit))
            .map_err(map_tantivy_search)?;

        let mut hits = Vec::with_capacity(top_docs.len());
        for (rank, (score, doc_addr)) in top_docs.into_iter().enumerate() {
            let retrieved: TantivyDocument = searcher.doc(doc_addr).map_err(map_tantivy_search)?;

            let doc_id_val = retrieved
                .get_first(self.f_doc_id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();

            let segment_id_val = retrieved
                .get_first(self.f_segment_id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();

            let seg_index_val = retrieved
                .get_first(self.f_seg_index)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;

            let body_val = retrieved
                .get_first(self.f_body)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();

            let span_start_val = retrieved
                .get_first(self.f_span_start)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;

            let span_end_val = retrieved
                .get_first(self.f_span_end)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;

            hits.push(FtsHit {
                doc_id: doc_id_val,
                segment_id: segment_id_val,
                seg_index: seg_index_val,
                body: body_val,
                span_start: span_start_val,
                span_end: span_end_val,
                bm25_score: score,
                bm25_rank: rank + 1,
            });
        }

        Ok(hits)
    }

    /// Count total indexed documents (segments in Tantivy terms).
    pub fn num_segments(&self) -> Result<u64, ShiroError> {
        let searcher = self.reader.searcher();
        Ok(searcher.num_docs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_core::id::SegmentId;
    use shiro_core::span::Span;

    fn test_segment(doc_id: &DocId, index: usize, body: &str) -> Segment {
        Segment {
            id: SegmentId::new(doc_id, index),
            doc_id: doc_id.clone(),
            index,
            span: Span::new(0, body.len()).expect("test span"),
            body: body.to_string(),
        }
    }

    #[test]
    fn test_index_and_search() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = camino::Utf8Path::from_path(tmp.path()).unwrap();
        let fts = FtsIndex::open(dir).unwrap();

        let doc_id = DocId::from_content(b"test-doc-1");
        let segments = vec![
            test_segment(&doc_id, 0, "the quick brown fox jumps"),
            test_segment(&doc_id, 1, "over the lazy dog"),
            test_segment(&doc_id, 2, "rust is great for systems programming"),
        ];

        fts.index_segments(&segments).unwrap();
        assert_eq!(fts.num_segments().unwrap(), 3);

        let results = fts.search("fox", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].body.contains("fox"));
        assert_eq!(results[0].bm25_rank, 1);
        assert_eq!(results[0].doc_id, doc_id.as_str());

        let results = fts.search("rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].body.contains("rust"));
    }

    #[test]
    fn test_empty_search() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = camino::Utf8Path::from_path(tmp.path()).unwrap();
        let fts = FtsIndex::open(dir).unwrap();

        let results = fts.search("anything", 10).unwrap();
        assert!(results.is_empty());

        let results = fts.search("", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_delete_doc() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = camino::Utf8Path::from_path(tmp.path()).unwrap();
        let fts = FtsIndex::open(dir).unwrap();

        let doc_id_a = DocId::from_content(b"doc-a");
        let doc_id_b = DocId::from_content(b"doc-b");

        let segments = vec![
            test_segment(&doc_id_a, 0, "alpha bravo charlie"),
            test_segment(&doc_id_b, 0, "delta echo foxtrot"),
        ];
        fts.index_segments(&segments).unwrap();
        assert_eq!(fts.num_segments().unwrap(), 2);

        fts.delete_doc(&doc_id_a).unwrap();
        assert_eq!(fts.num_segments().unwrap(), 1);

        let results = fts.search("alpha", 10).unwrap();
        assert!(results.is_empty());

        let results = fts.search("delta", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].doc_id, doc_id_b.as_str());
    }
}
