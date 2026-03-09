use std::sync::Mutex;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use shiro_core::error::ShiroError;
use shiro_core::fingerprint::EmbeddingFingerprint;
use shiro_core::ports::{Embedder, EmbeddingMeta};

/// Configuration for constructing a [`FastEmbedEmbedder`].
#[derive(Debug, Clone)]
pub struct FastEmbedEmbedderConfig {
    /// Model to use. Defaults to `AllMiniLML6V2`.
    pub model: EmbeddingModel,
    /// Directory to cache downloaded models. `None` uses the fastembed default.
    pub cache_dir: Option<std::path::PathBuf>,
    /// Show download progress on stderr.
    pub show_download_progress: bool,
}

impl Default for FastEmbedEmbedderConfig {
    fn default() -> Self {
        Self {
            model: EmbeddingModel::AllMiniLML6V2,
            cache_dir: None,
            show_download_progress: false,
        }
    }
}

/// Local embedding adapter backed by FastEmbed (ONNX Runtime).
///
/// Thread-safe via internal `Mutex`. The ONNX runtime session is `Send` but
/// not necessarily `Sync`, so the mutex guarantees safe concurrent access.
pub struct FastEmbedEmbedder {
    inner: Mutex<TextEmbedding>,
    model_name: String,
    dims: usize,
    fingerprint: EmbeddingFingerprint,
}

impl FastEmbedEmbedder {
    /// Create a new FastEmbed embedder. Downloads the model on first use.
    pub fn try_new(config: FastEmbedEmbedderConfig) -> Result<Self, ShiroError> {
        let model_name = format!("{:?}", config.model);
        let dims = model_dimensions(&config.model);

        let mut opts = InitOptions::new(config.model)
            .with_show_download_progress(config.show_download_progress);
        if let Some(dir) = config.cache_dir {
            opts = opts.with_cache_dir(dir);
        }

        let embedding = TextEmbedding::try_new(opts).map_err(|e| ShiroError::EmbedFail {
            message: format!("FastEmbed init failed: {e}"),
        })?;

        let fingerprint = EmbeddingFingerprint::new(
            "fastembed".to_string(),
            model_name.clone(),
            dims,
            "l2".to_string(),
            "model_default".to_string(),
            "full_segment".to_string(),
        );

        Ok(Self {
            inner: Mutex::new(embedding),
            model_name,
            dims,
            fingerprint,
        })
    }

    /// Return the embedding fingerprint for this configuration.
    pub fn fingerprint(&self) -> &EmbeddingFingerprint {
        &self.fingerprint
    }
}

impl Embedder for FastEmbedEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, ShiroError> {
        let guard = self.inner.lock().map_err(|e| ShiroError::EmbedFail {
            message: format!("lock poisoned: {e}"),
        })?;
        let results = guard
            .embed(vec![text], None)
            .map_err(|e| ShiroError::EmbedFail {
                message: format!("FastEmbed embed failed: {e}"),
            })?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| ShiroError::EmbedFail {
                message: "FastEmbed returned empty result".to_string(),
            })
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, ShiroError> {
        let guard = self.inner.lock().map_err(|e| ShiroError::EmbedFail {
            message: format!("lock poisoned: {e}"),
        })?;
        guard
            .embed(texts.to_vec(), None)
            .map_err(|e| ShiroError::EmbedFail {
                message: format!("FastEmbed batch embed failed: {e}"),
            })
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn meta(&self) -> EmbeddingMeta {
        EmbeddingMeta {
            provider: "fastembed".to_string(),
            model_name: self.model_name.clone(),
            dimensions: self.dims,
        }
    }
}

/// Map known [`EmbeddingModel`] variants to their output dimension count.
///
/// Uses a wildcard fallback for forward-compatibility with new fastembed variants.
fn model_dimensions(model: &EmbeddingModel) -> usize {
    match model {
        EmbeddingModel::AllMiniLML6V2 => 384,
        EmbeddingModel::AllMiniLML6V2Q => 384,
        EmbeddingModel::AllMiniLML12V2 => 384,
        EmbeddingModel::AllMiniLML12V2Q => 384,
        EmbeddingModel::BGEBaseENV15 => 768,
        EmbeddingModel::BGEBaseENV15Q => 768,
        EmbeddingModel::BGESmallENV15 => 384,
        EmbeddingModel::BGESmallENV15Q => 384,
        EmbeddingModel::BGELargeENV15 => 1024,
        EmbeddingModel::BGELargeENV15Q => 1024,
        EmbeddingModel::NomicEmbedTextV1 => 768,
        EmbeddingModel::NomicEmbedTextV15 => 768,
        EmbeddingModel::NomicEmbedTextV15Q => 768,
        EmbeddingModel::ParaphraseMLMiniLML12V2 => 384,
        EmbeddingModel::ParaphraseMLMiniLML12V2Q => 384,
        EmbeddingModel::ParaphraseMLMpnetBaseV2 => 768,
        EmbeddingModel::MxbaiEmbedLargeV1 => 1024,
        EmbeddingModel::MxbaiEmbedLargeV1Q => 1024,
        EmbeddingModel::GTEBaseENV15 => 768,
        EmbeddingModel::GTEBaseENV15Q => 768,
        EmbeddingModel::GTELargeENV15 => 1024,
        EmbeddingModel::GTELargeENV15Q => 1024,
        // Forward-compatible fallback for new models added to fastembed.
        // Dimension validation downstream will catch genuine mismatches.
        _ => 384,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_sane() {
        let cfg = FastEmbedEmbedderConfig::default();
        assert!(cfg.cache_dir.is_none());
        assert!(!cfg.show_download_progress);
        // Default model is AllMiniLML6V2
        assert_eq!(model_dimensions(&cfg.model), 384);
    }

    #[test]
    fn model_dimensions_known_models() {
        assert_eq!(model_dimensions(&EmbeddingModel::AllMiniLML6V2), 384);
        assert_eq!(model_dimensions(&EmbeddingModel::BGEBaseENV15), 768);
        assert_eq!(model_dimensions(&EmbeddingModel::BGELargeENV15), 1024);
        assert_eq!(model_dimensions(&EmbeddingModel::NomicEmbedTextV1), 768);
        assert_eq!(model_dimensions(&EmbeddingModel::MxbaiEmbedLargeV1), 1024);
        assert_eq!(model_dimensions(&EmbeddingModel::GTELargeENV15), 1024);
        assert_eq!(
            model_dimensions(&EmbeddingModel::ParaphraseMLMpnetBaseV2),
            768
        );
    }

    #[test]
    fn model_dimensions_quantized_matches_non_quantized() {
        assert_eq!(
            model_dimensions(&EmbeddingModel::AllMiniLML6V2),
            model_dimensions(&EmbeddingModel::AllMiniLML6V2Q),
        );
        assert_eq!(
            model_dimensions(&EmbeddingModel::BGEBaseENV15),
            model_dimensions(&EmbeddingModel::BGEBaseENV15Q),
        );
        assert_eq!(
            model_dimensions(&EmbeddingModel::BGELargeENV15),
            model_dimensions(&EmbeddingModel::BGELargeENV15Q),
        );
    }
}
