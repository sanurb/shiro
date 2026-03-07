# ADR-010: Search Indices Are Derived, Rebuildable Views of Canonical State

**Status:** Proposed
**Date:** 2026-03-07

## Context

FTS and vector indices accelerate retrieval but can become stale, corrupted, or partially published during failures. Architectural recovery depends on whether these indices are treated as authoritative or derived.

## Decision

**Boundary:** This ADR governs authority and recovery semantics for retrieval indices. It does not define concrete index backend internals.

All search indices are derived artifacts from canonical SQLite-backed state and processing/embedding contracts. They may be dropped and rebuilt without canonical data loss.

Activation of rebuilt indices uses generation-based publish semantics.

### Architecture Invariants

- Canonical truth MUST NOT depend on presence of any retrieval index.
- Retrieval index rebuild from canonical records MUST be sufficient for recovery.
- No mutation may write authoritative-only state to retrieval indices.
- Active generation state MUST identify which derived view is queryable.

### Deliberate Absences

- This ADR does not prescribe rebuild scheduling policy.
- This ADR does not choose index backend implementations.
- This ADR does not define performance SLOs for rebuild duration.

## Consequences

- Crash recovery and migration are simplified: rebuild instead of repair.
- Operational tooling must support index wipe/rebuild workflows.
- Rebuild cost for large corpora remains a meaningful operational expense.

## Alternatives Considered

- Treat indices as partially authoritative: reduces rebuild work but weakens recovery guarantees and complicates consistency reasoning.
- Log incremental recovery only: can reduce rebuild time but adds coupling and replay complexity.

## Non-Goals

- Defining index warmup strategy.
- Defining query-time fallback UX.
