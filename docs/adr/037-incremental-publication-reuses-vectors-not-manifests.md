# ADR-037: Incremental publication reuses vectors, not mutable manifests

**Status:** Accepted  
**Date:** 2026-08-16

## Context

Changed documents need fresh embeddings without whole-corpus model calls. Updating active FTS and vector files in place would reintroduce split generations, fingerprint ambiguity, and crash windows repaired by ADR-005.

## Decision

Incremental vector publication always creates new immutable FTS and vector generations and activates one complete corpus manifest. It may reuse owned vectors from the newest digest-verified manifest whose embedding fingerprint exactly matches the configured retrieval fingerprint. Vectors for changed document IDs and missing segment IDs are recomputed in bounded batches. Reused plus new entries must equal the complete target segment set and pass dimension, fingerprint, count, and artifact-digest validation before activation.

Canonical graph, segments, source provenance, processing fingerprint, staged document readiness, and manifest activation occur in one outer SQLite savepoint. A pre-activation embedding or validation failure rolls back canonical staging and leaves the previous complete corpus searchable. Reserved generation IDs are committed before that savepoint and never reused; failed artifacts remain inactive and auditable.

A query-embedding cache is not part of this decision. It may be introduced only after representative workload measurements establish hit rate and memory value.

Scoped reprocessing uses the same publication boundary. Dry-run is the default and reports selected sources, stale stages, transitive artifacts, byte/model/batch estimates, hard limits, and the exact verified rollback manifest. Execution must match an optional resume manifest and revalidate its artifacts first.

## Consequences

- Model work scales with changed segments while activation remains corpus-complete.
- Publication copies unchanged vectors at an explicit, non-search-path ownership boundary.
- FTS construction remains whole-corpus for now; this favors correctness over write throughput.
- Failed generations consume disk until cleanup policy removes inactive artifacts.
