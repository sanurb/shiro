# ADR-018: Reprocessing Policy for Parser, Segmenter, Embedder, and Enrichment Drift

**Status:** Proposed
**Date:** 2026-03-07

## Context

Identity drift can occur independently at parse, segmentation, embedding, and enrichment stages. Without a clear policy, reprocessing behavior becomes inconsistent and risks stale mixed-state retrieval.

## Decision

**Boundary:** This ADR defines policy-level reprocessing triggers and behavior. It does not prescribe job scheduler internals.

Reprocessing is stage-scoped and trigger-driven:

- Parser or segmenter drift triggers structural reprocessing and derived index rebuild.
- Embedder drift triggers vector re-embedding and vector index publish.
- Enrichment drift triggers enrichment recomputation under trust/provenance rules.

Reprocessing MUST be resumable and idempotent at the document level.

### Architecture Invariants

- Drift at a stage invalidates only artifacts derived from that stage and its dependents.
- Reprocessing MUST NOT mutate canonical source identity.
- Active retrieval views MUST remain generation-consistent during staged reprocessing.
- Reprocessing status MUST be observable and attributable.

### Deliberate Absences

- This ADR does not define exact batch sizing or parallelism.
- This ADR does not define user-facing progress UI.
- This ADR does not define automatic scheduling cadence.

## Consequences

- Drift handling becomes predictable and auditable.
- Partial reprocessing can reduce cost versus full rebuilds.
- Orchestration complexity increases due to dependency-aware invalidation.

## Alternatives Considered

- Always full rebuild: simplest to reason about, highest operational cost.
- Lazy reprocess on read: defers work but introduces variable query latency and mixed freshness windows.

## Non-Goals

- Designing distributed worker topology.
- Defining SLA values for reprocessing completion.
