# ADR-025: Define Scale Targets and Resource Budgets as Architectural Constraints

**Status:** Proposed
**Date:** 2026-03-07

## Context

Without explicit scale targets and resource budgets, architectural choices can optimize local benchmarks while violating practical deployment constraints for local-first users.

## Decision

**Boundary:** This ADR defines that scale and resource envelopes are architectural constraints. It does not set final numeric thresholds.

Shiro MUST maintain explicit, versioned targets for:

- Corpus size envelope
- Ingest throughput expectations
- Query latency envelopes by retrieval mode
- Memory/disk budget envelopes
- Rebuild/reprocessing budget envelopes

Release readiness and architectural changes MUST be evaluated against these budgets.

### Architecture Invariants

- Budget and target definitions are required artifacts, not optional notes.
- Architecture changes that exceed budget without explicit revision are regressions.
- Benchmarks MUST report against declared envelopes.
- Targets MUST be revisited with versioned updates, not ad hoc overrides.

### Deliberate Absences

- This ADR does not define exact numeric targets.
- This ADR does not define hardware certification matrix.
- This ADR does not define autoscaling infrastructure.

## Consequences

- Tradeoffs become measurable and governable.
- Planning and release processes gain concrete guardrails.
- Initial setup overhead increases due to target/budget maintenance.

## Alternatives Considered

- No explicit budgets: faster iteration, unpredictable operational behavior.
- Hardcode one environment profile: simpler management, poor portability.

## Non-Goals

- Defining cloud capacity planning.
- Replacing benchmark-gate ADR policies.
