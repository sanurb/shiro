# ADR-027: Benchmarking Is a Release Gate

**Status:** Accepted
**Date:** 2026-03-07

## Context

shiro's CI validates correctness (tests, linting, schema snapshots, protocol conformance) but does not benchmark retrieval quality or performance. Retrieval quality regressions are invisible until a user reports them.

Concrete failure modes without benchmarking:

- Changing the tokenizer or scoring parameters silently degrades ranking.
- Adjusting fusion parameters (e.g., RRF weights) shifts result ordering with no observable CI signal.
- Explain output completeness — the per-result provenance trace — is not validated.
- Index rebuild latency and **deterministic** (identical input bytes produce identical output bytes across invocations, given the same processing version) output are tested for correctness but not benchmarked for regression over time.

Without gated benchmarks, regressions ship.

## Decision

Retrieval quality, latency, rebuild integrity, and explain completeness are benchmarked in CI and block release on regression.

**Boundary:** This ADR decides *that* benchmarks gate releases and *which dimensions* are measured. It does not decide the evaluation corpus content (ADR-028's scope), the specific benchmark harness implementation, or CI infrastructure details.

**Benchmarked dimensions:**

| Dimension | What is measured | What regression means |
|---|---|---|
| Retrieval quality | Precision@k, recall@k, MRR against a **canonical** (the representation that wins when others disagree; the authority from which all others are derived) evaluation corpus | Users get worse search results |
| Latency | p50 / p95 / p99 for search, ingest, and reindex operations | Users experience slower interactions |
| Rebuild integrity | Deterministic reindex from storage produces identical search results | Index corruption or non-determinism has been introduced |
| Explain completeness | Every search result carries a complete provenance trace with no missing fields | Debugging and transparency are degraded |

**What is canonical:** Thresholds are committed as versioned code in the repository, alongside the benchmark suite. They are the single source of truth for acceptable performance and quality.

**What is derived:** Benchmark results for any given commit are derived by running the benchmark suite against the canonical evaluation corpus.

**What is allowed:**
- Threshold values may be updated when a legitimate improvement changes results (e.g., a new tokenizer improves quality but changes rankings). The threshold update commit is the auditable record of the intentional change.
- Non-deterministic benchmarks (e.g., timing-sensitive latency measurements) may use statistical thresholds (confidence intervals, percentile bounds) rather than exact equality.

**What is forbidden:**
- Suppressing or skipping a benchmark to unblock a release. The path forward is always to fix the regression or update the threshold with justification.
- Changing thresholds without code review. A threshold change is a decision about acceptable quality, not a CI configuration tweak.
- Releasing when any benchmark dimension is in regression beyond its defined threshold.

**Gating rules:**
- Benchmarks run in CI on every PR that touches retrieval, indexing, fusion, or parsing subsystems.
- Regressions beyond defined per-metric thresholds fail the build. The PR cannot merge.
- Benchmark results are stored as CI artifacts for trend analysis across releases.

### Architecture Invariants

- Thresholds are committed as code. Changing a threshold is a reviewable, auditable act — not a CI configuration tweak.
- Benchmark results are deterministic on identical inputs. Where true determinism is infeasible (timing), statistical thresholds with defined confidence levels are used instead of exact equality.
- The evaluation corpus is the source of truth for retrieval quality. If benchmark results and user-reported quality disagree, the corpus is expanded — the benchmark is not suppressed.
- A benchmark failure blocks the release pipeline. There is no manual override path that bypasses the gate.

### Deliberate Absences

- The evaluation corpus content and curation process are not defined here (see ADR-028).
- Specific threshold values for each metric are not specified — they are maintained in the benchmark suite code and evolve with the system.
- The benchmark harness implementation (framework, runner, reporting format) is not prescribed.
- Cross-release trend analysis tooling is not decided — only that results are stored as artifacts.
- Hardware requirements for benchmark reproducibility are not specified.

## Consequences

- **Product outcome:** Users get consistent retrieval quality across releases. Regressions are caught before they ship, not after user reports.
- Retrieval quality and performance budgets are explicit, enforceable, and visible in PR diffs.
- The benchmark suite becomes a living specification of expected retrieval behavior — threshold update commits document intentional quality changes over time.
- **CI cost:** Wall time increases. Mitigated by path-scoped triggering (benchmarks only run when relevant subsystems change).
- **Maintenance cost:** The evaluation corpus (ADR-028) becomes a first-class engineering responsibility. Stale or unrepresentative corpora produce false confidence.
- **Friction cost:** Legitimate improvements that change rankings require a two-step process — update code, then update thresholds with justification. This is intentional friction, but it slows velocity on retrieval changes.
- **Complexity cost:** Statistical thresholds for timing-sensitive benchmarks require careful calibration. Overly tight bounds cause flaky failures; overly loose bounds miss real regressions.

## Alternatives Considered

- **Manual QA before release:** Slow, inconsistent, dependent on individual judgment. Would catch obvious regressions but miss subtle ranking changes and latency degradation. Does not scale as the system grows.
- **Post-release monitoring only:** Regressions reach users before detection. For a local-first tool with no telemetry, detection depends entirely on user reports — an unacceptable feedback loop.
- **Continuous monitoring with rollback:** Run benchmarks after release against production-like workloads; roll back the release if regression is detected. Would catch regressions eventually, but users experience degraded quality during the detection window. Adds operational complexity (rollback infrastructure, release-channel management). Rejected in favor of pre-release gating, which prevents regressions from shipping at all.
- **Sample-based spot checks in CI:** Run a small subset of the evaluation corpus for speed. Would catch major regressions but miss ranking changes in tail queries or determinism failures. Incomplete coverage provides false confidence.

## Non-Goals

- Not implementing A/B testing infrastructure or online experimentation.
- Not benchmarking shiro against external retrieval systems.
- Not optimizing benchmark scores at the expense of real-world retrieval utility.
- Not defining the evaluation corpus content — that is ADR-028's scope.
