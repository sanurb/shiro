//! Application-level runtime composition for the CLI binary.
//!
//! Provider-specific adapters live here, above the provider-agnostic SDK.

use camino::Utf8PathBuf;
use shiro_core::config::{load_config, EmbedConfig, RerankConfig, ShiroConfig};
use shiro_core::ports::Embedder;
use shiro_core::{ShiroError, ShiroHome};
use shiro_embed::{HttpEmbedder, HttpEmbedderConfig};
use shiro_fastembed::{
    FastEmbedEmbedder, FastEmbedEmbedderConfig, FastEmbedReranker, FastEmbedRerankerConfig,
};
use shiro_index::FlatIndex;
use shiro_sdk::Engine;

/// Amount of configured retrieval behavior required by one command or program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeProfile {
    /// Canonical storage and BM25 only; no model adapters are initialized.
    Base,
    /// Configured reranker without embedding initialization.
    RerankOnly,
    /// Configured embedder and vector index without reranker initialization.
    Vector,
    /// Hybrid/vector retrieval with all configured adapters.
    Full,
}

/// Open the CLI Engine with only the adapters required by `profile`.
pub(crate) fn open_engine(home: &ShiroHome, profile: RuntimeProfile) -> Result<Engine, ShiroError> {
    let config = load_config(home)?;
    let mut engine = Engine::open(home.clone())?;
    match profile {
        RuntimeProfile::Base => Ok(engine),
        RuntimeProfile::RerankOnly => {
            if let Some(rerank) = &config.rerank {
                engine = configure_reranker(engine, rerank)?;
            }
            Ok(engine)
        }
        RuntimeProfile::Vector => {
            if let Some(embed) = &config.embed {
                engine = configure_embedder(engine, embed, VectorIndexMode::Strict)?;
            }
            Ok(engine)
        }
        RuntimeProfile::Full => configure_engine(engine, &config, VectorIndexMode::Strict),
    }
}

/// Open the CLI Engine for rebuilding vector artifacts without reading live vectors.
pub(crate) fn open_engine_for_reindex(home: &ShiroHome) -> Result<Engine, ShiroError> {
    let config = load_config(home)?;
    let engine = Engine::open(home.clone())?;
    configure_engine(engine, &config, VectorIndexMode::Skip)
}

/// Rebuild and fingerprint the vector index with the configured embedder.
pub(crate) fn reindex_vector(
    engine: &Engine,
) -> Result<shiro_sdk::ops::reindex::ReindexOutput, ShiroError> {
    engine.reindex_vector()
}

#[derive(Clone, Copy)]
enum VectorIndexMode {
    Strict,
    Skip,
}

fn configure_engine(
    mut engine: Engine,
    config: &ShiroConfig,
    vector_mode: VectorIndexMode,
) -> Result<Engine, ShiroError> {
    if let Some(embed) = &config.embed {
        engine = configure_embedder(engine, embed, vector_mode)?;
    }
    if matches!(vector_mode, VectorIndexMode::Strict) {
        if let Some(rerank) = &config.rerank {
            engine = configure_reranker(engine, rerank)?;
        }
    }
    Ok(engine)
}

fn configure_embedder(
    engine: Engine,
    config: &EmbedConfig,
    vector_mode: VectorIndexMode,
) -> Result<Engine, ShiroError> {
    match config.provider.as_deref().unwrap_or("http") {
        "http" => configure_http_embedder(engine, config, vector_mode),
        "fastembed" => configure_fastembed_embedder(engine, config, vector_mode),
        provider => Err(ShiroError::InvalidInput {
            message: format!("unknown embedding provider '{provider}'"),
        }),
    }
}

fn configure_http_embedder(
    mut engine: Engine,
    config: &EmbedConfig,
    vector_mode: VectorIndexMode,
) -> Result<Engine, ShiroError> {
    let base_url = required_config("embed.base_url", config.base_url.as_deref())?;
    let model = required_config("embed.model", config.model.as_deref())?;
    let dimensions = config.dimensions.unwrap_or(384);
    let embedder = HttpEmbedder::new(HttpEmbedderConfig {
        base_url: base_url.to_string(),
        model: model.to_string(),
        dimensions,
        api_key: config.api_key.clone(),
    });

    if matches!(vector_mode, VectorIndexMode::Strict) {
        let vector_index =
            open_vector_index(&engine.home, embedder.dimensions(), &embedder.fingerprint())?;
        engine = engine.with_embedder(Box::new(embedder));
        Ok(engine.with_vector_index(Box::new(vector_index)))
    } else {
        Ok(engine.with_embedder(Box::new(embedder)))
    }
}

fn configure_fastembed_embedder(
    mut engine: Engine,
    config: &EmbedConfig,
    vector_mode: VectorIndexMode,
) -> Result<Engine, ShiroError> {
    let model = config.model.as_deref().unwrap_or("AllMiniLML6V2");
    let cache_dir = config.cache_dir.as_ref().map(Utf8PathBuf::from);
    let embedder = FastEmbedEmbedder::try_new(FastEmbedEmbedderConfig {
        model: model.to_string(),
        cache_dir,
        show_download_progress: false,
    })?;

    if matches!(vector_mode, VectorIndexMode::Strict) {
        let vector_index =
            open_vector_index(&engine.home, embedder.dimensions(), &embedder.fingerprint())?;
        engine = engine.with_embedder(Box::new(embedder));
        Ok(engine.with_vector_index(Box::new(vector_index)))
    } else {
        Ok(engine.with_embedder(Box::new(embedder)))
    }
}

fn configure_reranker(mut engine: Engine, config: &RerankConfig) -> Result<Engine, ShiroError> {
    match config.provider.as_deref().unwrap_or("fastembed") {
        "fastembed" => {
            let model = config.model.as_deref().unwrap_or("BGERerankerBase");
            let reranker = FastEmbedReranker::try_new(FastEmbedRerankerConfig {
                model: model.to_string(),
                cache_dir: None,
                show_download_progress: false,
            })?;
            engine = engine.with_reranker(Box::new(reranker));
            Ok(engine)
        }
        provider => Err(ShiroError::InvalidInput {
            message: format!("unknown reranker provider '{provider}'"),
        }),
    }
}

fn open_vector_index(
    home: &ShiroHome,
    dimensions: usize,
    fingerprint: &shiro_core::EmbeddingFingerprint,
) -> Result<FlatIndex, ShiroError> {
    let data_path = home.vector_dir().join("flat.jsonl");
    match FlatIndex::open_compatible(dimensions, data_path.clone(), fingerprint) {
        Ok(index) => Ok(index),
        Err(ShiroError::FingerprintMismatch { .. }) => {
            // Preserve the incompatible index so the provider-agnostic SDK can
            // reject vector-capable operations while explicit BM25 still works.
            FlatIndex::open(dimensions, data_path)
        }
        Err(error) => Err(error),
    }
}

fn required_config<'a>(key: &str, value: Option<&'a str>) -> Result<&'a str, ShiroError> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ShiroError::Config {
            message: format!("{key} is required for the configured embedding provider"),
        })
}
