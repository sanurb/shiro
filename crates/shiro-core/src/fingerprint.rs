//! Processing fingerprints for cache invalidation.
//!
//! A [`ProcessingFingerprint`] captures the exact parser and segmenter versions
//! used to process a document, enabling deterministic cache-busting when any
//! processing stage changes.

use serde::{Deserialize, Serialize};

/// Captures the parser and segmenter versions used to process a document.
///
/// The [`content_hash`](ProcessingFingerprint::content_hash) method produces a
/// stable blake3 digest of the fingerprint, suitable for comparing whether a
/// document needs reprocessing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingFingerprint {
    /// Name of the parser that produced the IR (e.g. `"markdown"`, `"pdf"`).
    pub parser_name: String,
    /// Monotonic version of the parser implementation.
    pub parser_version: u32,
    /// Monotonic version of the segmenter implementation.
    pub segmenter_version: u32,
}

impl ProcessingFingerprint {
    /// Create a new fingerprint.
    pub fn new(
        parser_name: impl Into<String>,
        parser_version: u32,
        segmenter_version: u32,
    ) -> Self {
        Self {
            parser_name: parser_name.into(),
            parser_version,
            segmenter_version,
        }
    }

    /// Blake3 hex digest of `"{parser_name}:{parser_version}:{segmenter_version}"`.
    ///
    /// Two fingerprints with identical fields always produce the same hash.
    pub fn content_hash(&self) -> String {
        let input = format!(
            "{}:{}:{}",
            self.parser_name, self.parser_version, self.segmenter_version
        );
        blake3::hash(input.as_bytes()).to_hex().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_deterministic() {
        let a = ProcessingFingerprint::new("markdown", 1, 2);
        let b = ProcessingFingerprint::new("markdown", 1, 2);
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn content_hash_changes_on_version_bump() {
        let a = ProcessingFingerprint::new("markdown", 1, 2);
        let b = ProcessingFingerprint::new("markdown", 2, 2);
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn content_hash_changes_on_segmenter_bump() {
        let a = ProcessingFingerprint::new("markdown", 1, 1);
        let b = ProcessingFingerprint::new("markdown", 1, 2);
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn empty_parser_name_is_valid() {
        let fp = ProcessingFingerprint::new("", 0, 0);
        let hash = fp.content_hash();
        assert!(!hash.is_empty());
    }

    #[test]
    fn json_roundtrip() {
        let fp = ProcessingFingerprint::new("pdf", 3, 5);
        let json = serde_json::to_string(&fp).unwrap();
        let back: ProcessingFingerprint = serde_json::from_str(&json).unwrap();
        assert_eq!(fp.content_hash(), back.content_hash());
    }
    #[test]
    fn golden_content_hash() {
        let fp = ProcessingFingerprint::new("markdown", 1, 1);
        assert_eq!(
            fp.content_hash(),
            "2f9e2aafab6fb55ea9b2bd1b05dc0e43f405545f2c7d3f7c39907bfc6ef7f951",
            "ProcessingFingerprint golden hash changed — this is a breaking change to cache invalidation"
        );
    }
}
