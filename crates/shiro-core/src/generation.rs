//! Index generation tracking.
//!
//! Generations provide monotonic versioning for the search index, enabling
//! atomic swaps and garbage collection of stale data.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Monotonic generation counter for index snapshots.
///
/// Wraps a `u64` and provides saturating increment to avoid overflow panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GenerationId(u64);

impl GenerationId {
    /// The zero generation — used as a sentinel before any indexing has occurred.
    pub const ZERO: Self = Self(0);

    /// Create a generation from a raw counter value.
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the next generation, saturating at `u64::MAX`.
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// Unwrap the inner counter.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for GenerationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Snapshot metadata for a single index generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexGeneration {
    /// Which generation this snapshot represents.
    pub gen_id: GenerationId,
    /// ISO 8601 timestamp of when this generation was created.
    pub created_at: String,
    /// Number of documents included in this generation.
    pub doc_count: usize,
    /// Number of segments included in this generation.
    pub segment_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_zero() {
        assert_eq!(GenerationId::ZERO.as_u64(), 0);
    }

    #[test]
    fn next_increments() {
        let g = GenerationId::new(5);
        assert_eq!(g.next().as_u64(), 6);
    }

    #[test]
    fn next_saturates_at_max() {
        let g = GenerationId::new(u64::MAX);
        assert_eq!(g.next().as_u64(), u64::MAX);
    }

    #[test]
    fn ordering() {
        let a = GenerationId::new(1);
        let b = GenerationId::new(2);
        assert!(a < b);
    }

    #[test]
    fn display() {
        assert_eq!(GenerationId::new(42).to_string(), "42");
    }

    #[test]
    fn json_roundtrip() {
        let g = GenerationId::new(99);
        let json = serde_json::to_string(&g).unwrap();
        let back: GenerationId = serde_json::from_str(&json).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn index_generation_roundtrip() {
        let ig = IndexGeneration {
            gen_id: GenerationId::new(1),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            doc_count: 10,
            segment_count: 50,
        };
        let json = serde_json::to_string(&ig).unwrap();
        let back: IndexGeneration = serde_json::from_str(&json).unwrap();
        assert_eq!(ig.gen_id, back.gen_id);
        assert_eq!(ig.doc_count, back.doc_count);
    }
}
