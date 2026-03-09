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
}

impl Engine {
    /// Open an engine rooted at the given [`ShiroHome`].
    pub fn open(home: ShiroHome) -> Result<Self, ShiroError> {
        let store = Store::open(&home.db_path())?;
        let fts = FtsIndex::open(&home.tantivy_dir())?;
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
        ops::ingest::execute(&self.store, &self.fts, parser, input, on_event)
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
