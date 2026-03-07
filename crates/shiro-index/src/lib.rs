//! Full-text search index backed by Tantivy (BM25 ranking).

use shiro_core::error::ShiroError;
use shiro_core::id::DocId;
use shiro_core::ir::Segment;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::document::TantivyDocument;
use tantivy::schema::{
    IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING,
};
use tantivy::tokenizer::{LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer};
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
    gen_id: u64,
}

impl std::fmt::Debug for FtsIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FtsIndex")
            .field("gen_id", &self.gen_id)
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
    /// Open or create a Tantivy index in the given directory (generation 0).
    pub fn open(dir: &camino::Utf8Path) -> Result<Self, ShiroError> {
        Self::open_generation(dir, 0)
    }

    /// Open or create a Tantivy index tracking the given generation.
    pub fn open_generation(dir: &camino::Utf8Path, gen_id: u64) -> Result<Self, ShiroError> {
        let mut builder = Schema::builder();
        let f_doc_id = builder.add_text_field("doc_id", STRING | STORED);
        let f_segment_id = builder.add_text_field("segment_id", STRING | STORED);
        let f_seg_index = builder.add_u64_field("seg_index", STORED);
        let text_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("shiro_default")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        let f_body = builder.add_text_field("body", text_options);
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

        let tokenizer = TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(RemoveLongFilter::limit(40))
            .filter(LowerCaser)
            .build();
        index.tokenizers().register("shiro_default", tokenizer);

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
            gen_id,
        })
    }

    /// Return the generation ID this index instance tracks.
    pub fn gen_id(&self) -> u64 {
        self.gen_id
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

    /// Build a fresh index from segments in a staging directory.
    ///
    /// This creates a new index at `staging_dir`, indexes all segments,
    /// then returns. The caller is responsible for atomic rename.
    pub fn build_from_segments(
        staging_dir: &camino::Utf8Path,
        segments: &[Segment],
        gen_id: u64,
    ) -> Result<(), ShiroError> {
        // Remove staging dir if it exists (leftover from failed build)
        if staging_dir.as_std_path().exists() {
            std::fs::remove_dir_all(staging_dir.as_std_path())?;
        }

        // Create fresh index
        let fts = Self::open_generation(staging_dir, gen_id)?;

        // Index all segments in one batch
        if !segments.is_empty() {
            fts.index_segments(segments)?;
        }

        tracing::info!(segments = segments.len(), dir = %staging_dir, "built staging index");
        Ok(())
    }

    /// Promote a staging index by atomically replacing the live index.
    ///
    /// On Unix, this uses `rename()` which is atomic.
    /// Returns the old index directory path (caller should clean up).
    pub fn promote_staging(
        staging_dir: &camino::Utf8Path,
        live_dir: &camino::Utf8Path,
    ) -> Result<Option<camino::Utf8PathBuf>, ShiroError> {
        let backup = if live_dir.as_std_path().exists() {
            let backup_dir = live_dir.with_extension("old");
            std::fs::rename(live_dir.as_std_path(), backup_dir.as_std_path())?;
            Some(camino::Utf8PathBuf::from(backup_dir.to_string()))
        } else {
            None
        };

        std::fs::rename(staging_dir.as_std_path(), live_dir.as_std_path())?;

        // Clean up backup
        if let Some(ref backup) = backup {
            let _ = std::fs::remove_dir_all(backup.as_std_path());
        }

        tracing::info!(from = %staging_dir, to = %live_dir, "promoted staging index");
        Ok(backup)
    }

    /// Compute the directory path for a specific FTS generation.
    pub fn gen_dir(base: &camino::Utf8Path, gen_id: u64) -> camino::Utf8PathBuf {
        if gen_id == 0 {
            base.to_owned()
        } else {
            let dir_name = format!("tantivy_gen_{gen_id}");
            base.parent().unwrap_or(base).join(dir_name)
        }
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

    #[test]
    fn test_deterministic_tokenization() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = camino::Utf8Path::from_path(tmp.path()).unwrap();
        let fts = FtsIndex::open(dir).unwrap();

        let doc_id = DocId::from_content(b"token-test");
        let segments = vec![test_segment(&doc_id, 0, "The Quick Brown Fox")];
        fts.index_segments(&segments).unwrap();

        // Case-insensitive search should work
        let results = fts.search("quick", 10).unwrap();
        assert_eq!(results.len(), 1, "lowercase query should match");

        let results = fts.search("QUICK", 10).unwrap();
        assert_eq!(results.len(), 1, "uppercase query should match");

        // Long tokens should be filtered
        let doc_id2 = DocId::from_content(b"long-token");
        let long_word = "a".repeat(50);
        let segments2 = vec![test_segment(&doc_id2, 0, &long_word)];
        fts.index_segments(&segments2).unwrap();
        let results = fts.search(&long_word, 10).unwrap();
        assert!(results.is_empty(), "tokens >40 chars should be filtered");
    }

    #[test]
    fn test_build_from_segments() {
        let tmp = tempfile::TempDir::new().unwrap();
        let staging_path = tmp.path().join("staging");
        let staging = camino::Utf8Path::from_path(&staging_path).unwrap();
        let live_path = tmp.path().join("live");
        let live = camino::Utf8Path::from_path(&live_path).unwrap();

        let doc_id = DocId::from_content(b"gen-test");
        let segments = vec![test_segment(&doc_id, 0, "generational index test content")];

        // Build staging index
        FtsIndex::build_from_segments(staging, &segments, 1).unwrap();
        assert!(staging.as_std_path().exists());

        // Promote to live
        FtsIndex::promote_staging(staging, live).unwrap();
        assert!(live.as_std_path().exists());
        assert!(!staging.as_std_path().exists());

        // Verify live index works
        let fts = FtsIndex::open(live).unwrap();
        let results = fts.search("generational", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_generation_tracking() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = camino::Utf8Path::from_path(tmp.path()).unwrap();

        let fts = FtsIndex::open(dir).unwrap();
        assert_eq!(fts.gen_id(), 0, "default open should be gen 0");

        let tmp2 = tempfile::TempDir::new().unwrap();
        let dir2 = camino::Utf8Path::from_path(tmp2.path()).unwrap();
        let fts2 = FtsIndex::open_generation(dir2, 42).unwrap();
        assert_eq!(fts2.gen_id(), 42);
    }

    #[test]
    fn test_gen_dir_naming() {
        let base = camino::Utf8Path::new("/data/tantivy");

        // gen 0 returns base unchanged
        let dir0 = FtsIndex::gen_dir(base, 0);
        assert_eq!(dir0, base);

        // gen N returns sibling directory
        let dir5 = FtsIndex::gen_dir(base, 5);
        assert_eq!(dir5, camino::Utf8PathBuf::from("/data/tantivy_gen_5"));

        // gen with root-level path
        let root = camino::Utf8Path::new("tantivy");
        let dir1 = FtsIndex::gen_dir(root, 1);
        assert_eq!(dir1, camino::Utf8PathBuf::from("tantivy_gen_1"));
    }
}
