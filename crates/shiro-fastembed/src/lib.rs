//! `shiro-fastembed` — local embedding and reranking via FastEmbed (ONNX Runtime).
//!
//! This crate provides adapter implementations of the `Embedder` and `Reranker`
//! traits from `shiro-core`, backed by the `fastembed` crate for local inference.
//! No external service required — models are downloaded from HuggingFace Hub
//! and cached locally.

mod embedder;
mod reranker;

pub use embedder::{FastEmbedEmbedder, FastEmbedEmbedderConfig};
pub use reranker::{FastEmbedReranker, FastEmbedRerankerConfig};
