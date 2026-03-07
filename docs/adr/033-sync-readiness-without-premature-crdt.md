# ADR-033: Sync Readiness Without Premature CRDT Adoption

**Status:** Proposed
**Date:** 2026-03-07

## Context

Future multi-device or collaborative sync is plausible. Adopting CRDTs now would impose complexity before concrete sync requirements are validated.

## Decision

**Boundary:** This ADR governs present architecture posture toward future sync. It does not decide final sync protocol.

Shiro remains sync-ready by preserving stable identities, provenance, versioned derived artifacts, and explicit conflict surfaces, without adopting CRDTs as a default architectural dependency at this stage.

### Architecture Invariants

- Core data model MUST retain durable IDs and provenance needed for future replication.
- Canonical/derived separation MUST remain intact under potential sync.
- Conflict-prone domains MUST remain explicit in schema/contracts.
- Premature CRDT-specific constraints MUST NOT shape unrelated local-first paths.

### Deliberate Absences

- This ADR does not reject CRDTs permanently.
- This ADR does not define sync transport.
- This ADR does not define conflict-resolution UX.

## Consequences

- Current architecture avoids unnecessary distributed-systems complexity.
- Future sync work retains clear extension points.
- Some future migrations may still be required when concrete sync constraints are known.

## Alternatives Considered

- Adopt CRDT foundation now: future-proofing upside, major immediate complexity cost.
- Ignore sync readiness entirely: short-term simplicity, high future migration risk.

## Non-Goals

- Designing offline-first collaboration protocol.
- Defining replication topology.
