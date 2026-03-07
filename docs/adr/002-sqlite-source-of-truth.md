# ADR-002: SQLite as Source of Truth

**Status:** Accepted
**Date:** 2026-03-07

## Context

Shiro manages several persistent stores: a relational database for document metadata, segment content, concept graphs, enrichments, and processing state; a full-text search index for BM25 retrieval; and a vector index for embedding-based semantic search. These stores can diverge after a crash, partial write, or interrupted indexing run. Without a single authoritative store, there is no way to define what "correct" means or to recover from inconsistency.

## Decision

SQLite is the **canonical** (the representation that wins when others disagree; the authority from which all others are derived or rebuilt) source of truth for all persistent state in Shiro. The full-text search index and vector index are **derived** (computed from canonical data; rebuildable, not authoritative) caches that can be fully rebuilt from SQLite content. No authoritative state lives outside the SQLite database.

**Boundary:** This ADR decides where authoritative state lives and the recovery model. It does not decide the SQLite schema, the indexing pipeline, or the specific tables and columns used.

**What is canonical:** Document metadata, segment content, concept graphs, enrichments, processing state, and generation tracking all live in SQLite. SQLite is the single recovery path.

**What is derived:** The full-text search index (BM25) and the vector index (embeddings) are derived from SQLite content. They carry no authoritative state.

**What is allowed:** Any derived index may be dropped and rebuilt from SQLite at any time without data loss. Consumers may treat index results as best-effort and fall back to SQLite state for authoritative answers. Processing state transitions are transactional within SQLite.

**What is forbidden:** No authoritative state may be stored exclusively in a derived index. Derived indices must not be treated as recovery sources — if SQLite and an index disagree, the index is wrong and must be rebuilt. No external process may write to SQLite concurrently; writes funnel through a single process.

### Architecture Invariants

- SQLite is the sole recovery path. If the SQLite file is lost or corrupted beyond recovery, **all data is lost** — derived indices cannot reconstruct the canonical state. This is the fundamental cost of the single-source-of-truth design.
- When SQLite and any derived index disagree, SQLite wins. The correct recovery action is always to rebuild the index from SQLite, never to "fix" SQLite from index content.
- Processing state transitions (e.g., staged → indexing → ready, or any state → deleted) are transactional within SQLite. A crash mid-transition leaves the document in its previous state, not in a half-indexed limbo.
- Generation-based versioning allows derived indices to detect staleness. An index that is behind the current generation knows it must rebuild without consulting any external coordinator.
- Writes are single-process. No distributed coordination problem exists because Shiro does not support concurrent writers.

### Deliberate Absences

- **Schema details** are not decided here. Table names, column types, and schema version are implementation concerns that may change without revising this ADR.
- **Rebuild triggers** are not specified. How and when stale indices are detected and rebuilt is an implementation decision.
- **Backup strategy** is not prescribed. This ADR establishes that backup requires only the SQLite file, but does not decide how backups are taken, scheduled, or verified.
- **Encryption at rest** is not addressed. The SQLite file may contain sensitive document content; protection is out of scope for this decision.
- **WAL mode or journal mode** is an implementation choice, not an architectural decision.

## Consequences

- **Single-file backup and portability.** Users can back up, copy, or migrate their entire knowledge base by copying one SQLite file. No multi-store coordination, no "did I get all the files?" anxiety. This is a direct user-facing benefit for local-first operation.
- **Recoverable index corruption.** If the full-text or vector index becomes corrupted (disk error, interrupted write, version mismatch), it can be dropped and rebuilt from SQLite without any data loss. Users experience a rebuild delay, not data loss.
- **Single point of failure.** SQLite is the sole recovery path. If the SQLite file is lost or corrupted beyond SQLite's own recovery mechanisms, everything is gone — derived indices cannot reconstruct documents, metadata, or concept graphs. Users must maintain their own backups of the SQLite file.
- **Rebuild cost.** Full index rebuilds (BM25 + vector) are expensive: re-segmenting, re-embedding (which may involve API calls to an external embedding service), and re-indexing the entire corpus. For large knowledge bases, this can take minutes to hours and incur monetary cost for embedding API calls.
- **Write serialization.** Single-process writes avoid coordination complexity but mean that bulk ingest cannot be parallelized across processes. This is acceptable for the target scale but becomes a bottleneck if Shiro ever needs multi-process write throughput.
- **No partial recovery.** Because indices are derived, there is no way to recover "just the search index" independently. A corrupted SQLite means full data loss even if the search index is perfectly intact and contains all the content.

## Alternatives Considered

- **Tantivy as primary store:** Tantivy is optimized for text retrieval, not relational queries. Storing segment provenance, concept graphs, or enrichment metadata in Tantivy would require external joins or heavy denormalization. Choosing this would give fast text search but make every metadata query awkward and error-prone. Rejected.
- **Custom binary format:** Lower dependency weight but imposes a maintenance burden for schema evolution, crash recovery, and ad-hoc querying. Choosing this would mean reimplementing transaction safety, migration tooling, and query capabilities that SQLite provides for free. Rejected.
- **Embedded key-value store (e.g., sled, redb):** No relational query capability. Concept graphs and closure tables require joins that would have to be implemented in application code. Choosing this would trade SQLite's query expressiveness for marginally lower overhead. Rejected because relational queries are genuinely needed for concept graph traversal and enrichment lookups.
- **SQLite + search index co-primary:** Both SQLite and the full-text/vector index are authoritative for their respective domains — SQLite for metadata, the index for search content. This would avoid rebuild cost for index corruption (the index is its own authority) and allow index-specific optimizations without roundtripping through SQLite. Rejected because it creates a split-brain recovery problem: when the two disagree, there is no single arbiter. Every inconsistency would require domain-specific resolution logic, and crash recovery would need to coordinate two independent transactional systems. The operational simplicity of a single source of truth outweighs the rebuild cost.

## Non-Goals

- Shiro is not a distributed database. No replication, no multi-writer coordination.
- No remote database access. The SQLite file is local to the Shiro home directory.
- No multi-tenancy. One SQLite file per Shiro home directory.
