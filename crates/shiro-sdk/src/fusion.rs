//! Reciprocal Rank Fusion (RRF) for merging ranked result lists.

use std::collections::BTreeMap;

/// RRF parameter k (standard default: 60).
pub const RRF_K: f64 = 60.0;

/// A scored result from a single retrieval source.
#[derive(Debug, Clone)]
pub struct RankedHit {
    pub id: String,
    pub bm25_rank: Option<usize>,
    pub vector_rank: Option<usize>,
}

/// Fused result with combined score.
#[derive(Debug, Clone)]
pub struct FusedHit {
    pub id: String,
    pub rrf_score: f64,
    pub bm25_rank: Option<usize>,
    pub vector_rank: Option<usize>,
}

/// Merge ranked lists using RRF.
///
/// `rrf_score(d) = sum over S of 1 / (k + rank_S(d))`
///
/// Deterministic tie-breaking: when scores are equal, sort by ID (ascending).
pub fn reciprocal_rank_fusion(hits: &[RankedHit]) -> Vec<FusedHit> {
    let mut scores: BTreeMap<&str, (f64, Option<usize>, Option<usize>)> = BTreeMap::new();

    for hit in hits {
        let entry = scores.entry(&hit.id).or_insert((0.0, None, None));
        if let Some(rank) = hit.bm25_rank {
            entry.0 += 1.0 / (RRF_K + rank as f64);
            entry.1 = Some(rank);
        }
        if let Some(rank) = hit.vector_rank {
            entry.0 += 1.0 / (RRF_K + rank as f64);
            entry.2 = Some(rank);
        }
    }

    let mut fused: Vec<FusedHit> = scores
        .into_iter()
        .map(|(id, (score, bm25_rank, vector_rank))| FusedHit {
            id: id.to_string(),
            rrf_score: score,
            bm25_rank,
            vector_rank,
        })
        .collect();

    // Sort by descending score, then ascending ID for deterministic tie-breaking.
    fused.sort_by(|a, b| {
        b.rrf_score
            .partial_cmp(&a.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bm25_only() {
        let hits = vec![
            RankedHit {
                id: "a".into(),
                bm25_rank: Some(1),
                vector_rank: None,
            },
            RankedHit {
                id: "b".into(),
                bm25_rank: Some(2),
                vector_rank: None,
            },
            RankedHit {
                id: "c".into(),
                bm25_rank: Some(3),
                vector_rank: None,
            },
        ];
        let fused = reciprocal_rank_fusion(&hits);
        assert_eq!(fused.len(), 3);
        assert_eq!(fused[0].id, "a");
        assert!(fused[0].rrf_score > fused[1].rrf_score);
        assert!(fused[1].rrf_score > fused[2].rrf_score);
    }

    #[test]
    fn deterministic_tie_breaking() {
        let hits = vec![
            RankedHit {
                id: "b".into(),
                bm25_rank: Some(1),
                vector_rank: None,
            },
            RankedHit {
                id: "a".into(),
                bm25_rank: Some(1),
                vector_rank: None,
            },
        ];
        let fused = reciprocal_rank_fusion(&hits);
        assert_eq!(fused[0].id, "a");
        assert_eq!(fused[1].id, "b");
    }

    #[test]
    fn hybrid() {
        let hits = vec![
            RankedHit {
                id: "a".into(),
                bm25_rank: Some(1),
                vector_rank: Some(5),
            },
            RankedHit {
                id: "b".into(),
                bm25_rank: Some(5),
                vector_rank: Some(1),
            },
        ];
        let fused = reciprocal_rank_fusion(&hits);
        assert_eq!(fused.len(), 2);
        assert!((fused[0].rrf_score - fused[1].rrf_score).abs() < 1e-10);
    }

    #[test]
    fn empty() {
        assert!(reciprocal_rank_fusion(&[]).is_empty());
    }
}
