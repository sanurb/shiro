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
mod engine;
pub mod executor;
mod fusion;
pub mod ops;
pub mod spec;

pub use dsl::{CallTarget, ExecutionResult, Limits, Node, StepTrace};
pub use engine::Engine;
pub use fusion::{reciprocal_rank_fusion, FusedHit, RankedHit, RRF_K};
pub use ops::add::{AddInput, AddOutput};
pub use ops::doctor::{DoctorCheck, DoctorInput, DoctorOutput};
pub use ops::enrich::{EnrichInput, EnrichOutput};
pub use ops::explain::{ExplainInput, ExplainOutput, RetrievalTrace};
pub use ops::ingest::{IngestEvent, IngestInput, IngestOutput};
pub use ops::list::{ListInput, ListOutput};
pub use ops::read::{ReadInput, ReadMode, ReadOutput};
pub use ops::reindex::ReindexOutput;
pub use ops::remove::{RemoveInput, RemoveOutput};
pub use ops::search::{
    ContextBlock, SearchHit, SearchInput, SearchMode, SearchOutput, SearchScores,
};

/// Schema version for SDK output types.
///
/// Bump when any output struct shape changes. CLI and MCP embed this in
/// their responses so consumers can detect breaking changes.
pub const SCHEMA_VERSION: u32 = 3;
