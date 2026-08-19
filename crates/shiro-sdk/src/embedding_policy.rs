//! Effective embedding identity for versioned retrieval text.

use shiro_core::EmbeddingFingerprint;

/// Bind a provider/model fingerprint to Shiro's retrieval-text preprocessing.
///
/// Changing title/heading/chunk derivation must invalidate document vectors even
/// when provider, model, dimensions, and provider-side preprocessing are stable.
pub fn retrieval_embedding_fingerprint(base: &EmbeddingFingerprint) -> EmbeddingFingerprint {
    EmbeddingFingerprint::new(
        base.provider.clone(),
        base.model.clone(),
        base.dimensions,
        base.normalization.clone(),
        base.truncation_policy.clone(),
        format!(
            "{}+shiro_retrieval_text_v{}",
            base.chunk_policy,
            shiro_parse::RETRIEVAL_TEXT_VERSION
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_fingerprint_records_retrieval_text_policy() {
        let base = EmbeddingFingerprint::new(
            "provider".to_string(),
            "model".to_string(),
            3,
            "none".to_string(),
            "none".to_string(),
            "segment".to_string(),
        );
        let effective = retrieval_embedding_fingerprint(&base);
        assert!(effective.chunk_policy.contains("shiro_retrieval_text_v1"));
        assert_ne!(effective.fingerprint_hash, base.fingerprint_hash);
    }
}
