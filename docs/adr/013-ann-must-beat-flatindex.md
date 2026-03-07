# ADR-013: ANN Backends Must Beat FlatIndex by Policy Before Default Adoption

**Status:** Proposed
**Date:** 2026-03-07

## Context

FlatIndex provides a correctness baseline for vector retrieval. ANN backends provide speed benefits but can degrade recall. Adoption requires explicit quality gates.

## Decision

**Boundary:** This ADR defines adoption policy for ANN defaults. It does not choose a specific ANN implementation.

Any ANN backend may be experimental, but it MUST NOT become the default vector backend unless it meets policy gates versus FlatIndex on official evaluation corpora.

Required policy gates:

- Recall threshold against FlatIndex top-k baseline
- Latency and resource profile published with benchmark evidence
- Deterministic index metadata/version compatibility checks

### Architecture Invariants

- FlatIndex remains the ground-truth correctness baseline.
- ANN default adoption requires measured evidence, not qualitative claims.
- Policy thresholds are versioned and release-gated.
- Failed policy gate blocks default switch.

### Deliberate Absences

- This ADR does not define exact numeric thresholds (owned by benchmark policy updates).
- This ADR does not ban ANN as opt-in experimental mode.
- This ADR does not define a specific ANN library.

## Consequences

- Performance improvements can ship safely without silent quality regressions.
- Benchmarking and corpus governance become operational requirements.
- ANN integration overhead increases due to evaluation and compatibility checks.

## Alternatives Considered

- Adopt ANN as default immediately: fastest path to speed, highest regression risk.
- Never adopt ANN: simplest quality model, forfeits scalable latency improvements.

## Non-Goals

- Designing ANN index internals.
- Replacing benchmark governance ADRs.
