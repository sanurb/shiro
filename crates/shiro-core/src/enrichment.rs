//! Enrichment results produced by external providers (LLM, NLP, etc.).
//!
//! An [`EnrichmentResult`] ties provider-generated metadata (title, summary,
//! tags, concepts) back to its source document via [`DocId`] and records a
//! content hash so stale enrichments can be detected.

use serde::{Deserialize, Serialize};

use crate::id::DocId;
use crate::taxonomy::ConceptId;

/// Result of enriching a single document through an external provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentResult {
    /// The document that was enriched.
    pub doc_id: DocId,
    /// Provider-generated title, if any.
    pub title: Option<String>,
    /// Provider-generated summary, if any.
    pub summary: Option<String>,
    /// Free-form tags assigned by the provider.
    pub tags: Vec<String>,
    /// Taxonomy concepts assigned by the provider.
    pub concepts: Vec<ConceptId>,
    /// Name of the enrichment provider (e.g. `"openai-gpt4"`).
    pub provider: String,
    /// Blake3 hex digest of the document content at the time of enrichment.
    ///
    /// Compare against the current content hash to detect stale enrichments.
    pub content_hash: String,
    /// ISO 8601 timestamp of when enrichment was performed.
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_enrichment() -> EnrichmentResult {
        EnrichmentResult {
            doc_id: DocId::from_content(b"hello world"),
            title: Some("Hello World".to_string()),
            summary: Some("A greeting".to_string()),
            tags: vec!["greeting".to_string(), "example".to_string()],
            concepts: vec![ConceptId::new("http://example.org", "Greeting")],
            provider: "test-provider".to_string(),
            content_hash: blake3::hash(b"hello world").to_hex().to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn json_roundtrip() {
        let e = sample_enrichment();
        let json = serde_json::to_string(&e).unwrap();
        let back: EnrichmentResult = serde_json::from_str(&json).unwrap();
        assert_eq!(e.doc_id.as_str(), back.doc_id.as_str());
        assert_eq!(e.tags, back.tags);
    }

    #[test]
    fn empty_optional_fields() {
        let e = EnrichmentResult {
            doc_id: DocId::from_content(b"test"),
            title: None,
            summary: None,
            tags: vec![],
            concepts: vec![],
            provider: "empty".to_string(),
            content_hash: String::new(),
            created_at: String::new(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: EnrichmentResult = serde_json::from_str(&json).unwrap();
        assert!(back.title.is_none());
        assert!(back.concepts.is_empty());
    }

    #[test]
    fn multiple_concepts() {
        let e = EnrichmentResult {
            doc_id: DocId::from_content(b"multi"),
            title: None,
            summary: None,
            tags: vec![],
            concepts: vec![
                ConceptId::new("http://a.org", "Alpha"),
                ConceptId::new("http://b.org", "Beta"),
            ],
            provider: "test".to_string(),
            content_hash: String::new(),
            created_at: String::new(),
        };
        assert_eq!(e.concepts.len(), 2);
        assert_ne!(e.concepts[0], e.concepts[1]);
    }
}
