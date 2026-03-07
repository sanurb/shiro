# ADR-027: Benchmarking Is a Release Gate

**Status:** Accepted
**Date:** 2026-03-07

## Context

shiro CI runs fmt, clippy, test, check, schema-snapshot, capabilities-parity, and MCP-smoke. No retrieval quality or performance benchmarks exist.

Retrieval quality regressions are invisible until a user reports them. Concrete failure modes:

- Changing the tokenizer in `shiro-index` (Tantivy BM25) silently degrades ranking.
- Adjusting the RRF k parameter in `shiro-sdk` fusion (`rrf(s) = Σ 1/(k + rank_S(s))`, default k=60) shifts result ordering with no observable CI signal.
- `explain` output completeness — the per-result trace emitted by `shiro-sdk::executor` — is not validated.
- `shiro-embed::FlatIndex` rebuild integrity is tested for correctness but not benchmarked for latency or determinism under load.

Without gated benchmarks, regressions ship.

## Decision

Retrieval quality, latency, rebuild integrity, and explain completeness MUST be benchmarked and block release on regression.

**Required benchmark dimensions:**

| Dimension | Metric | Scope |
|---|---|---|
| Retrieval quality | precision@k, recall@k, MRR | Canonical evaluation corpora (see ADR-028) |
| Latency | p50 / p95 / p99 | search, ingest, reindex operations |
| Rebuild integrity | Determinism check | Full reindex from SQLite must produce bit-identical search results |
| Explain completeness | No missing fields | Every result from `shiro-sdk::executor` must carry a complete explain trace |

**Gating rules:**

- Benchmarks run in CI on every PR that touches `shiro-index`, `shiro-embed`, `shiro-sdk/fusion`, or `shiro-parse`.
- Regressions beyond defined per-metric thresholds fail the build.
- Benchmark results are stored as CI artifacts for trend analysis across releases.

Thresholds are committed as code alongside the benchmark suite. Threshold changes require explicit review.

## Consequences

- Retrieval quality regressions are caught before release, not after user reports.
- Performance budgets are explicit, enforceable, and visible in PR diffs.
- The benchmark suite becomes a living specification of expected retrieval behavior — changes to `shiro-sdk::fusion`, `shiro-index`, or `shiro-embed::FlatIndex` are self-documenting via threshold commits.
- CI wall time increases. Mitigated by path-scoped triggering: benchmarks only run when relevant crates change.
- Maintaining the evaluation corpus (ADR-028) becomes a first-class engineering responsibility.

## Alternatives Considered

- **Manual QA before release**: Slow, inconsistent, dependent on individual judgment. Does not scale.
- **Post-release monitoring only**: Regressions reach users before detection. Unacceptable for a local-first tool with no telemetry.
- **Sample-based spot checks in CI**: Incomplete coverage. Does not catch ranking regressions in tail queries or determinism failures under concurrent reindex.

## Non-Goals

- Not implementing A/B testing infrastructure or online experimentation.
- Not benchmarking shiro against external retrieval systems.
- Not optimizing benchmark scores at the expense of real-world retrieval utility.
- Not defining the evaluation corpus content — that is ADR-028's scope.
