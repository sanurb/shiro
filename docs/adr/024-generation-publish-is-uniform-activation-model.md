# ADR-024: Generation-Based Publish Is the Uniform Activation Model for Derived Views

**Status:** Proposed
**Date:** 2026-03-07

## Context

Generation-based activation exists for search indices. Other derived views (future retrieval caches, enrichment projections, or auxiliary ranking artifacts) risk inconsistent activation semantics if each adopts a different publish model.

## Decision

**Boundary:** This ADR defines activation semantics for derived views. It does not define implementation internals for each view type.

Generation-based publish is the uniform activation model for every query-visible derived view:

- Build in staging
- Validate compatibility and completeness
- Atomically promote
- Record active generation snapshot

### Architecture Invariants

- Query-visible derived artifacts MUST only become visible via generation activation.
- Readers MUST observe one coherent active generation snapshot per query.
- Failed builds MUST NOT partially activate.
- Recovery MUST reconcile filesystem/storage generation records.

### Deliberate Absences

- This ADR does not require all derived views to share one physical storage backend.
- This ADR does not define retention count for old generations.
- This ADR does not define garbage collection schedule.

## Consequences

- Activation semantics stay consistent across evolving subsystems.
- Recovery and observability patterns can be shared.
- Build/publish orchestration becomes a common platform concern.

## Alternatives Considered

- Per-subsystem activation semantics: local flexibility, global inconsistency.
- In-place mutation activation: lower storage overhead, higher partial-visibility risk.

## Non-Goals

- Defining generation ID format.
- Defining backup/restore procedures.
