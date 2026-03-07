# ADR-014: Hybrid Retrieval Contract

**Status:** Accepted
**Date:** 2026-03-07

## Context

Shiro supports two retrieval signals: BM25 full-text search and dense vector similarity. Neither signal is universally superior — BM25 excels on keyword-heavy queries; vector search handles semantic paraphrase. A fusion layer is required to combine ranked lists from both sources into a single result set without requiring calibrated scores.

A vector index may be absent (embeddings not yet computed or index not populated). BM25 is always expected to be available once a document is ready for retrieval.

## Decision

**Reciprocal Rank Fusion (RRF)** is the canonical fusion method. *Canonical* here means this is the authoritative algorithm from which all fused rankings are produced; no other fusion path exists.

The formula is:

```
rrf(s) = Σ 1 / (k + rank_S(s))
```

where `k = 60`, summed over all sources `S` in which segment `s` appears. `k` is a system-level constant — it is not configurable per-query.

**Ordinal scores.** Fused scores are *ordinal*: they reflect relative position in a ranked list, not magnitude of relevance. Consumers MUST NOT interpret fused scores as calibrated probabilities, confidence measures, or comparable values across different queries or index generations. Ranking comparisons are meaningful only within a single result set from a single query.

**Fallback semantics.** BM25 is a hard requirement. If BM25 is unavailable, the query fails with an error. If the vector index is absent or empty, fusion degrades gracefully to BM25-only ranking without returning an error — the BM25 rank list is fused alone (each segment's RRF score is computed from a single source).

**Tie-break rule.** When two segments share an identical fused score, ordering is by segment identifier ascending (lexicographic). This is a stability contract: results MUST be *deterministic* (identical input query against identical index state produces identical output ordering) across invocations.

**Explain output.** Every retrieval response MUST expose an explain payload containing: per-source rank and raw score for each contributing source, the fusion formula applied (RRF, k value), which sources contributed (BM25, vector, or BM25-only), and the generation identifier of each index at query time. Every source that contributed a rank to a result MUST appear in that result's explain output. A result with unexplained score components is a bug.

**Extension model.** When a new retrieval source is added (e.g., taxonomy-boosted ranking), it contributes an additional term to the RRF sum. The fusion formula does not change — new sources produce a new ranked list, and each segment's RRF score gains an additional `1 / (k + rank)` term for any source that retrieved it. Sources that did not retrieve a segment contribute nothing (not zero — absent). This additive model means new sources can be introduced without modifying the fusion algorithm itself.

### Architecture Invariants

- **BM25 is always required; vector is always optional.** If both are available, both contribute. No available source is silently dropped from fusion.
- **Fused scores are ordinal, not calibrated.** Consumers MUST NOT compare scores across queries, across index generations, or treat them as probabilities. This is a contract, not a suggestion.
- **Explain is exhaustive.** The explain output must enumerate every source that contributed to every result. Partial or missing attribution is a correctness violation.
- **Deterministic ordering.** Given the same query and the same index state, the result ordering MUST be identical across invocations. The tie-break rule ensures this.
- **Source of truth.** When fusion produces a ranking, the fused rank list is the authoritative ordering for that query. Per-source raw scores are informational; the fused ordinal ranking is what consumers act on.

### Deliberate Absences

- Per-query tuning of the `k` constant is not decided. The current value (60) is a system-level default; whether to expose it as a query parameter is a future decision.
- Query-dependent source weighting (e.g., boosting vector for semantic queries, boosting BM25 for keyword queries) is not specified.
- Pluggable fusion strategies (swapping RRF for another algorithm at runtime) are not specified.
- Score calibration or normalization to produce probability-like outputs is explicitly not provided.
- Cache invalidation strategy for stored retrieval results is not specified by this ADR.

## Consequences

- **Users always get results.** BM25 provides a retrieval baseline as soon as documents are indexed, before any embedding model is configured. Embeddings enhance retrieval quality but are never a prerequisite.
- **Explain shows provenance.** Users and agent consumers can always see where results came from — which sources contributed, with what ranks — enabling trust and debugging.
- **Adding a new retrieval source** requires producing a ranked list and feeding it into the existing RRF sum. The fusion algorithm itself does not change, but the explain contract expands to cover the new source.
- **Complexity cost:** Every new source adds a dimension to explain output and to integration testing. The fusion layer must handle any combination of present/absent sources gracefully.
- **Performance cost:** RRF computation is linear in the number of sources times the number of candidate segments. Additional sources increase fusion time proportionally.
- **API surface cost:** Consumers that parse explain output must handle a variable set of contributing sources. The explain schema must be forward-compatible with new source types.
- **Testing cost:** Each combination of present/absent sources (BM25-only, BM25+vector, BM25+vector+taxonomy, etc.) requires dedicated test coverage to verify correct fallback and explain completeness.
- Score values are not portable across queries; ranking comparisons are only meaningful within a single result set.

## Alternatives Considered

- **Linear combination of scores:** Requires that BM25 and vector scores occupy a shared, calibrated range. Neither BM25 scores nor raw cosine similarities satisfy this without normalization that introduces additional tuning surface. Would produce calibrated-looking numbers that tempt cross-query comparison — the opposite of the ordinal contract.
- **Learned fusion (Learning to Rank):** Requires labeled training data tied to this corpus and query distribution. Not available at launch; retraining cadence would couple retrieval quality to data collection infrastructure. Would produce better rankings for known query patterns but degrade unpredictably on novel ones.
- **CombMNZ:** Scores improve with the number of sources that retrieve a segment. Less robust when one source is absent — the missing source penalizes segments it would have ranked highly, distorting results rather than gracefully degrading. Violates the invariant that an absent source should be invisible, not harmful.

## Non-Goals

- Per-query `k` tuning is not in scope.
- Query-dependent source weighting is not in scope.
- Pluggable fusion strategies are not in scope.
- Score calibration or normalization to produce probability-like outputs is explicitly out of scope.
