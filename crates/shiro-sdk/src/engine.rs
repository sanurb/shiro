//! `Engine` — root entry point holding open handles to stores and indices.

use shiro_core::ports::{Embedder, Parser, Reranker, VectorIndex};
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::{FlatIndex, FtsIndex};
use shiro_store::Store;

use crate::ops;

/// Central handle to the shiro knowledge engine.
///
/// Holds open connections to the Store (SQLite), FtsIndex (Tantivy), and
/// optionally an Embedder, VectorIndex, and Reranker for hybrid retrieval.
pub struct Engine {
    pub store: Store,
    pub fts: FtsIndex,
    pub home: ShiroHome,
    embedder: Option<Box<dyn Embedder>>,
    vector_index: Option<Box<dyn VectorIndex>>,
    reranker: Option<Box<dyn Reranker>>,
    automatic_concept_proposals: bool,
}

impl Engine {
    /// Open an engine rooted at the given [`ShiroHome`].
    pub fn open(home: ShiroHome) -> Result<Self, ShiroError> {
        let store = Store::open(&home.db_path())?;
        ops::corpus_publication::verify_active_manifest_artifacts(&home, &store)?;
        let generation = store.active_generation("fts")?.as_u64();
        let fts = FtsIndex::open_generation(&home.tantivy_generation_dir(generation), generation)?;
        Ok(Self {
            store,
            fts,
            home,
            embedder: None,
            vector_index: None,
            reranker: None,
            automatic_concept_proposals: true,
        })
    }

