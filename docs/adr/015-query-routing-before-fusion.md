# ADR-015: Query Routing Happens Before Fusion

**Status:** Proposed
**Date:** 2026-03-07

## Context

Hybrid retrieval can involve multiple candidate sources, some unavailable or inappropriate for a given query context. Fusion should combine valid ranked lists, not decide source availability.

## Decision

**Boundary:** This ADR defines retrieval pipeline stage ordering between routing and fusion. It does not define query classification model details.

Query routing is a distinct pre-fusion stage that determines eligible retrieval sources for a query based on availability and policy. Fusion executes only over routed sources.

### Architecture Invariants

- Routing MUST happen before fusion.
- Fusion MUST NOT infer source unavailability; it only combines provided ranked lists.
- Source absence after routing is non-contribution, not implicit penalty.
- Explain output MUST identify routed sources and contributing sources.

### Deliberate Absences

- This ADR does not prescribe a specific routing algorithm.
- This ADR does not require ML-based query intent classification.
- This ADR does not define per-user personalization.

## Consequences

- Fusion logic remains simpler and source-agnostic.
- Routing policy can evolve independently of fusion mechanics.
- Retrieval pipeline observability must include route decisions.

## Alternatives Considered

- Let fusion handle all source presence logic: simpler pipeline shape but mixes concerns and complicates explainability.
- Hardcode fixed sources for every query: predictable but wastes resources and misses policy flexibility.

## Non-Goals

- Defining query rewrite strategy.
- Defining ranking weight calibration.
