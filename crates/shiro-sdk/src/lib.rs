//! `shiro-sdk` — typed API surface for the shiro knowledge engine.
//!
//! All public functionality is reachable through this crate. CLI and MCP are
//! thin adapters that delegate to SDK methods.
//!
//! # Design
//!
//! The SDK is organised around a small set of **operations** that mirror the
//! domain lifecycle: ingest, search, read, explain, enrich, reindex, doctor.
//! Each operation takes typed inputs and returns typed outputs.
//!
//! The [`Engine`] struct is the root entry point — it holds open handles to
//! the underlying stores and indices and exposes every operation as a method.

pub mod dsl;
mod embedding_policy;
mod engine;
pub mod executor;
mod fusion;
pub mod ops;
mod retrieval_policy;
mod retrieval_result;
pub mod spec;

pub use dsl::{CallTarget, ExecutionResult, Limits, Node, StepTrace};
pub use embedding_policy::retrieval_embedding_fingerprint;
pub use engine::Engine;
pub use fusion::{reciprocal_rank_fusion, FusedHit, RankedHit, RRF_K};
pub use ops::acquire::{AcquireUrlInput, AcquireUrlOutput, AcquisitionParser};
pub use ops::add::{AddInput, AddOutput};
pub use ops::benchmark::{
    BenchmarkDocument, BenchmarkHardwareEvidence, BenchmarkHardwareProfile, BenchmarkJudgment,
    BenchmarkManifest, BenchmarkOutput, BenchmarkPipeline, BenchmarkQuery, BenchmarkThresholds,
    LatencyPercentiles, PairedConfidenceInterval, PipelineBenchmarkResult, RebuildIntegrity,
    BENCHMARK_MANIFEST_VERSION,
};
pub use ops::doctor::{DoctorCheck, DoctorInput, DoctorOutput};
pub use ops::enrich::{EnrichInput, EnrichOutput};
pub use ops::explain::{ExplainInput, ExplainOutput, RetrievalTrace};
pub use ops::ingest::{IngestEvent, IngestInput, IngestOutput};
pub use ops::list::{ListInput, ListOutput};
pub use ops::model_enrichment::{
    ModelEnrichmentProposalInput, ModelEnrichmentProposalOutput, ModelEnrichmentResolutionAction,
    ModelEnrichmentResolutionInput, ModelEnrichmentResolutionOutput, ProposedModelConcept,
};
pub use ops::read::{ReadInput, ReadMode, ReadOutput};
pub use ops::reindex::ReindexOutput;
pub use ops::remove::{RemoveInput, RemoveOutput};
pub use ops::reprocess::{
    ReprocessDocumentPlan, ReprocessInput, ReprocessLimits, ReprocessOutput, ReprocessPlan,
    ReprocessTarget,
};
pub use ops::search::{
    ContextBlock, RetrievalInfo, SearchFilters, SearchHit, SearchInput, SearchMode, SearchOutput,
    SearchScores,
};
pub use ops::search_pack::{SearchPackHit, SearchPackInput, SearchPackOutput, SearchPackQuery};
pub use ops::taxonomy::{
    TaxonomyBrowseInput, TaxonomyBrowseOutput, TaxonomyConceptView, TaxonomyEdgeView,
    TaxonomySearchInput, TaxonomySearchOutput,
};

/// Schema version for SDK output types.
///
/// Bump when any output struct shape changes. CLI and MCP embed this in
/// their responses so consumers can detect breaking changes.
pub const SCHEMA_VERSION: u32 = 5;
