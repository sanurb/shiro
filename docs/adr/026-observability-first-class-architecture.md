# ADR-026: Observability Is a First-Class Architectural Concern

**Status:** Proposed
**Date:** 2026-03-07

## Context

Shiro coordinates ingest, index publish, retrieval fusion, trust filtering, and reprocessing. Failures and quality regressions are difficult to diagnose without architecture-level observability contracts.

## Decision

**Boundary:** This ADR defines required observability capabilities. It does not mandate one telemetry vendor.

Observability is part of the architecture, not an implementation afterthought. Core paths MUST emit structured telemetry for:

- Ingest lifecycle and state transitions
- Index generation build/publish events
- Retrieval routing/fusion decisions and source availability
- Trust/provenance filtering decisions
- Reprocessing drift detection and execution

### Architecture Invariants

- Every user-visible failure path MUST emit diagnosable structured events.
- Critical state transitions MUST be traceable by stable identifiers (`run_id`, `doc_id`, `generation_id`).
- Observability output MUST avoid leaking sensitive raw content by default.
- Telemetry absence on core paths is a correctness gap.

### Deliberate Absences

- This ADR does not require distributed tracing specifically.
- This ADR does not define one log schema versioning mechanism.
- This ADR does not define external alerting ownership.

## Consequences

- Operations and debugging become materially faster and safer.
- Instrumentation becomes part of definition-of-done for core changes.
- Runtime overhead increases modestly due to telemetry emission.

## Alternatives Considered

- Best-effort logging only: low upfront effort, weak diagnosability.
- Add observability later: delays complexity but accumulates blind spots and retrofit cost.

## Non-Goals

- Selecting one observability stack.
- Defining retention windows for telemetry backends.
