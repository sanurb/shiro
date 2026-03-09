//! `shiro search` — thin adapter over shiro-sdk search.
//!
//! Per ADR-007, output uses EntryPoint shape: block-level position
//! and context window. No segment identifiers in public output.

use std::collections::BTreeMap;

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::config::ShiroConfig;
use shiro_core::fingerprint::EmbeddingFingerprint;
use shiro_core::ports::{Embedder, VectorIndex};
use shiro_core::{ShiroError, ShiroHome};
use shiro_sdk::{Engine, SearchInput};

pub use shiro_sdk::SearchMode;

/// Build an Engine with optional embedder, vector index, and reranker
/// derived from the config file.
fn open_engine(home: &ShiroHome) -> Result<Engine, ShiroError> {
    let mut engine = Engine::open(home.clone())?;

    // Load config.
    let config_path = home.config_path();
    let config: ShiroConfig = if config_path.as_std_path().is_file() {
        let text = std::fs::read_to_string(config_path.as_std_path())?;
        toml::from_str(&text).map_err(|e| ShiroError::Config {
            message: format!("config parse error: {e}"),
        })?
    } else {
        ShiroConfig::default()
    };

    // Wire embedder + vector index if configured.
    if let Some(ref embed) = config.embed {
        let provider = embed.provider.as_deref().unwrap_or("http");
        match provider {
            "fastembed" => {
                let model_name = embed.model.as_deref().unwrap_or("AllMiniLML6V2");
                let cache_dir = embed.cache_dir.as_ref().map(camino::Utf8PathBuf::from);
                let fe_config = shiro_fastembed::FastEmbedEmbedderConfig {
                    model: model_name.to_string(),
                    cache_dir,
                    show_download_progress: false,
                };
                match shiro_fastembed::FastEmbedEmbedder::try_new(fe_config) {
                    Ok(embedder) => {
                        let fp = embedder.fingerprint();
                        let dims = embedder.dimensions();
                        engine = engine.with_embedder(Box::new(embedder));

                        // Open or create the flat vector index with fingerprint enforcement (ADR-012).
                        let vector_path = home.vector_dir().join("flat.jsonl");
                        match open_vector_index(dims, vector_path, &fp) {
                            Ok(index) => {
                                engine = engine.with_vector_index(Box::new(index));
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "vector index fingerprint mismatch or open failed");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to init FastEmbed embedder");
                    }
                }
            }
            "http" => {
                // HttpEmbedder requires base_url and model.
                if let (Some(base_url), Some(model)) =
                    (embed.base_url.as_deref(), embed.model.as_deref())
                {
                    let dims = embed.dimensions.unwrap_or(384);
                    let http_config = shiro_embed::HttpEmbedderConfig {
                        base_url: base_url.to_string(),
                        model: model.to_string(),
                        dimensions: dims,
                        api_key: embed.api_key.clone(),
                    };
                    let embedder = shiro_embed::HttpEmbedder::new(http_config);
                    let fp = embedder.fingerprint();
                    let dims = embedder.dimensions();
                    engine = engine.with_embedder(Box::new(embedder));

                    // Open or create the flat vector index with fingerprint enforcement (ADR-012).
                    let vector_path = home.vector_dir().join("flat.jsonl");
                    match open_vector_index(dims, vector_path, &fp) {
                        Ok(index) => {
                            engine = engine.with_vector_index(Box::new(index));
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "vector index fingerprint mismatch or open failed");
                        }
                    }
                }
            }
            other => {
                tracing::warn!(provider = other, "unknown embed provider in config");
            }
        }
    }

    // Wire reranker if configured.
    if let Some(ref rerank) = config.rerank {
        let provider = rerank.provider.as_deref().unwrap_or("fastembed");
        if provider == "fastembed" {
            let model_name = rerank.model.as_deref().unwrap_or("BGERerankerBase");
            let rr_config = shiro_fastembed::FastEmbedRerankerConfig {
                model: model_name.to_string(),
                cache_dir: None,
                show_download_progress: false,
            };
            match shiro_fastembed::FastEmbedReranker::try_new(rr_config) {
                Ok(reranker) => {
                    engine = engine.with_reranker(Box::new(reranker));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to init FastEmbed reranker");
                }
            }
        }
    }

    Ok(engine)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    home: &ShiroHome,
    query: &str,
    mode: SearchMode,
    limit: usize,
    expand: bool,
    max_blocks: usize,
    max_chars: usize,
    rerank: bool,
) -> Result<CmdOutput, ShiroError> {
    let engine = open_engine(home)?;

    let input = SearchInput {
        query: query.to_string(),
        mode,
        limit,
        expand,
        max_blocks,
        max_chars,
        rerank,
    };
    let output = engine.search(&input)?;

    // Convert SDK output to JSON envelope.
    let results: Vec<serde_json::Value> = output
        .hits
        .iter()
        .map(|h| {
            let context_window: Vec<serde_json::Value> = h
                .context_window
                .iter()
                .map(|cb| {
                    serde_json::json!({
                        "block_idx": cb.block_idx,
                        "kind": cb.kind,
                        "span": { "start": cb.span_start, "end": cb.span_end },
                        "text": cb.text,
                    })
                })
                .collect();

            let mut scores = serde_json::Map::new();
            if let Some(bm25_rank) = h.scores.bm25_rank {
                scores.insert(
                    "bm25".to_string(),
                    serde_json::json!({
                        "score": h.scores.bm25_score,
                        "rank": bm25_rank,
                    }),
                );
            }
            if let Some(vector_rank) = h.scores.vector_rank {
                scores.insert(
                    "vector".to_string(),
                    serde_json::json!({
                        "score": h.scores.vector_score,
                        "rank": vector_rank,
                    }),
                );
            }
            scores.insert(
                "fused".to_string(),
                serde_json::json!({
                    "score": h.scores.fused_score,
                    "rank": h.scores.fused_rank,
                }),
            );
            if let Some(reranker_rank) = h.scores.reranker_rank {
                scores.insert(
                    "reranker".to_string(),
                    serde_json::json!({
                        "score": h.scores.reranker_score,
                        "rank": reranker_rank,
                    }),
                );
            }

            serde_json::json!({
                "result_id": h.result_id,
                "doc_id": h.doc_id,
                "block_idx": h.block_idx,
                "block_kind": h.block_kind,
                "span": { "start": h.span_start, "end": h.span_end },
                "snippet": h.snippet,
                "scores": scores,
                "context_window": context_window,
            })
        })
        .collect();

    let result = serde_json::json!({
        "query": output.query,
        "mode": output.mode,
        "generations": { "fts": output.fts_generation },
        "retrieval_info": {
            "bm25_active": output.retrieval_info.bm25_active,
            "vector_active": output.retrieval_info.vector_active,
            "reranker_active": output.retrieval_info.reranker_active,
            "reranker_model": output.retrieval_info.reranker_model,
        },
        "results": results,
    });

    let mut next_actions = Vec::new();
    if let Some(first) = output.hits.first() {
        let mut params = BTreeMap::new();
        params.insert(
            "result_id".to_string(),
            ParamMeta {
                value: Some(serde_json::json!(first.result_id)),
                default: None,
                description: Some("Result ID from search".to_string()),
            },
        );
        next_actions.push(NextAction::with_params(
            "shiro explain <result_id>",
            "Explain why this result matched",
            params,
        ));
    }
    next_actions.push(NextAction::simple("shiro list", "List all documents"));

    Ok(CmdOutput {
        result,
        next_actions,
    })
}

