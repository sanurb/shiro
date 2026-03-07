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
}
