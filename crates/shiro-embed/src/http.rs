//! HTTP-based embedder for OpenAI-compatible embedding APIs.
//!
//! Works with: ollama, llama.cpp server, text-embeddings-inference, vLLM,
//! or any server implementing POST /v1/embeddings with the OpenAI schema.

use shiro_core::ports::{Embedder, EmbeddingMeta};
use shiro_core::ShiroError;

/// Configuration for the HTTP embedder.
pub struct HttpEmbedderConfig {
    /// Base URL of the embedding service (e.g., `"http://localhost:11434/v1"`).
    pub base_url: String,
    /// Model name to request (e.g., `"all-minilm"` or `"nomic-embed-text"`).
    pub model: String,
    /// Expected embedding dimensions (validated on first response).
    pub dimensions: usize,
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<String>,
}

pub struct HttpEmbedder {
    config: HttpEmbedderConfig,
}

impl HttpEmbedder {
    pub fn new(config: HttpEmbedderConfig) -> Self {
        Self { config }
    }

    fn post_embeddings(&self, input: Vec<String>) -> Result<Vec<Vec<f32>>, ShiroError> {
        let url = format!("{}/embeddings", self.config.base_url.trim_end_matches('/'));

        let mut req = ureq::post(&url).set("Content-Type", "application/json");
        if let Some(ref key) = self.config.api_key {
            req = req.set("Authorization", &format!("Bearer {key}"));
        }

        let body = ureq::json!({
            "model": self.config.model,
            "input": input,
        });

        let resp = req.send_json(body).map_err(|e| ShiroError::InvalidInput {
            message: format!("embedding HTTP request failed: {e}"),
        })?;

        let resp_body: serde_json::Value =
            resp.into_json().map_err(|e| ShiroError::InvalidInput {
                message: format!("invalid JSON from embedding endpoint: {e}"),
            })?;

        // Parse OpenAI embedding response: { data: [{ embedding: [...], index: N }] }
        let data = resp_body["data"]
            .as_array()
            .ok_or_else(|| ShiroError::InvalidInput {
                message: "embedding response missing 'data' array".to_string(),
            })?;

        let mut embeddings: Vec<(usize, Vec<f32>)> = Vec::with_capacity(data.len());
        for item in data {
            let index = item["index"].as_u64().unwrap_or(0) as usize;
            let embedding = item["embedding"]
                .as_array()
                .ok_or_else(|| ShiroError::InvalidInput {
                    message: "embedding item missing 'embedding' array".to_string(),
                })?
                .iter()
                .map(|v: &serde_json::Value| v.as_f64().unwrap_or(0.0) as f32)
                .collect::<Vec<f32>>();

            if embedding.len() != self.config.dimensions {
                return Err(ShiroError::InvalidInput {
                    message: format!(
                        "embedding dimension mismatch: expected {}, got {}",
                        self.config.dimensions,
                        embedding.len()
                    ),
                });
            }
            embeddings.push((index, embedding));
        }

        // Sort by index to maintain input order.
        embeddings.sort_by_key(|(i, _)| *i);
        Ok(embeddings.into_iter().map(|(_, e)| e).collect())
    }
}

impl Embedder for HttpEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, ShiroError> {
        let results = self.post_embeddings(vec![text.to_string()])?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| ShiroError::InvalidInput {
                message: "empty embedding response".to_string(),
            })
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, ShiroError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let input: Vec<String> = texts.iter().map(|t| (*t).to_string()).collect();
        self.post_embeddings(input)
    }

    fn dimensions(&self) -> usize {
        self.config.dimensions
    }

    fn meta(&self) -> EmbeddingMeta {
        EmbeddingMeta {
            dimensions: self.config.dimensions,
            model_name: self.config.model.clone(),
            provider: "http".to_string(),
        }
    }
}
