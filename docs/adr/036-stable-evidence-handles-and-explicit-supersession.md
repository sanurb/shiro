# ADR-036: Stable evidence handles and explicit supersession

**Status:** Accepted  
**Date:** 2026-03-10

## Context

Search result IDs identify immutable retrieval snapshots, while segment IDs identify operational segmentation. Neither is a durable public reference to canonical evidence. Agents need to defer reads, deduplicate evidence across queries, and detect whether a cited block changed after reprocessing.

## Decision

Shiro assigns each canonical block an `EvidenceHandleId` with the `blk_` prefix. The handle is BLAKE3 over:

1. the content-addressed document ID;
2. the block's source-faithful canonical text; and
3. the occurrence ordinal of equal block text in reading order.

Byte spans, block arena indexes, source locators, block kinds, retrieval text, and segment IDs do not participate in identity. A segmenter-only change therefore cannot alter a handle. Parser changes preserve a handle when canonical block text and its equal-text occurrence identity remain stable.

The store snapshots every handle's canonical text, block metadata, source locators, and span. Replacing a graph marks handles absent from the replacement as `SUPERSEDED`. When the old and new byte spans overlap, Shiro records the deterministic maximum-overlap successor; a tie is resolved by handle ID. A missing overlap leaves `superseded_by` empty rather than claiming equivalence without evidence. Handles present in both graphs remain `ACTIVE` and receive current metadata.

`shiro read <blk_...>` returns the stored block snapshot and an explicit resolution object. It never silently redirects a superseded handle. Callers may decide whether to follow `superseded_by`. Search and explain expose the same handle; explain continues to identify its immutable search snapshot separately.

Page reads use parser-neutral source locators and return canonical blocks attributed to the requested one-based page. They are unavailable when the parser did not provide page provenance.

## Consequences

- Public evidence references survive segmentation and non-semantic span changes.
- Deferred reads do not depend on mutable ranking state.
- Duplicate block text remains deterministic without exposing operational segment IDs.
- Text edits intentionally create new handles.
- Supersession is explicit and conservative; absence of a successor is a valid result.
- Document deletion removes its handles under the existing document lifecycle.
