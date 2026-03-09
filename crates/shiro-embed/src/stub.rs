use shiro_core::ports::{Embedder, EmbeddingMeta};
use shiro_core::ShiroError;

/// A test-only embedder that always returns zero vectors.
pub struct StubEmbedder {
    dims: usize,
}

impl StubEmbedder {
    pub fn new(dims: usize) -> Self {
        Self { dims }
    }
}

impl Embedder for StubEmbedder {
    fn embed(&self, _text: &str) -> Result<Vec<f32>, ShiroError> {
        Ok(vec![0.0; self.dims])
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, ShiroError> {
        Ok(texts.iter().map(|_| vec![0.0; self.dims]).collect())
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn meta(&self) -> EmbeddingMeta {
        EmbeddingMeta {
            dimensions: self.dims,
            model_name: "stub".to_string(),
            provider: "stub".to_string(),
        }
    }
}

/// A test-only embedder that produces deterministic hash-based vectors.
///
/// Given the same text input, always returns the same vector. Different texts
/// produce different vectors. Useful for integration tests that need meaningful
/// cosine similarity without downloading real models.
pub struct DeterministicStubEmbedder {
    dims: usize,
}

impl DeterministicStubEmbedder {
    pub fn new(dims: usize) -> Self {
        Self { dims }
    }
}

impl Embedder for DeterministicStubEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, ShiroError> {
        let hash = blake3::hash(text.as_bytes());
        let hash_bytes = hash.as_bytes();
        let mut vec = Vec::with_capacity(self.dims);
        for i in 0..self.dims {
            // Cycle through hash bytes, convert to f32 in [-1, 1]
            let byte = hash_bytes[i % 32];
            vec.push((byte as f32 / 127.5) - 1.0);
        }
        // L2-normalize
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vec {
                *v /= norm;
            }
        }
        Ok(vec)
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn meta(&self) -> EmbeddingMeta {
        EmbeddingMeta {
            dimensions: self.dims,
            model_name: "deterministic-stub".to_string(),
            provider: "stub".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shiro_core::ports::Embedder;

    #[test]
    fn test_stub_dimensions() {
        let e = StubEmbedder::new(128);
        assert_eq!(e.dimensions(), 128);
    }

    #[test]
    fn test_stub_embed() {
        let e = StubEmbedder::new(64);
        let v = e.embed("anything").unwrap();
        assert_eq!(v.len(), 64);
        assert!(v.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn test_stub_batch() {
        let e = StubEmbedder::new(32);
        let vecs = e.embed_batch(&["a", "b", "c"]).unwrap();
        assert_eq!(vecs.len(), 3);
        for v in &vecs {
            assert_eq!(v.len(), 32);
            assert!(v.iter().all(|&x| x == 0.0));
        }
    }

    #[test]
    fn test_deterministic_same_input() {
        let e = DeterministicStubEmbedder::new(64);
        let v1 = e.embed("hello world").unwrap();
        let v2 = e.embed("hello world").unwrap();
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_deterministic_different_input() {
        let e = DeterministicStubEmbedder::new(64);
        let v1 = e.embed("hello").unwrap();
        let v2 = e.embed("world").unwrap();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_deterministic_normalized() {
        let e = DeterministicStubEmbedder::new(128);
        let v = e.embed("test input").unwrap();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "vector should be L2-normalized");
    }
}
