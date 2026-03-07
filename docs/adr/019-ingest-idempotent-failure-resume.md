# ADR-019: Ingest Is Idempotent with Explicit Failure and Resume Semantics

**Status:** Proposed
**Date:** 2026-03-07

## Context

Ingest spans parsing, persistence, and index publication stages. Failures during ingest are expected; retry behavior must avoid duplicates and ambiguous partial states.

## Decision

**Boundary:** This ADR defines ingest lifecycle semantics and retry behavior. It does not define transport/protocol for ingest requests.

Ingest operations are idempotent by document identity and processing identity. Failures produce explicit persisted state transitions. Resume/retry reuses existing canonical records when valid and recomputes missing derived artifacts.

### Architecture Invariants

- Repeating ingest with identical inputs and processing identity MUST converge to one ready document view.
- Partial ingest failure MUST leave explicit state for recovery.
- Ingest retry MUST NOT create duplicate canonical records.
- Retrieval MUST exclude non-ready ingest states.

### Deliberate Absences

- This ADR does not define CLI retry UX details.
- This ADR does not define distributed locking strategy.
- This ADR does not define dead-letter handling.

## Consequences

- Operators can safely rerun ingest after crashes.
- State machine contracts become load-bearing and test-critical.
- Additional bookkeeping is required for resumable progress.

## Alternatives Considered

- Best-effort ingest with implicit retries: lower implementation burden, poor failure transparency.
- Hard rollback on any failure: clean semantics, but expensive and less resilient for large batches.

## Non-Goals

- Defining queue-based ingestion architecture.
- Defining source file change detection strategy.
