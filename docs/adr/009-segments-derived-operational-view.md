# ADR-009: Segments Are a Derived Operational View, Not the Canonical Semantic Unit

**Status:** Accepted
**Date:** 2026-03-07

## Context

Segments are useful for indexing and retrieval performance, but segment boundaries are operational artifacts of parser/segmenter behavior. Treating segments as canonical semantics creates instability when segmentation changes.

## Decision

**Boundary:** This ADR defines the role of segments in indexing and retrieval contracts. It does not decide segmentation algorithms.

Segments are **derived operational views**. Canonical document structure and semantics remain in the persisted Document Graph representation.

Segment identifiers are stable only within a processing fingerprint context. Any semantic interpretation required by APIs MUST resolve through canonical document structure.

### Architecture Invariants

- Segments MUST be derivable from canonical representation and processing fingerprint.
- Segment-level retrieval MUST be mappable to canonical entry points.
- Segment drift due to parser/segmenter changes MUST NOT redefine canonical document semantics.
- Rebuild from canonical data MUST be sufficient to regenerate segments.

### Deliberate Absences

- This ADR does not require one segmentation strategy.
- This ADR does not define segment size targets.
- This ADR does not define segment storage layout.

## Consequences

- Segmenter changes become operational migrations, not semantic model breaks.
- Retrieval consumers can rely on canonical references rather than brittle segment-only contracts.
- Additional mapping logic is required from segment hits to canonical entry points.

## Alternatives Considered

- Make segments canonical: easier immediate indexing, but fragile under parser drift and poor semantic durability.
- Keep dual-canonical model (graph + segments): creates ambiguity and conflict resolution burden.

## Non-Goals

- Defining context expansion policy.
- Defining retrieval fusion policy.