    /// Attach an embedder for vector search.
    pub fn with_embedder(mut self, embedder: Box<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// Attach a vector index for semantic retrieval.
    pub fn with_vector_index(mut self, index: Box<dyn VectorIndex>) -> Self {
        self.vector_index = Some(index);
        self
    }

    /// Attach a reranker for post-fusion reranking.
    pub fn with_reranker(mut self, reranker: Box<dyn Reranker>) -> Self {
        self.reranker = Some(reranker);
        self
    }

    /// Enable or disable automatic PROPOSED concept assignments after ingest.
    pub fn with_automatic_concept_proposals(mut self, enabled: bool) -> Self {
        self.automatic_concept_proposals = enabled;
        self
    }

    /// Reference to the embedder, if configured.
    pub fn embedder(&self) -> Option<&dyn Embedder> {
        self.embedder.as_deref()
    }

    /// Reference to the vector index, if configured.
    pub fn vector_index(&self) -> Option<&dyn VectorIndex> {
        self.vector_index.as_deref()
    }

    /// Reference to the reranker, if configured.
    pub fn reranker(&self) -> Option<&dyn Reranker> {
        self.reranker.as_deref()
    }

    /// Add a single document from a file path.
    pub fn add(
        &self,
        parser: &dyn Parser,
        input: &ops::add::AddInput,
    ) -> Result<ops::add::AddOutput, ShiroError> {
        ops::add::execute(&self.store, &self.fts, parser, input)
    }

    /// Acquire and ingest a bounded remote PDF or UTF-8 text source.
    pub fn acquire_url(
        &self,
        input: &ops::acquire::AcquireUrlInput,
    ) -> Result<ops::acquire::AcquireUrlOutput, ShiroError> {
        ops::acquire::execute(&self.store, &self.fts, input)
    }

    /// Acquire a remote source and incrementally publish configured embeddings.
    pub fn acquire_url_incremental(
        &mut self,
        input: &ops::acquire::AcquireUrlInput,
        embedding_batch_size: usize,
    ) -> Result<ops::acquire::AcquireUrlOutput, ShiroError> {
        let Some(embedder) = self.embedder.as_deref() else {
            return ops::acquire::execute(&self.store, &self.fts, input);
        };
        let generations = ops::corpus_publication::reserve_incremental_generations(&self.store)?;
        let output = self.store.with_atomic_corpus_publication(|| {
            let mut publish = |staged: &ops::document_ingestion::StagedDocumentIngestion| {
                ops::corpus_publication::publish_reserved_incremental_staged(
                    &self.home,
                    &self.store,
                    embedder,
                    std::slice::from_ref(&staged.doc_id),
                    embedding_batch_size,
                    generations,
                )
                .map(|_| ())
            };
            ops::acquire::execute_with_publisher(&self.store, input, &mut publish)
        })?;
        if output.changed {
            let manifest =
                self.store
                    .active_corpus_manifest()?
                    .ok_or_else(|| ShiroError::StoreCorrupt {
                        message: "URL acquisition activated no corpus manifest".to_string(),
                    })?;
            self.fts = FtsIndex::open_generation(
                &self
                    .home
                    .tantivy_generation_dir(manifest.fts_generation.as_u64()),
                manifest.fts_generation.as_u64(),
            )?;
            if let Some(vector_generation) = manifest.vector_generation {
                let fingerprint = crate::retrieval_embedding_fingerprint(&embedder.fingerprint());
                self.vector_index = Some(Box::new(FlatIndex::open_generation_compatible(
                    embedder.dimensions(),
                    self.home.vector_data_path(vector_generation.as_u64()),
                    vector_generation.as_u64(),
                    &fingerprint,
                )?));
            }
        }
        Ok(output)
    }

    /// Add a document and publish a complete FTS/vector generation while
    /// embedding only changed segments in bounded batches.
    ///
    /// Falls back to the BM25-only add path when no embedder is attached.
    pub fn add_incremental(
        &mut self,
        parser: &dyn Parser,
        input: &ops::add::AddInput,
        embedding_batch_size: usize,
    ) -> Result<ops::add::AddOutput, ShiroError> {
        let Some(embedder) = self.embedder.as_deref() else {
            return ops::add::execute(&self.store, &self.fts, parser, input);
        };
        let content = std::fs::read(&input.path)?;
        let generations = ops::corpus_publication::reserve_incremental_generations(&self.store)?;
        let (staged, publication) = self.store.with_atomic_corpus_publication(|| {
            let staged = ops::document_ingestion::stage_document_bytes(
                &self.store,
                parser,
                &input.path,
                &content,
            )?;
            if !staged.changed {
                return Ok((staged, None));
            }
            let publication = ops::corpus_publication::publish_reserved_incremental_staged(
                &self.home,
                &self.store,
                embedder,
                std::slice::from_ref(&staged.doc_id),
                embedding_batch_size,
                generations,
            )?;
            Ok((staged, Some(publication)))
        })?;
        let Some(publication) = publication else {
            return Ok(ops::add::AddOutput {
                doc_id: staged.doc_id.as_str().to_string(),
                status: "READY".to_string(),
                title: staged.title,
                segments: staged.segments.len(),
                changed: false,
                incremental_publication: None,
            });
        };

        self.fts = FtsIndex::open_generation(
            &self.home.tantivy_generation_dir(publication.fts_generation),
            publication.fts_generation,
        )?;
        let fingerprint = crate::retrieval_embedding_fingerprint(&embedder.fingerprint());
        self.vector_index = Some(Box::new(FlatIndex::open_generation_compatible(
            embedder.dimensions(),
            self.home.vector_data_path(publication.vector_generation),
            publication.vector_generation,
            &fingerprint,
        )?));
        Ok(ops::add::AddOutput {
            doc_id: staged.doc_id.as_str().to_string(),
            status: "READY".to_string(),
            title: staged.title,
            segments: staged.segments.len(),
            changed: true,
            incremental_publication: Some(publication),
        })
    }

    /// Batch-ingest documents from directories.
    pub fn ingest(
        &self,
        parser: &dyn Parser,
        input: &ops::ingest::IngestInput,
        on_event: Option<&dyn Fn(&ops::ingest::IngestEvent)>,
    ) -> Result<ops::ingest::IngestOutput, ShiroError> {
        let mut output = ops::ingest::execute(&self.store, &self.fts, parser, input, on_event)?;
        self.attach_automatic_concept_proposals(&mut output)?;
        Ok(output)
    }

    /// Batch-ingest and publish configured embeddings in bounded batches.
    pub fn ingest_incremental(
        &mut self,
        parser: &dyn Parser,
        input: &ops::ingest::IngestInput,
        on_event: Option<&dyn Fn(&ops::ingest::IngestEvent)>,
        embedding_batch_size: usize,
    ) -> Result<ops::ingest::IngestOutput, ShiroError> {
        let Some(embedder) = self.embedder.as_deref() else {
            return self.ingest(parser, input, on_event);
        };
        let generations = ops::corpus_publication::reserve_incremental_generations(&self.store)?;
        let publication_failed = std::cell::Cell::new(false);
        let mut rolled_back_output = None;
        let transaction = self.store.with_atomic_corpus_publication(|| {
            let mut publish = |staged: &[&ops::document_ingestion::StagedDocumentIngestion]| {
                if staged.is_empty() {
                    return Ok(());
                }
                let document_ids = staged
                    .iter()
                    .map(|document| document.doc_id.clone())
                    .collect::<Vec<_>>();
                ops::corpus_publication::publish_reserved_incremental_staged(
                    &self.home,
                    &self.store,
                    embedder,
                    &document_ids,
                    embedding_batch_size,
                    generations,
                )
                .map(|_| ())
                .map_err(|error| {
                    publication_failed.set(true);
                    error
                })
            };
            let output = ops::ingest::execute_with_publisher(
                &self.store,
                parser,
                input,
                on_event,
                &mut publish,
            )?;
            if publication_failed.get() {
                rolled_back_output = Some(output);
                return Err(ShiroError::IndexBuildVec {
                    message: "incremental ingest publication rolled back".to_string(),
                });
            }
            Ok(output)
        });
        let mut output = match transaction {
            Ok(output) => output,
            Err(_) if rolled_back_output.is_some() => {
                rolled_back_output.ok_or_else(|| ShiroError::StoreCorrupt {
                    message: "incremental ingest rollback lost failure output".to_string(),
                })?
            }
            Err(error) => return Err(error),
        };
        if output.added > 0 {
            let manifest =
                self.store
                    .active_corpus_manifest()?
                    .ok_or_else(|| ShiroError::StoreCorrupt {
                        message: "incremental ingest activated no corpus manifest".to_string(),
                    })?;
            self.fts = FtsIndex::open_generation(
                &self
                    .home
                    .tantivy_generation_dir(manifest.fts_generation.as_u64()),
                manifest.fts_generation.as_u64(),
            )?;
            if let Some(vector_generation) = manifest.vector_generation {
                let fingerprint = crate::retrieval_embedding_fingerprint(&embedder.fingerprint());
                self.vector_index = Some(Box::new(FlatIndex::open_generation_compatible(
                    embedder.dimensions(),
                    self.home.vector_data_path(vector_generation.as_u64()),
                    vector_generation.as_u64(),
                    &fingerprint,
                )?));
            }
        }
        self.attach_automatic_concept_proposals(&mut output)?;
        Ok(output)
    }

    fn attach_automatic_concept_proposals(
        &self,
        output: &mut ops::ingest::IngestOutput,
    ) -> Result<(), ShiroError> {
        if !self.automatic_concept_proposals {
            return Ok(());
        }
        for doc_id in &output.ingested_document_ids {
            if let Some(proposal) = ops::model_enrichment::propose_automatic_document_concepts(
                &self.store,
                self.embedder.as_deref(),
                doc_id,
            )? {
                output.concept_proposals.push(proposal);
            }
        }
        Ok(())
    }

    /// Search indexed documents with optional hybrid retrieval and reranking.
    pub fn search(
        &self,
        input: &ops::search::SearchInput,
    ) -> Result<ops::search::SearchOutput, ShiroError> {
        ops::search::execute(
            &self.store,
            &self.fts,
            self.embedder.as_deref(),
            self.vector_index.as_deref(),
            self.reranker.as_deref(),
            input,
        )
    }

    /// Evaluate a versioned judged benchmark manifest against this engine.
    pub fn benchmark(
        &self,
        manifest: &ops::benchmark::BenchmarkManifest,
        warmup_runs: usize,
        measured_runs: usize,
    ) -> Result<ops::benchmark::BenchmarkOutput, ShiroError> {
        ops::benchmark::execute(self, manifest, warmup_runs, measured_runs)
    }

    /// Plan or execute bounded scoped reprocessing from persisted source artifacts.
    pub fn reprocess(
        &mut self,
        parser: &dyn Parser,
        input: &ops::reprocess::ReprocessInput,
    ) -> Result<ops::reprocess::ReprocessOutput, ShiroError> {
        let output = ops::reprocess::execute(
            &self.home,
            &self.store,
            &self.fts,
            parser,
            self.embedder.as_deref(),
            input,
        )?;
        if output.status == "executed" {
            let manifest =
                self.store
                    .active_corpus_manifest()?
                    .ok_or_else(|| ShiroError::StoreCorrupt {
                        message: "reprocessing activated no corpus manifest".to_string(),
                    })?;
            self.fts = FtsIndex::open_generation(
                &self
                    .home
                    .tantivy_generation_dir(manifest.fts_generation.as_u64()),
                manifest.fts_generation.as_u64(),
            )?;
            if let (Some(embedder), Some(vector_generation)) =
                (self.embedder.as_deref(), manifest.vector_generation)
            {
                let fingerprint = crate::retrieval_embedding_fingerprint(&embedder.fingerprint());
                self.vector_index = Some(Box::new(FlatIndex::open_generation_compatible(
                    embedder.dimensions(),
                    self.home.vector_data_path(vector_generation.as_u64()),
                    vector_generation.as_u64(),
                    &fingerprint,
                )?));
            } else {
                self.vector_index = None;
            }
        }
        Ok(output)
    }

    /// Run multiple queries and return deduplicated stable evidence handles.
    pub fn search_pack(
        &self,
        input: &ops::search_pack::SearchPackInput,
    ) -> Result<ops::search_pack::SearchPackOutput, ShiroError> {
        ops::search_pack::execute(
            &self.store,
            &self.fts,
            self.embedder.as_deref(),
            self.vector_index.as_deref(),
            self.reranker.as_deref(),
            input,
        )
    }

    /// Store model output as an isolated, attributed proposal.
    pub fn propose_model_enrichment(
        &self,
        input: &ops::model_enrichment::ModelEnrichmentProposalInput,
    ) -> Result<ops::model_enrichment::ModelEnrichmentProposalOutput, ShiroError> {
        ops::model_enrichment::propose(&self.store, input)
    }

    /// Explicitly promote or reject a model-enrichment proposal.
    pub fn resolve_model_enrichment(
        &self,
        input: &ops::model_enrichment::ModelEnrichmentResolutionInput,
    ) -> Result<ops::model_enrichment::ModelEnrichmentResolutionOutput, ShiroError> {
        ops::model_enrichment::resolve(&self.store, input)
    }

    /// Search taxonomy labels and text fallbacks.
    pub fn taxonomy_search(
        &self,
        input: &ops::taxonomy::TaxonomySearchInput,
    ) -> Result<ops::taxonomy::TaxonomySearchOutput, ShiroError> {
        ops::taxonomy::search(&self.store, input)
    }

    /// Browse a bounded taxonomy graph.
    pub fn taxonomy_browse(
        &self,
        input: &ops::taxonomy::TaxonomyBrowseInput,
    ) -> Result<ops::taxonomy::TaxonomyBrowseOutput, ShiroError> {
        ops::taxonomy::browse(&self.store, input)
    }

    /// List documents in the store.
    pub fn list(&self, input: &ops::list::ListInput) -> Result<ops::list::ListOutput, ShiroError> {
        ops::list::execute(&self.store, input)
    }

    /// Read a document's content.
    pub fn read(&self, input: &ops::read::ReadInput) -> Result<ops::read::ReadOutput, ShiroError> {
        ops::read::execute(&self.store, input)
    }

    /// Explain a search result.
    pub fn explain(
        &self,
        input: &ops::explain::ExplainInput,
    ) -> Result<ops::explain::ExplainOutput, ShiroError> {
        ops::explain::execute(&self.store, input)
    }

    /// Remove a document.
    pub fn remove(
        &self,
        input: &ops::remove::RemoveInput,
    ) -> Result<ops::remove::RemoveOutput, ShiroError> {
        ops::remove::execute(&self.store, Some(&self.fts), input)
    }

    /// Run enrichment on a document.
    pub fn enrich(
        &self,
        input: &ops::enrich::EnrichInput,
    ) -> Result<ops::enrich::EnrichOutput, ShiroError> {
        ops::enrich::execute(&self.store, input)
    }

    /// Rebuild the FTS index from stored segments.
    pub fn reindex(&self) -> Result<ops::reindex::ReindexOutput, ShiroError> {
        ops::reindex::execute(&self.home, &self.store)
    }

    /// Rebuild all configured indices and atomically activate one corpus manifest.
    pub fn reindex_all(&self) -> Result<Vec<ops::reindex::ReindexOutput>, ShiroError> {
        ops::reindex::execute_all(&self.home, &self.store, self.embedder.as_deref())
    }

    /// Rebuild the vector index using the configured embedder.
    pub fn reindex_vector(&self) -> Result<ops::reindex::ReindexOutput, ShiroError> {
        let embedder = self
            .embedder
            .as_deref()
            .ok_or_else(|| ShiroError::EmbedFail {
                message: "no embedder configured for vector reindex".to_string(),
            })?;
        ops::reindex::execute_vector(&self.home, &self.store, embedder)
    }

    /// Run diagnostic checks.
    pub fn doctor(
        home: &ShiroHome,
        input: &ops::doctor::DoctorInput,
    ) -> Result<ops::doctor::DoctorOutput, ShiroError> {
        ops::doctor::execute(home, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_core::{DocId, EmbeddingFingerprint, EmbeddingMeta};
    use shiro_parse::{MarkdownParser, PlainTextParser};

    struct FailingIncrementalEmbedder {
        fingerprint: EmbeddingFingerprint,
    }

    impl Embedder for FailingIncrementalEmbedder {
        fn embed(&self, _text: &str) -> Result<Vec<f32>, ShiroError> {
            Err(ShiroError::EmbedFail {
                message: "incremental publication test embedder failed".to_string(),
            })
        }

        fn dimensions(&self) -> usize {
            self.fingerprint.dimensions
        }

        fn meta(&self) -> EmbeddingMeta {
            EmbeddingMeta {
                provider: self.fingerprint.provider.clone(),
                model_name: self.fingerprint.model.clone(),
                dimensions: self.fingerprint.dimensions,
            }
        }

        fn fingerprint(&self) -> EmbeddingFingerprint {
            self.fingerprint.clone()
        }
    }

    #[test]
    fn successful_incremental_add_is_immediately_hybrid_searchable() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(temporary.path())
            .unwrap()
            .to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let initial_path = home.root().join("initial.txt");
        std::fs::write(initial_path.as_std_path(), b"initial corpus evidence").unwrap();
        let initial_input = ops::add::AddInput {
            path: initial_path.as_str().to_string(),
        };
        let initial = Engine::open(home.clone()).unwrap();
        initial.add(&PlainTextParser, &initial_input).unwrap();
        let published =
            initial.with_embedder(Box::new(shiro_embed::DeterministicStubEmbedder::new(8)));
        published.reindex_all().unwrap();
        drop(published);

        let added_path = home.root().join("incremental.txt");
        std::fs::write(
            added_path.as_std_path(),
            b"new immediate hybrid freshness evidence",
        )
        .unwrap();
        let added_input = ops::add::AddInput {
            path: added_path.as_str().to_string(),
        };
        let mut engine = Engine::open(home)
            .unwrap()
            .with_embedder(Box::new(shiro_embed::DeterministicStubEmbedder::new(8)));
        let added = engine
            .add_incremental(&PlainTextParser, &added_input, 1)
            .unwrap();
        assert!(added.changed);
        let publication = added.incremental_publication.unwrap();
        assert_eq!(publication.embedded_segments, 1);
        assert_eq!(publication.reused_segments, 1);

        let search = engine
            .search(&ops::search::SearchInput {
                query: "hybrid freshness".to_string(),
                mode: ops::search::SearchMode::Hybrid,
                limit: 5,
                expand: false,
                max_blocks: 12,
                max_chars: 8_000,
                rerank: false,
                filters: ops::search::SearchFilters::default(),
            })
            .unwrap();
        assert!(search.retrieval_info.vector_active);
        assert!(search.hits.iter().any(|hit| hit.doc_id == added.doc_id));
    }

    #[test]
    fn failed_incremental_batch_preserves_previous_documents_and_reports_failures() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(temporary.path())
            .unwrap()
            .to_owned();
        let home = ShiroHome::new(root.clone());
        home.ensure_dirs().unwrap();
        let source_path = root.join("batch.md");
        let content = b"# Batch Atomic Evidence\n\nprevious batch content";
        std::fs::write(source_path.as_std_path(), content).unwrap();
        let add_input = ops::add::AddInput {
            path: source_path.as_str().to_string(),
        };
        let initial = Engine::open(home.clone()).unwrap();
        initial.add(&PlainTextParser, &add_input).unwrap();
        let deterministic = shiro_embed::DeterministicStubEmbedder::new(8);
        let base_fingerprint = deterministic.fingerprint();
        let published = initial.with_embedder(Box::new(deterministic));
        published.reindex_all().unwrap();
        drop(published);

        let mut failing = Engine::open(home.clone()).unwrap().with_embedder(Box::new(
            FailingIncrementalEmbedder {
                fingerprint: base_fingerprint,
            },
        ));
        let output = failing
            .ingest_incremental(
                &MarkdownParser,
                &ops::ingest::IngestInput {
                    dirs: vec![root.as_str().to_string()],
                    max_files: Some(1),
                },
                None,
                1,
            )
            .unwrap();
        assert_eq!(output.added, 0);
        assert_eq!(output.failed, 1);
        drop(failing);

        let store = shiro_store::Store::open(&home.db_path()).unwrap();
        let doc_id = DocId::from_content(content);
        assert_eq!(
            store.get_document(&doc_id).unwrap().1,
            shiro_core::DocState::Ready
        );
        assert_eq!(
            store.get_fingerprint(&doc_id).unwrap().unwrap().parser_name,
            "plaintext"
        );
    }

    #[test]
    fn failed_incremental_reprocessing_preserves_previous_searchable_document() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = camino::Utf8Path::from_path(temporary.path())
            .unwrap()
            .to_owned();
        let home = ShiroHome::new(root);
        home.ensure_dirs().unwrap();
        let source_path = home.root().join("atomic.md");
        let content = b"# Atomic Evidence\n\nprevious searchable content";
        std::fs::write(source_path.as_std_path(), content).unwrap();
        let input = ops::add::AddInput {
            path: source_path.as_str().to_string(),
        };

        let initial = Engine::open(home.clone()).unwrap();
        initial.add(&PlainTextParser, &input).unwrap();
        let deterministic = shiro_embed::DeterministicStubEmbedder::new(8);
        let base_fingerprint = deterministic.fingerprint();
        let published = initial.with_embedder(Box::new(deterministic));
        published.reindex_all().unwrap();
        drop(published);

        let mut failing = Engine::open(home.clone()).unwrap().with_embedder(Box::new(
            FailingIncrementalEmbedder {
                fingerprint: base_fingerprint,
            },
        ));
        let error = failing
            .add_incremental(&MarkdownParser, &input, 1)
            .unwrap_err();
        assert!(matches!(error, ShiroError::EmbedFail { .. }));
        drop(failing);

        let reopened = Engine::open(home).unwrap();
        let doc_id = DocId::from_content(content);
        let (document, state) = reopened.store.get_document(&doc_id).unwrap();
        assert_eq!(state, shiro_core::DocState::Ready);
        assert_eq!(
            reopened
                .store
                .get_fingerprint(&doc_id)
                .unwrap()
                .unwrap()
                .parser_name,
            "plaintext"
        );
        assert_eq!(
            document.blocks.blocks[0].kind,
            shiro_core::BlockKind::Paragraph
        );
        let search = reopened
            .search(&ops::search::SearchInput {
                query: "searchable content".to_string(),
                mode: ops::search::SearchMode::Bm25,
                limit: 5,
                expand: false,
                max_blocks: 12,
                max_chars: 8_000,
                rerank: false,
                filters: ops::search::SearchFilters::default(),
            })
            .unwrap();
        assert_eq!(search.hits.len(), 1);
        assert_eq!(search.hits[0].doc_id, doc_id.as_str());
    }
}
