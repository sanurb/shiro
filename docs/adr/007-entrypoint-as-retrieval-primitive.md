# ADR-007: Treat EntryPoint as the Primary Retrieval Result

**Status:** Accepted
**Date:** 2026-03-07

## Context

Current search returns segment-level results: a `SegmentId` plus BM25 and vector scores. The consumer must then resolve back to document and block context independently. The `explain` command returns raw index artifacts — `result_id`, `doc_id`, `segment_id`, `block_id`, `span` — alongside scoring and expansion data.

Segments are an indexing artifact whose granularity is chosen to serve recall, not presentation. A segment boundary need not align with a natural reading boundary. When a consumer (human or agent) receives a `SegmentId`, it holds an opaque chunk reference with no directly usable reading position. Context expansion (via `--expand`) already reconstructs surrounding `BlockIdx` values from the persisted `BlockGraph` in reading order, bounded by `max_blocks` and `max_chars`; that result is discarded rather than made first-class.

Tying the public retrieval result shape to the indexing unit couples consumers to internal implementation choices and makes any future change to segmentation strategy a breaking API change.

## Decision

Retrieval MUST return an `EntryPoint` as its primary result type. An `EntryPoint` is the best entry position into a document for a given query — not only a document id or a chunk reference.

`EntryPoint` is defined as:

```
EntryPoint {
    doc_id:         DocId,
    block_id:       BlockIdx,       // the specific block that matched
    span:           Span,           // byte range into canonical_text
    context_window: Vec<BlockIdx>,  // surrounding blocks for readability
    scores:         ScoringMetadata,
}
```

- `segment_id` is internal to the indexing layer (`shiro-index`, `shiro-store`); it MUST NOT appear in the public retrieval result.
- `explain` MUST render an `EntryPoint`, not raw index artifacts.
- Context expansion (currently `--expand`) produces the `context_window` field of the `EntryPoint`; it is no longer a separate post-processing step.
- `ScoringMetadata` carries fused RRF score, individual BM25 and vector ranks, and any expansion provenance needed for explanation.
- The `search_results` table in `shiro-store` evolves to store `EntryPoint` data rather than raw segment matches.

## Consequences

- Consumers receive a directly usable reading position (`block_id` + `span` + `context_window`) without additional resolution calls.
- Decouples the public retrieval result shape from the indexing strategy: segmentation granularity can change without breaking consumers.
- Requires a persisted `BlockGraph` (ADR-006) to resolve `block_id` from a segment match at query time; this is a hard dependency.
- `search_results` table schema must be extended or migrated to store `EntryPoint` fields; schema version in `schema_meta` advances accordingly.
- `shiro-sdk` executor and DSL interpreter consume `EntryPoint` directly; downstream spec results no longer require a resolution step.
- `shiro-cli` JSON envelope changes: the `segments` field in search output is replaced by `entry_points`.

## Alternatives Considered

- **Return document id only.** Consumer performs all resolution. Maximally decoupled but shifts burden entirely to callers; no universal resolution API exists today.
- **Return segment id only (current behavior).** Ties every consumer to the indexing strategy. Any change to segmentation is a breaking change for all callers.
- **Return page number.** PDF-specific. `MarkdownParser` has no page concept; this alternative is not universally applicable across `Parser` implementations.

## Non-Goals

- Not implementing multi-document entry points (a single query result spanning multiple `DocId` values).
- Not ranking multiple entry points within a single document; one best entry point per document per query is the model.
- Not providing a full reading path or navigation graph from an `EntryPoint`.
- Not changing how segments are created, stored, or scored internally.
