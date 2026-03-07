# ADR-034: Managed Forgetting Affects Caches and Ranking Priors, Not Canonical Truth

**Status:** Proposed
**Date:** 2026-03-07

## Context

Managed forgetting can improve retrieval relevance by decaying stale signals, but applying forgetting directly to canonical truth risks accidental data loss and non-reproducible state.

## Decision

**Boundary:** This ADR defines where forgetting policies may apply. It does not define one forgetting function.

Managed forgetting may apply to derived retrieval aids (caches, ranking priors, optional transient boosts), but MUST NOT rewrite canonical source content or provenance history.

Canonical state changes require explicit user-driven delete or archival operations, not implicit decay.

### Architecture Invariants

- Canonical records are never silently decayed by forgetting policies.
- Forgetting effects must be reversible via cache/prior rebuild from canonical state.
- Forgetting policy application MUST be observable and explainable.
- Retrieval correctness must remain reproducible from canonical data plus declared policy.

### Deliberate Absences

- This ADR does not define retention/TTL values.
- This ADR does not define one scoring-decay algorithm.
- This ADR does not define legal/compliance deletion policy.

## Consequences

- Relevance tuning gains flexibility without corrupting canonical history.
- Derived-layer complexity increases due to policy lifecycle management.
- Explainability needs to surface forgetting-influenced ranking behavior.

## Alternatives Considered

- Apply forgetting directly to canonical data: aggressive simplification, unacceptable data integrity risk.
- No forgetting support: maximally stable truth, weaker long-term relevance adaptation.

## Non-Goals

- Implementing legal right-to-erasure workflows.
- Defining user interface for forgetting controls.
