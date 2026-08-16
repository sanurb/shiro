//! Embedding provider adapters and test doubles for shiro.

mod http;
mod stub;

pub use http::{HttpEmbedder, HttpEmbedderConfig};
/// Backward-compatible export; the flat vector index now lives in `shiro-index`.
pub use shiro_index::FlatIndex;
pub use stub::{DeterministicStubEmbedder, StubEmbedder};
