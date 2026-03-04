//! Content-addressed identifiers for documents, segments, and runs.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Content-addressed document identifier (blake3 hash of raw bytes).
///
/// Two documents with identical content always produce the same `DocId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocId(String);

impl DocId {
    /// Derive a `DocId` from the raw content bytes.
    pub fn from_content(content: &[u8]) -> Self {
        Self(blake3::hash(content).to_hex().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DocId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Deterministic segment identifier derived from its parent document and
/// positional index within that document.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentId(String);

impl SegmentId {
    /// Create a segment id from the parent [`DocId`] and a zero-based index.
    pub fn new(doc_id: &DocId, index: usize) -> Self {
        let input = format!("{}:{index}", doc_id);
        Self(blake3::hash(input.as_bytes()).to_hex().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SegmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque identifier for a processing run.
///
/// Not content-addressed — callers provide uniqueness (e.g. ULID, UUID).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(String);

impl RunId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_id_deterministic() {
        let a = DocId::from_content(b"hello world");
        let b = DocId::from_content(b"hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn doc_id_differs_for_different_content() {
        let a = DocId::from_content(b"hello");
        let b = DocId::from_content(b"world");
        assert_ne!(a, b);
    }

    #[test]
    fn segment_id_deterministic() {
        let doc = DocId::from_content(b"test");
        let a = SegmentId::new(&doc, 0);
        let b = SegmentId::new(&doc, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn segment_id_differs_by_index() {
        let doc = DocId::from_content(b"test");
        let a = SegmentId::new(&doc, 0);
        let b = SegmentId::new(&doc, 1);
        assert_ne!(a, b);
    }

    #[test]
    fn run_id_roundtrip() {
        let id = RunId::new("run-001");
        assert_eq!(id.as_str(), "run-001");
        assert_eq!(id.to_string(), "run-001");
    }

    #[test]
    fn doc_id_json_roundtrip() {
        let id = DocId::from_content(b"hello");
        let json = serde_json::to_string(&id).unwrap();
        let back: DocId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
