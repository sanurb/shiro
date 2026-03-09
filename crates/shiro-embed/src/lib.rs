//! Vector embedding index implementations for shiro.

mod flat;
mod http;
mod stub;

pub use flat::FlatIndex;
pub use http::{HttpEmbedder, HttpEmbedderConfig};
pub use stub::{DeterministicStubEmbedder, StubEmbedder};
