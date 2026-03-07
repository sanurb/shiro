# ADR-006: Persist the Document Graph as a First-Class Stored Representation

**Status:** Accepted
**Date:** 2026-03-07

## Context

`BlockGraph` (defined in `shiro-core::ir`) is constructed during parsing by `shiro-parse` (via `MarkdownParser`, `PdfParser`, and `segment_document()`) but is not persisted to SQLite. After ingest, only the `segments` table is populated — a derived flattening of the graph. The `documents` table stores `canonical_text` and a `ProcessingFingerprint`, but not graph topology.

This means:

- `Block` (with `BlockKind` and `Span`), `Edge` (with `Relation`), and `reading_order: Vec<BlockIdx>` are discarded after ingest.
- Context expansion in `shiro-sdk` uses segments as a structural proxy, which loses inter-block relationships.
- The `explain` command cannot reference block-level structure, edges, or reading order.
- Any block-level feature (graph traversal, structural retrieval, dependency ordering) requires re-parsing, coupling all such features to parser availability and determinism.

## Decision

`BlockGraph` (or a canonical relational projection of it) MUST be persisted in SQLite as a first-class stored representation. It is not a transient ingest artifact.

Two new tables are introduced as part of a schema migration from v4 to v5:

**`blocks`** — one row per `Block` in a document:
- `doc_id` (references `documents`)
- `block_idx` (position within document, corresponds to `BlockIdx`)
- `kind` (serialized `BlockKind`)
- `span_start`, `span_end` (byte offsets from `Span`)
- `reading_order` (integer rank within `reading_order: Vec<BlockIdx>`, NULL if absent)

**`edges`** — one row per `Edge` between blocks:
- `doc_id`
- `source_idx`, `target_idx` (references into `blocks.block_idx`)
- `relation` (serialized `Relation`)

`reading_order` is stored as an integer column on `blocks` rather than a separate join table to avoid an additional relation with no non-key attributes.

The `segments` table remains as a derived operational view used by BM25 (`shiro-index`) and vector search (`shiro-embed::FlatIndex`). Segments are NOT replaced — they serve a different purpose (see ADR-009).

Write path: `shiro-store` persists `blocks` and `edges` immediately after `shiro-parse` produces a `BlockGraph`, within the same transaction that writes the `documents` row and `segments`.

`DocState` transitions (Staged → Indexing → Ready/Failed) are unaffected. The new tables are populated during the Indexing phase.

## Consequences

- Block-level retrieval results become possible: callers can receive a `Block` reference, not only a `SegmentId`.
- Context expansion in `shiro-sdk` can traverse actual graph edges rather than using segment adjacency as a proxy.
- `explain` output can reference blocks, edges, and reading order by index.
- Schema migration from v4 to v5 is required; `schema_meta` version is bumped, and existing databases must be migrated or rebuilt.
- Storage cost is proportional to block count. For typical documents this is modest; no compression is applied at this layer.
- The `Parser` trait's determinism requirement (`name()` + `parse()` must be deterministic, captured in `ProcessingFingerprint`) becomes load-bearing: if re-parse is ever used to reconstruct a graph (e.g., after migration), the result must be identical.

## Alternatives Considered

**Keep graph transient; re-parse on demand.** Avoids schema change. Requires parser to be present and deterministic at query time; adds latency to every structural query; couples read path to parse path.

**Store `BlockGraph` as a serialized blob** (e.g., bincode or JSON in the `blobs` table). Avoids new tables. Precludes relational queries (filtering by `BlockKind`, traversing edges with SQL). Does not benefit from WAL mode's concurrent read performance.

**Store only `reading_order` without full edge graph.** Simpler schema. Loses `Relation`-typed edges; structural queries that need edge semantics (e.g., containment, reference) are impossible without re-parse.

## Non-Goals

- No graph query language is introduced. SQL over `blocks` and `edges` is the query interface.
- No rendering or layout information is stored. `Span` offsets are byte positions in canonical text, not visual coordinates.
- Parser-internal AST nodes are not persisted. Only the `BlockGraph` abstraction exposed by `shiro-core::ir` is stored.
- No changes to the `VectorIndex` trait (`shiro-embed`) or `FlatIndex` persistence format.
- No changes to `SegmentId` derivation (`blake3(doc_id:index)`) or the `segments` table schema.
