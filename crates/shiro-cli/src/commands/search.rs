//! `shiro search` — thin adapter over shiro-sdk search.
//!
//! Per ADR-007, output uses EntryPoint shape: block-level position
//! and context window. No segment identifiers in public output.

use std::collections::BTreeMap;

use crate::envelope::{CmdOutput, NextAction, ParamMeta};
use shiro_core::config::ShiroConfig;
use shiro_core::ports::Embedder;
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
                let fe_model = parse_fastembed_model(model_name);
                let cache_dir = embed.cache_dir.as_ref().map(std::path::PathBuf::from);

                let fe_config = shiro_fastembed::FastEmbedEmbedderConfig {
                    model: fe_model,
                    cache_dir,
                    show_download_progress: false,
                };

                match shiro_fastembed::FastEmbedEmbedder::try_new(fe_config) {
                    Ok(embedder) => {
                        let dims = embedder.dimensions();
                        engine = engine.with_embedder(Box::new(embedder));

                        // Open or create the flat vector index.
                        let vector_path = home.vector_dir().join("flat.jsonl");
                        if let Ok(index) = shiro_embed::FlatIndex::open(dims, vector_path) {
                            engine = engine.with_vector_index(Box::new(index));
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
                    engine = engine.with_embedder(Box::new(embedder));

                    let vector_path = home.vector_dir().join("flat.jsonl");
                    if let Ok(index) = shiro_embed::FlatIndex::open(dims, vector_path) {
                        engine = engine.with_vector_index(Box::new(index));
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
            let rr_model = parse_reranker_model(model_name);
            let rr_config = shiro_fastembed::FastEmbedRerankerConfig {
                model: rr_model,
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

// ---------------------------------------------------------------------------
// Model name parsing helpers
// ---------------------------------------------------------------------------

/// Parse a fastembed embedding model name from config string.
/// Falls back to AllMiniLML6V2 for unknown names.
fn parse_fastembed_model(name: &str) -> shiro_fastembed::EmbeddingModel {
    use shiro_fastembed::EmbeddingModel;
    match name {
        "AllMiniLML6V2" | "all-minilm-l6-v2" => EmbeddingModel::AllMiniLML6V2,
        "AllMiniLML6V2Q" => EmbeddingModel::AllMiniLML6V2Q,
        "AllMiniLML12V2" | "all-minilm-l12-v2" => EmbeddingModel::AllMiniLML12V2,
        "BGEBaseENV15" | "bge-base-en-v1.5" => EmbeddingModel::BGEBaseENV15,
        "BGESmallENV15" | "bge-small-en-v1.5" => EmbeddingModel::BGESmallENV15,
        "BGELargeENV15" | "bge-large-en-v1.5" => EmbeddingModel::BGELargeENV15,
        "NomicEmbedTextV1" | "nomic-embed-text-v1" => EmbeddingModel::NomicEmbedTextV1,
        "NomicEmbedTextV15" | "nomic-embed-text-v1.5" => EmbeddingModel::NomicEmbedTextV15,
        "MxbaiEmbedLargeV1" | "mxbai-embed-large-v1" => EmbeddingModel::MxbaiEmbedLargeV1,
        _ => {
            tracing::warn!(
                name,
                "unknown fastembed model, falling back to AllMiniLML6V2"
            );
            EmbeddingModel::AllMiniLML6V2
        }
    }
}

/// Parse a fastembed reranker model name from config string.
fn parse_reranker_model(name: &str) -> shiro_fastembed::RerankerModel {
    use shiro_fastembed::RerankerModel;
    match name {
        "BGERerankerBase" | "bge-reranker-base" => RerankerModel::BGERerankerBase,
        "BGERerankerV2M3" | "bge-reranker-v2-m3" => RerankerModel::BGERerankerV2M3,
        _ => {
            tracing::warn!(
                name,
                "unknown reranker model, falling back to BGERerankerBase"
            );
            RerankerModel::BGERerankerBase
        }
    }
}
