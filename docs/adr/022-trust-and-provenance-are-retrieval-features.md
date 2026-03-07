# ADR-022: Trust and Provenance Are Retrieval Features, Not Audit-Only Metadata

**Status:** Proposed
**Date:** 2026-03-07

## Context

Provenance and trust zone metadata can be treated as passive audit records, but retrieval behavior depends on origin trust and promotion status. If retrieval ignores trust/provenance, untrusted data can silently influence ranking and context.

## Decision

**Boundary:** This ADR defines retrieval-time use of trust and provenance metadata. It does not define access control models.

Trust zone and provenance metadata are first-class retrieval inputs:

- Default retrieval policy filters and ranks using trust constraints.
- Explain output includes trust/provenance basis for included results.
- Policy changes must be configurable without schema reinterpretation.

### Architecture Invariants

- Retrieval MUST enforce trust zone policy at query time.
- Untrusted/unpromoted content MUST NOT appear in default result sets.
- Provenance attributes used in retrieval decisions MUST be explainable.
- Trust/provenance filtering MUST be deterministic for a fixed policy and index state.

### Deliberate Absences

- This ADR does not define full access-control authorization.
- This ADR does not define UI affordances for trust disclosure.
- This ADR does not define promotion workflow details.

## Consequences

- Trust and provenance become operationally meaningful, not passive metadata.
- Retrieval explain payload grows with policy attribution data.
- Query planning may incur additional filtering overhead.

## Alternatives Considered

- Audit-only provenance: simpler retrieval path, unsafe default behavior.
- Hard-delete untrusted content: simpler runtime policy, loses reversible workflow.

## Non-Goals

- Replacing provenance write requirements.
- Defining user identity/authn policy.
