# ADR-032: Adaptive Retrieval Roadmap Is Phased Behind Stable Contracts

**Status:** Proposed
**Date:** 2026-03-07

## Context

Adaptive retrieval capabilities (query classification, dynamic routing, policy-aware source weighting) are valuable but can destabilize baseline retrieval contracts if introduced monolithically.

## Decision

**Boundary:** This ADR governs rollout strategy for adaptive retrieval features. It does not define specific adaptive algorithms.

Adaptive retrieval evolves in explicit phases behind stable contracts:

1. Baseline deterministic hybrid retrieval
2. Pre-fusion routing policy
3. Adaptive routing/weighting experiments gated by evaluation
4. Default adoption only after benchmark and explainability parity

### Architecture Invariants

- Baseline retrieval contract remains stable while adaptive features are introduced.
- Adaptive features MUST preserve explainability obligations.
- Any adaptive default switch requires benchmark evidence and rollback path.
- Experimental adaptive behavior MUST be opt-in until accepted.

### Deliberate Absences

- This ADR does not define one adaptive model family.
- This ADR does not define online learning infrastructure.
- This ADR does not define personalization policy.

## Consequences

- Innovation can proceed without breaking core contracts.
- Additional feature-flag and experimentation infrastructure is needed.
- Roadmap governance must track phase exit criteria.

## Alternatives Considered

- Big-bang adaptive rewrite: faster capability jump, high regression and rollback risk.
- Freeze on baseline forever: low risk, limited long-term quality gains.

## Non-Goals

- Defining experiment platform implementation.
- Defining business-priority sequencing of adaptive features.