/// Open a FlatIndex and enforce fingerprint consistency (ADR-012).
///
/// If the index has a stored fingerprint, verify it matches the active
/// embedder's fingerprint. A mismatch is a hard error — the index must
/// be rebuilt with `shiro reindex`. If no fingerprint is stored (legacy
/// data or fresh index), the active fingerprint is recorded.
fn open_vector_index(
    dims: usize,
    vector_path: camino::Utf8PathBuf,
    embedder_fp: &EmbeddingFingerprint,
) -> Result<shiro_embed::FlatIndex, ShiroError> {
    let index = shiro_embed::FlatIndex::open(dims, vector_path)?;

    // ADR-012: fingerprint mismatch is a hard error.
    if let Some(stored_fp) = index.stored_fingerprint() {
        if stored_fp.fingerprint_hash != embedder_fp.fingerprint_hash {
            return Err(ShiroError::FingerprintMismatch {
                message: format!(
                    "embedding model changed: stored={}/{}({}d), active={}/{}({}d). Run `shiro reindex` to rebuild.",
                    stored_fp.provider, stored_fp.model, stored_fp.dimensions,
                    embedder_fp.provider, embedder_fp.model, embedder_fp.dimensions,
                ),
            });
        }
    } else if index.count()? == 0 {
        // Fresh index — record the fingerprint.
        index.set_fingerprint(embedder_fp)?;
    } else {
        // Non-empty index without fingerprint — legacy data.
        // Record the fingerprint but warn.
        tracing::warn!(
            "vector index has {} entries but no fingerprint sidecar; assuming current config is correct",
            index.count().unwrap_or(0)
        );
        index.set_fingerprint(embedder_fp)?;
    }

    Ok(index)
}
