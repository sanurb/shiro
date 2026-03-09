use std::sync::Mutex;

use camino::Utf8PathBuf;
use fastembed::{RerankInitOptions, RerankerModel, TextRerank};
use shiro_core::error::ShiroError;
use shiro_core::ports::{RerankResult, Reranker};

/// Configuration for constructing a [`FastEmbedReranker`].
#[derive(Debug, Clone)]
pub struct FastEmbedRerankerConfig {
    /// Reranker model to use. Defaults to `BGERerankerBase`.
    pub model: String,
    /// Directory to cache downloaded models. `None` uses the fastembed default.
    pub cache_dir: Option<Utf8PathBuf>,
    /// Show download progress on stderr.
    pub show_download_progress: bool,
}

impl Default for FastEmbedRerankerConfig {
    fn default() -> Self {
        Self {
            model: "BGERerankerBase".to_string(),
            cache_dir: None,
            show_download_progress: false,
        }
    }
}

/// Local reranking adapter backed by FastEmbed (ONNX Runtime).
///
/// Thread-safe via internal `Mutex`. Cross-encoder models are loaded once at
/// construction time; inference calls are serialized through the lock.
pub struct FastEmbedReranker {
    inner: Mutex<TextRerank>,
    model_name: String,
}

impl FastEmbedReranker {
    /// Create a new FastEmbed reranker. Downloads the model on first use.
    pub fn try_new(config: FastEmbedRerankerConfig) -> Result<Self, ShiroError> {
        let parsed_model = parse_reranker_model(&config.model)?;
        let model_name = config.model.clone();

        let mut opts = RerankInitOptions::new(parsed_model)
            .with_show_download_progress(config.show_download_progress);
        if let Some(dir) = config.cache_dir {
            opts = opts.with_cache_dir(dir.into_std_path_buf());
        }

        let reranker = TextRerank::try_new(opts).map_err(|e| ShiroError::RerankFail {
            message: format!("FastEmbed reranker init failed: {e}"),
        })?;

        Ok(Self {
            inner: Mutex::new(reranker),
            model_name,
        })
    }
}

impl Reranker for FastEmbedReranker {
    fn rerank(
        &self,
        query: &str,
        documents: &[&str],
        top_n: usize,
    ) -> Result<Vec<RerankResult>, ShiroError> {
        let guard = self.inner.lock().map_err(|e| ShiroError::RerankFail {
            message: format!("lock poisoned: {e}"),
        })?;

        let results = guard
            .rerank(query, documents.to_vec(), true, Some(top_n))
            .map_err(|e| ShiroError::RerankFail {
                message: format!("FastEmbed rerank failed: {e}"),
            })?;

        Ok(results
            .into_iter()
            .map(|r| RerankResult {
                index: r.index,
                score: r.score,
            })
            .collect())
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_sane() {
        let cfg = FastEmbedRerankerConfig::default();
        assert!(cfg.cache_dir.is_none());
        assert!(!cfg.show_download_progress);
    }

    #[test]
    fn unknown_reranker_model_is_hard_error() {
        let cfg = FastEmbedRerankerConfig {
            model: "NonExistentModel".to_string(),
            cache_dir: None,
            show_download_progress: false,
        };
        let err = FastEmbedReranker::try_new(cfg)
            .err()
            .expect("expected error");
        assert!(matches!(
            err,
            shiro_core::error::ShiroError::InvalidInput { .. }
        ));
    }

    #[test]
    fn empty_reranker_model_is_error() {
        let cfg = FastEmbedRerankerConfig {
            model: "".to_string(),
            ..Default::default()
        };
        assert!(FastEmbedReranker::try_new(cfg).is_err());
    }
}

fn parse_reranker_model(name: &str) -> Result<RerankerModel, ShiroError> {
    match name {
        "BGERerankerBase" | "bge-reranker-base" => Ok(RerankerModel::BGERerankerBase),
        "BGERerankerV2M3" | "bge-reranker-v2-m3" => Ok(RerankerModel::BGERerankerV2M3),
        _ => Err(ShiroError::InvalidInput {
            message: format!("unknown reranker model: {name}"),
        }),
    }
}
