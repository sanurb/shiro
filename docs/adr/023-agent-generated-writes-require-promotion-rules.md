# ADR-023: Agent-Generated Writes Require Explicit Promotion Rules

**Status:** Proposed
**Date:** 2026-03-07

## Context

Agent-generated writes can add useful enrichment and structure but are non-deterministic and may be incorrect. Automatic inclusion in canonical retrieval risks silent quality regressions.

## Decision

**Boundary:** This ADR defines governance for agent-generated write promotion. It does not define agent execution runtime internals.

Agent-origin writes default to non-canonical trust zones and require explicit promotion rules before affecting canonical or default retrieval scopes.

Promotion rules MUST be policy-driven, auditable, and reversible.

### Architecture Invariants

- Agent-generated writes MUST be identifiable by provenance.
- Default retrieval MUST exclude non-promoted agent-generated artifacts.
- Promotion events MUST record actor, reason, and timestamp.
- Demotion/revocation MUST be supported without rewriting canonical source content.

### Deliberate Absences

- This ADR does not define one approval workflow (manual vs assisted).
- This ADR does not define rule language syntax.
- This ADR does not define notification systems for pending promotions.

## Consequences

- Agent augmentation can be adopted safely with explicit governance.
- Additional operational workflows are required for review/promotion.
- Promotion latency may delay usefulness of generated artifacts.

## Alternatives Considered

- Auto-promote all agent writes: fastest flow, highest trust risk.
- Never allow promotion: safest strictness, underutilizes agent assistance.

## Non-Goals

- Defining agent model selection.
- Defining UI for promotion queue management.
