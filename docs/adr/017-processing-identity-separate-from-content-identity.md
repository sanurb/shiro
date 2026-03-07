# ADR-017: Processing Identity Is Separate from Content Identity Across the Full Pipeline

**Status:** Proposed
**Date:** 2026-03-07

## Context

Content identity and processing identity are distinct concerns. Content hashes identify source bytes, while pipeline evolution (parser, segmenter, embedder, enrichment logic) requires separate drift detection and rebuild triggers.

## Decision

**Boundary:** This ADR extends identity separation across all processing stages. It does not redefine document content IDs.

Every derived artifact MUST be associated with explicit processing identity metadata for its stage family:

- Parse/segment processing identity
- Embedding processing identity
- Enrichment processing identity

Content identity alone is insufficient to assert derived artifact freshness.

### Architecture Invariants

- Content identity MUST NOT be used as a proxy for processing freshness.
- Each derived stage MUST define versioned processing identity components.
- Mismatched processing identity MUST trigger incompatible/stale handling.
- Processing identity metadata MUST be queryable for reprocessing decisions.

### Deliberate Absences

- This ADR does not define one global fingerprint format for all stages.
- This ADR does not define automatic migration timing.
- This ADR does not define how much historical identity metadata is retained.

## Consequences

- Drift detection is explicit and auditable across pipeline stages.
- Reprocessing policies become clearer and safer.
- Metadata volume and operational complexity increase.

## Alternatives Considered

- Content-hash-only identity: operationally simple but cannot detect behavior drift.
- Single monolithic pipeline fingerprint: easier comparisons but weak stage isolation and poor diagnostics.

## Non-Goals

- Standardizing every stage's internal algorithm version scheme.
- Defining policy thresholds for automatic rebuild.
