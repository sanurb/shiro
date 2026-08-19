//! `Engine` — root entry point holding open handles to stores and indices.

use shiro_core::ports::{Embedder, Parser, Reranker, VectorIndex};
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
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
