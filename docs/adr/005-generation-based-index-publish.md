# ADR-005: Generation-Based Index Publish

**Status:** Accepted
**Date:** 2026-03-07

## Context

Shiro maintains two indexes: a Tantivy BM25 full-text index in `shiro-index` and a brute-force cosine similarity vector index (`FlatIndex`) in `shiro-embed`. Both must serve consistent read snapshots at all times.

Incremental adds (upsert of individual segments) are safe to apply in-place — readers see a superset of the previous state. Full reindexes are not: building directly against the live index directory exposes partially-written state to concurrent readers, which can produce corrupt or incomplete search results.

The system must guarantee that a reader either sees the old index in its entirety or the new index in its entirety, never a mix.

## Decision

Each full reindex builds into a staging directory tagged with a `GenerationId(u64)`, a monotonically increasing integer. Promotion to live is a three-step atomic swap:

1. Rename live → backup
2. Rename staging → live
3. Delete backup

The `active_generations` table in the SQLite store records which `GenerationId` is currently live per index kind. This table is the authoritative source for which generation readers should open.

This pattern is implemented in both `shiro-index` (Tantivy FTS, staging build + promote) and `shiro-embed` (`FlatIndex`, JSONL persistence with blake3 checksums).

## Consequences

- Concurrent readers always see a consistent, fully-built snapshot; they are never exposed to a partially-written index.
- A failed build leaves staging in place without touching the live directory. No corruption occurs; the next build overwrites staging and retries promotion.
- The `active_generations` table enables future rollback: the backup from a previous promotion step can be restored by reversing the rename sequence.
- `GenerationId` is monotonic and never reused; readers can detect staleness by comparing their opened generation against the value in `active_generations`.
- Promotion involves filesystem renames, which are atomic within a single volume on supported OSes (Linux ext4/xfs, macOS APFS). Cross-device moves are not atomic and must not be used.
- The staging → live rename is not coordinated with the SQLite WAL; the `active_generations` update and the filesystem rename are not a single atomic operation. A crash between them leaves `active_generations` pointing at a generation whose directory may not exist. Recovery requires re-scanning present directories on startup.

## Alternatives Considered

- **In-place mutation with WAL**: Writing new segments directly into the live Tantivy index using its internal WAL. Safe for incremental adds but does not provide atomic visibility for full reindexes, as the index directory is partially written throughout the build. Rejected for full reindex scenarios.
- **Dual-index swap without generation tracking**: Maintaining two named slots (A/B) and swapping a symlink or config pointer. Provides atomic visibility but no monotonic generation record, no rollback path beyond a single previous version, and no audit trail. Rejected.

## Non-Goals

- Multi-version concurrent reads (MVCC): readers open exactly one generation at a time. Serving queries against multiple historical generations simultaneously is not implemented.
- Automatic rollback on read-error or query-latency regression: generation table enables rollback but triggering logic is not part of this decision.
- Distributed or multi-process coordination: generation tracking is local to a single `ShiroHome` instance. Cross-process safety relies on SQLite WAL mode serialization for the `active_generations` update only.
