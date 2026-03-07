//! `Engine` — root entry point holding open handles to stores and indices.

use shiro_core::ports::Parser;
use shiro_core::{ShiroError, ShiroHome};
use shiro_index::FtsIndex;
use shiro_store::Store;

use crate::ops;

/// Central handle to the shiro knowledge engine.
///
/// Holds open connections to the Store (SQLite) and FtsIndex (Tantivy).
/// Every SDK operation is available as a method on this struct.
pub struct Engine {
    pub store: Store,
    pub fts: FtsIndex,
    pub home: ShiroHome,
}

impl Engine {
    /// Open an engine rooted at the given [`ShiroHome`].
    pub fn open(home: ShiroHome) -> Result<Self, ShiroError> {
        let store = Store::open(&home.db_path())?;
        let fts = FtsIndex::open(&home.tantivy_dir())?;
        Ok(Self { store, fts, home })
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

    /// Search indexed documents.
    pub fn search(
        &self,
        input: &ops::search::SearchInput,
    ) -> Result<ops::search::SearchOutput, ShiroError> {
        ops::search::execute(&self.store, &self.fts, input)
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

    /// Run diagnostic checks.
    pub fn doctor(
        home: &ShiroHome,
        input: &ops::doctor::DoctorInput,
    ) -> Result<ops::doctor::DoctorOutput, ShiroError> {
        ops::doctor::execute(home, input)
    }
}
