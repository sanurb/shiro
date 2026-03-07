# ADR-002: SQLite as Source of Truth

**Status:** Accepted
**Date:** 2026-03-07

## Context

Shiro manages several persistent stores: document metadata, segment content, concept graph, enrichments, and generation tracking in `shiro-store` (SQLite via rusqlite, schema v4, WAL mode); full-text search in `shiro-index` (Tantivy BM25); and vector embeddings in `shiro-embed` (FlatIndex, JSONL). These stores can diverge after a crash or partial write. A single authoritative store is required to define correctness and enable recovery.

`shiro-store` holds the complete relational state: `documents`, `segments`, `blobs`, `concepts`, `concept_relations`, `concept_closure`, `doc_concepts`, `enrichments`, `generations`, `active_generations`, `search_results`, and `schema_meta`. `DocState` transitions (Staged → Indexing → Ready/Failed, any → Deleted) are transactional in SQLite.

## Decision

SQLite (`shiro-store`) is the authoritative source of truth. The Tantivy index (`shiro-index`) and FlatIndex (`shiro-embed`) are derived caches that can be fully rebuilt from SQLite state. No authoritative state lives outside SQLite.

## Consequences

- Backup and migration require only the single SQLite file. No multi-store coordination is needed.
- Index corruption (Tantivy or FlatIndex) is recoverable by dropping and rebuilding from SQLite without data loss.
- `GenerationId(u64)` monotonic versioning in `active_generations` allows `shiro-index` to detect stale or missing index builds and trigger a rebuild.
- WAL mode enables concurrent readers during indexing writes without blocking queries.
- Writes funnel through a single process; no distributed coordination problem exists.
- SQLite's row-level transactionality makes `DocState` transitions atomic and crash-safe.

## Alternatives Considered

- **Tantivy as primary store**: Tantivy is an inverted index optimized for text retrieval, not relational queries. Storing segment provenance, concept graphs, or enrichment metadata in Tantivy would require external joins or denormalization. Rejected.
- **Custom binary format**: Lower dependency weight but imposes a maintenance burden for schema evolution, crash recovery, and tooling. Rejected; SQLite provides these for free.
- **Embedded key-value store (e.g., sled, redb)**: No relational query capability; concept graph and closure tables require joins. Rejected.

## Non-Goals

- Shiro is not a distributed database. No replication, no multi-writer coordination.
- No remote SQLite access. The file is local to `ShiroHome`.
- No multi-tenancy. One SQLite file per `ShiroHome`.
