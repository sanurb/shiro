# ADR-014: Hybrid Retrieval Contract

**Status:** Accepted
**Date:** 2026-03-07

## Context

Shiro supports two retrieval signals: BM25 full-text search (via `shiro-index`, backed by Tantivy) and dense vector similarity (via `shiro-embed::FlatIndex`). Neither signal is universally superior — BM25 excels on keyword-heavy queries; vector search handles semantic paraphrase. A fusion layer is required to combine ranked lists from both sources into a single result set without requiring calibrated scores.

The fusion implementation lives in `shiro-sdk::fusion`. Results are persisted to the `search_results` table with columns `bm25_score`, `bm25_rank`, `fused_score`, `fts_gen`, `vec_gen`, and `query_digest`. Generation ids (`GenerationId(u64)`) track which index snapshot produced the result, enabling cache invalidation.

A vector index may be absent (embeddings not yet computed, or `shiro-embed::FlatIndex` not populated). BM25 is always expected to be available once a document reaches `DocState::Ready`.

## Decision

**Fusion algorithm:** Reciprocal Rank Fusion (RRF) is the canonical method. The formula is:

```
rrf(s) = Σ 1 / (k + rank_S(s))
```

where `k = 60` (constant `RRF_K` in `shiro-sdk::fusion`), summed over all sources `S` in which segment `s` appears. `k` is a tunable constant at the crate level — it is not configurable per-query.

**Fallback semantics:** If the vector index (`shiro-embed::FlatIndex`) is absent or empty, fusion degrades gracefully to BM25-only without returning an error; the BM25 rank list is fused alone. If the BM25 index (`shiro-index`) is absent or unavailable, the query returns an error — BM25 is a hard requirement.

**Score semantics:** `fused_score` values are ordinal (rank-derived). They MUST NOT be interpreted as calibrated probabilities or confidence measures by consumers. Callers comparing scores across queries or index generations are doing it wrong.

**Tie-break:** When two segments share an identical `fused_score`, ordering is by `segment_id` ascending (lexicographic on the `seg_` prefixed blake3 string). This is a stability contract: results MUST be deterministic across identical queries against identical index generations.

**Explain output:** Every retrieval response MUST expose an `explain` payload containing: per-source rank and raw score for each contributing source, the fusion formula applied (`rrf`, `k` value), which sources contributed (BM25, vector, or BM25-only), and the `GenerationId` of each index at query time (`fts_gen`, `vec_gen`). No source that contributed a rank MUST appear as unknown in the explain output.

**Cache invalidation:** `query_digest` stored in `search_results` is a hash of the query text and index generation ids. Consumers MAY use it to detect stale cached results when either the query or an index generation changes.

## Consequences

- Retrieval is always available once BM25 is populated — embeddings are an enhancement, not a prerequisite.
- Adding a new retrieval source (e.g., taxonomy-boosted ranking via `shiro-store` concept relations) requires extending the fusion loop in `shiro-sdk::fusion`, not replacing the algorithm.
- `explain` output is always complete — the set of contributing sources is fully enumerated, no partial or missing attribution.
- Consumers that cache `search_results` rows MUST revalidate against `query_digest` when index generations advance.
- Score values are not portable across queries; ranking comparisons are only meaningful within a single result set.

## Alternatives Considered

- **Linear combination of scores:** Requires that BM25 and vector scores occupy a shared, calibrated range. Neither Tantivy BM25 scores nor raw cosine similarities satisfy this without normalization that introduces additional tuning surface.
- **Learned fusion (LTR):** Requires labeled training data tied to this corpus and query distribution. Not available at v1; retraining cadence would couple retrieval quality to data collection infrastructure.
- **CombMNZ:** Scores improve with the number of sources that retrieve a segment. Less robust when one source is absent — the missing source penalizes segments it would have ranked highly, distorting results rather than gracefully degrading.

## Non-Goals

- Per-query `k` tuning is not implemented. `RRF_K` is a compile-time/crate-level constant.
- Query-dependent source weighting (e.g., boosting vector for semantic queries, boosting BM25 for code) is not in scope for v1.
- Pluggable fusion strategies (swapping RRF for another algorithm at runtime) are not supported in v1.
- Score calibration or normalization to produce probability-like outputs is explicitly out of scope.
