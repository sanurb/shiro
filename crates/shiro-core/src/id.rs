//! Content-addressed identifiers for documents, segments, and runs.
//!
//! Format per `docs/MCP.md`: `doc_<blake3_hex>`, `seg_<blake3_hex>`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Content-addressed document identifier (`doc_<blake3_hex>`).
///
/// Two documents with identical content always produce the same `DocId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocId(String);

impl DocId {
    /// Derive a `DocId` from the raw content bytes.
    pub fn from_content(content: &[u8]) -> Self {
        let hex = blake3::hash(content).to_hex();
        Self(format!("doc_{hex}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Reconstruct from a stored string (e.g. from SQLite).
    /// Validates the `doc_` prefix.
    pub fn from_stored(s: impl Into<String>) -> Result<Self, &'static str> {
        let s = s.into();
        if s.starts_with("doc_") {
            Ok(Self(s))
        } else {
            Err("DocId must start with 'doc_'")
        }
    }
}

impl fmt::Display for DocId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Deterministic segment identifier (`seg_<blake3_hex>`).
///
/// Derived from parent `DocId` and positional index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentId(String);

impl SegmentId {
    /// Create a segment id from the parent [`DocId`] and a zero-based index.
    pub fn new(doc_id: &DocId, index: usize) -> Self {
        let input = format!("{}:{index}", doc_id);
        let hex = blake3::hash(input.as_bytes()).to_hex();
        Self(format!("seg_{hex}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Reconstruct from a stored string.
    pub fn from_stored(s: impl Into<String>) -> Result<Self, &'static str> {
        let s = s.into();
        if s.starts_with("seg_") {
            Ok(Self(s))
        } else {
            Err("SegmentId must start with 'seg_'")
        }
    }
}

impl fmt::Display for SegmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque identifier for a processing run.
///
/// Not content-addressed — callers provide uniqueness (e.g. timestamp-based).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(String);

impl RunId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a run ID from the current timestamp (monotonic within process).
    pub fn generate() -> Self {
        use std::time::SystemTime;
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        Self(format!("run_{}.{}", ts.as_secs(), ts.subsec_nanos()))
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
    fn doc_id_has_prefix() {
        let id = DocId::from_content(b"test");
        assert!(id.as_str().starts_with("doc_"));
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
    fn segment_id_has_prefix() {
        let doc = DocId::from_content(b"test");
        let id = SegmentId::new(&doc, 0);
        assert!(id.as_str().starts_with("seg_"));
    }

    #[test]
    fn segment_id_differs_by_index() {
        let doc = DocId::from_content(b"test");
        let a = SegmentId::new(&doc, 0);
        let b = SegmentId::new(&doc, 1);
        assert_ne!(a, b);
    }

    #[test]
    fn from_stored_validates_prefix() {
        assert!(DocId::from_stored("doc_abc123").is_ok());
        assert!(DocId::from_stored("bad_abc123").is_err());
        assert!(SegmentId::from_stored("seg_abc123").is_ok());
        assert!(SegmentId::from_stored("bad_abc123").is_err());
    }

    #[test]
    fn doc_id_json_roundtrip() {
        let id = DocId::from_content(b"hello");
        let json = serde_json::to_string(&id).unwrap();
        let back: DocId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn run_id_roundtrip() {
        let id = RunId::new("run-001");
        assert_eq!(id.as_str(), "run-001");
        assert_eq!(id.to_string(), "run-001");
    }

    #[test]
    fn run_id_generate_has_prefix() {
        let id = RunId::generate();
        assert!(id.as_str().starts_with("run_"));
    }
}
