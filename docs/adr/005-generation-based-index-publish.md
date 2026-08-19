# ADR-005: Generation-based index publication

**Status:** Accepted
**Date:** 2025-02-16
**Revised:** 2026-03-10

## Context

Shiro maintains BM25 and vector indices outside canonical SQLite state. A reader must never combine an FTS artifact for one corpus with vectors, canonical blocks, or document eligibility from another corpus. This applies to full rebuilds and incremental document changes.

Filesystem replacement plus a later database-pointer update has a crash window. In-place incremental FTS mutation also invalidates the previous vector corpus and can expose a mixed view. Canonical staging that commits before embedding can make an existing READY document disappear when embedding fails.

## Decision

Every complete derived corpus is identified by one immutable `CorpusManifest`. The manifest records:

- corpus digest and document/segment counts;
- FTS generation and artifact digest;
- optional vector generation, artifact digest, and complete embedding fingerprint;
- creation time.

Generation IDs are monotonically increasing and never reused. Builders reserve IDs before writing generation-specific paths. Failed or interrupted attempts may leave unreferenced artifacts and audit rows; those artifacts are never opened by readers or reused by later builds.

Builders write and validate all required artifacts before activation. Activation is one SQLite transaction that updates the active corpus manifest and both active generation pointers. Incremental hybrid publication also commits the candidate canonical graph, segments, provenance, processing fingerprint, and READY transition in that same transaction. Before activation, concurrent readers retain the previous complete canonical and index view. An embedding or validation failure rolls back canonical staging and leaves the previous manifest searchable.

Readers open only generations named by the active manifest. Long-lived readers compare their open generation with the authoritative pointer and fail with an explicit reopen error when stale. Startup verifies active artifact digests before serving reads.

BM25-only mutations that cannot publish vectors must explicitly deactivate the vector generation before changing corpus eligibility. They are a compatibility path, not a complete hybrid publication.

## Architecture invariants

- One active manifest names one complete corpus view.
- A vector generation is active only with its matching fingerprint and artifact digest.
- Canonical document readiness and matching FTS/vector activation become visible together for incremental hybrid publication.
- Every active artifact exists and passes digest/count validation before its pointer is committed.
- Generation IDs are never reused, including after failed attempts.
- Unreferenced generation directories are inert and safe to garbage-collect.
- Cleanup removes only canonical generation-directory names absent from every retained manifest; generation audit rows and manifest-referenced rollback/reuse artifacts remain.
- Readers never infer an active generation from directory names or modification times.
- Failure before activation preserves the previous searchable canonical and derived view.
- Process-termination tests kill a publisher immediately before and after activation and require restart to resolve one complete, digest-valid corpus.

## Consequences

- New and reprocessed documents become BM25/vector searchable without embedding unchanged segments again.
- Changed-document vectors can be combined with fingerprint-compatible vectors copied from the previous immutable generation.
- Incremental publication still rebuilds complete local FTS/vector artifacts; “incremental” describes bounded embedding work and atomic freshness, not an ANN-style in-place update.
- A publication temporarily requires disk for old and candidate generations.
- Failed attempts consume generation IDs; the next successful activation performs best-effort cleanup of their unreferenced artifact directories while retaining audit rows.
- Cleanup failures are reported through tracing but cannot turn an already-committed activation into a failed publication response.
- An already-open engine must reopen after another process activates a newer generation.

## Alternatives considered

### Mutate active FTS and vectors in place

Rejected. Independent commits expose partial or mixed corpora and make rollback ambiguous.

### Rename a live directory, then update SQLite

Rejected as the authority protocol. The two operations cannot be one transaction, and a crash can leave the pointer and filesystem disagreeing. Immutable generation paths avoid replacement entirely.

### Store vector rows in the canonical SQLite transaction

Rejected for the current architecture. It would collapse derived index storage into the canonical store and does not generalize to Tantivy or future index adapters.

### Copy the entire previous vector generation without fingerprint checks

Rejected. Reuse is valid only when provider, model, dimensions, normalization, truncation, and retrieval-text policy match exactly.

## Non-goals

- Multi-version query selection by public clients.
- Distributed consensus between multiple Shiro writers.
- ANN-specific online graph mutation.
- Automatic quality-regression rollback after a valid activation.
