//! Byte span within a document.
//!
//! Invariant: `start <= end`. Enforced by the constructor.

use serde::{Deserialize, Serialize};

/// A half-open byte range `[start, end)` within a document.
///
/// Constructed via [`Span::new`] which rejects `start > end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    start: usize,
    end: usize,
}

/// Error returned when `start > end`.
#[derive(Debug, thiserror::Error)]
#[error("invalid span: start ({start}) must be <= end ({end})")]
pub struct InvalidSpan {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// Create a new span. Fails if `start > end`.
    pub fn new(start: usize, end: usize) -> Result<Self, InvalidSpan> {
        if start > end {
            return Err(InvalidSpan { start, end });
        }
        Ok(Self { start, end })
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }

    /// Byte length of this span.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// True if `self` fully contains `other` (inclusive on both sides).
    pub fn contains(&self, other: &Span) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// True if `self` and `other` share at least one byte.
    ///
    /// Adjacent spans (`[0,5)` and `[5,10)`) do **not** overlap.
    pub fn overlaps(&self, other: &Span) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_span() {
        let s = Span::new(0, 10).unwrap();
        assert_eq!(s.start(), 0);
        assert_eq!(s.end(), 10);
        assert_eq!(s.len(), 10);
        assert!(!s.is_empty());
    }

    #[test]
    fn empty_span() {
        let s = Span::new(5, 5).unwrap();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn invalid_span_rejected() {
        let err = Span::new(10, 0).unwrap_err();
        assert_eq!(err.start, 10);
        assert_eq!(err.end, 0);
    }

    #[test]
    fn contains_inner() {
        let outer = Span::new(0, 10).unwrap();
        let inner = Span::new(2, 5).unwrap();
        assert!(outer.contains(&inner));
        assert!(!inner.contains(&outer));
    }

    #[test]
    fn contains_self() {
        let s = Span::new(0, 10).unwrap();
        assert!(s.contains(&s));
    }

    #[test]
    fn overlaps_partial() {
        let a = Span::new(0, 5).unwrap();
        let b = Span::new(3, 10).unwrap();
        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
    }

    #[test]
    fn adjacent_spans_do_not_overlap() {
        let a = Span::new(0, 5).unwrap();
        let b = Span::new(5, 10).unwrap();
        assert!(!a.overlaps(&b));
        assert!(!b.overlaps(&a));
    }

    #[test]
    fn disjoint_spans() {
        let a = Span::new(0, 3).unwrap();
        let b = Span::new(7, 10).unwrap();
        assert!(!a.overlaps(&b));
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn len_equals_end_minus_start(start in 0usize..10_000, delta in 0usize..10_000) {
                let end = start + delta;
                let s = Span::new(start, end).unwrap();
                prop_assert_eq!(s.len(), end - start);
            }

            #[test]
            fn contains_implies_overlaps(
                a_start in 0usize..1_000,
                a_delta in 1usize..1_000,
                b_off in 0usize..500,
                b_delta in 1usize..500,
            ) {
                let a_end = a_start + a_delta;
                let b_start = a_start + (b_off % a_delta);
                let b_end = (b_start + b_delta).min(a_end);
                if b_start < b_end {
                    let a = Span::new(a_start, a_end).unwrap();
                    let b = Span::new(b_start, b_end).unwrap();
                    if a.contains(&b) {
                        prop_assert!(a.overlaps(&b));
                    }
                }
            }

            #[test]
            fn reversed_args_rejected(start in 1usize..10_000, delta in 1usize..10_000) {
                let end = start.saturating_sub(delta);
                if start > end {
                    prop_assert!(Span::new(start, end).is_err());
                }
            }
        }
    }
}
