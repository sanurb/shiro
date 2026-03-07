# ADR-005: Generation-Based Index Publish

**Status:** Accepted
**Date:** 2026-03-07

## Context

Shiro maintains multiple derived indices (full-text search, vector similarity). Both must serve consistent read snapshots at all times.

Incremental additions (upserting individual segments) are safe to apply in place — readers see a superset of the previous state. Full reindexes are not safe: building directly against the live index exposes partially-written state to concurrent readers, producing corrupt or incomplete search results.

The system must guarantee that a reader either sees the old index in its entirety or the new index in its entirety, never a mix of the two.

## Decision

**Boundary:** This decision governs how all derived indices are built, promoted, and activated. It applies uniformly to every index type — current and future. Any new index type must follow the same generation lifecycle.

Each full reindex builds into a staging location tagged with a GenerationId — a monotonically increasing, never-reused integer. Promotion to live is a directory-level swap: the staging location replaces the live location.

An active-generations record in the persistent store is the authoritative source for which GenerationId is currently live per index kind. Readers must consult this record to determine which generation to open.

**What is canonical:** The active-generations record is canonical. It determines which generation readers should open.

**What is derived:** Each index generation is a derived artifact, rebuildable from the canonical document store and the current processing pipeline.

**What is allowed:** Readers may open the generation identified by the active-generations record. A failed build may leave staging in place without affecting the live generation. The next build overwrites staging.

**What is forbidden:** Readers must not open a generation not listed in the active-generations record. Builds must not mutate the live index directory directly during a full reindex. Cross-volume moves must not be used for promotion (filesystem rename atomicity is single-volume only).

**Atomicity clarification:** "Atomic" in this context means atomic within a single filesystem volume — a directory rename is an atomic operation on supported filesystems. It does NOT mean atomic across the filesystem-plus-database boundary. The filesystem rename and the active-generations record update are two separate operations with a crash window between them.

### Architecture Invariants

- A reader that opens a GenerationId from the active-generations record must find a valid, complete index at the corresponding location. If the index is missing or corrupt, the system is in a recovery state requiring startup re-scan.
- GenerationId is monotonically increasing and never reused. Readers can detect staleness by comparing their opened generation against the active-generations record.
- The crash window between filesystem rename and active-generations update is a known invariant violation risk. If a crash occurs in this window, the active-generations record may point to a generation whose directory does not exist, or a promoted directory may not be reflected in the record. Recovery requires scanning present index directories on startup and reconciling with the record.
- Incremental additions to a live index are safe and do not require a new generation. Generation-based publish is required only for full reindexes.
- The generation model is the uniform activation model for ALL derived indices. Adding a new index type does not require a new publish mechanism — it registers its kind in the active-generations record and follows the same staging-promote lifecycle.

### Deliberate Absences

- Multi-version concurrent reads (MVCC) are not provided. Readers open exactly one generation at a time.
- Automatic rollback on read errors or quality regression is not specified. The generation model enables rollback, but triggering logic is not part of this decision.
- Distributed or multi-process coordination is not addressed. Generation tracking is local to a single shiro instance.
- Crash recovery logic (the specific startup re-scan algorithm) is not specified here — only the invariant that recovery must restore consistency.

## Consequences

- **Read consistency:** Concurrent readers always see a fully-built snapshot. Users never see partial or corrupt search results, even during a full reindex. This is the primary product outcome.
- **Failure isolation:** A failed build leaves staging in place without touching the live index. No corruption occurs; the next build retries.
- **Rollback capability:** The active-generations record and the backup from promotion enable rollback to a previous generation, though triggering rollback is not automated.
- **Staleness detection:** Monotonic GenerationId lets readers and operators detect when they are serving a stale generation.
- **Crash window risk:** The filesystem rename and active-generations update are not a single atomic operation. A crash between them requires startup recovery. This is a real failure mode with a small but nonzero probability window.
- **Storage cost:** Staging and backup directories coexist with the live directory during promotion, temporarily requiring up to 3x the index storage.
- **Complexity cost:** Every index type must implement the staging-promote lifecycle. Developers adding a new index kind must understand and follow the generation protocol.
- **Filesystem dependency:** Correct promotion depends on single-volume atomic rename semantics. Deployments that span multiple volumes or use network filesystems may violate atomicity assumptions.

## Alternatives Considered

- **In-place mutation with write-ahead log:** Writing new segments directly into the live index using its internal write-ahead log. Safe for incremental additions but does not provide atomic visibility for full reindexes — the index directory is partially written throughout the build. Readers may observe intermediate states. Rejected for full reindex scenarios.
- **Dual-slot swap (A/B):** Maintaining two named slots and swapping a pointer. Provides atomic visibility but no monotonic generation record, no rollback beyond a single previous version, and no audit trail. Loses the ability to detect staleness or reason about generation ordering. Rejected.
- **Copy-on-write with filesystem reflinks:** Using filesystem-level reflinks (e.g., cp --reflink) to create cheap snapshots of the live index before overwriting. Provides rollback and avoids the staging directory overhead. However, reflink support is filesystem-dependent (requires a COW filesystem), does not solve the atomic-visibility problem for full reindexes (the live directory is still mutated in place), and adds filesystem-specific code paths. Rejected as insufficient on its own, though it could complement the generation model as an optimization for backup creation.

## Non-Goals

- Multi-version concurrent reads: serving queries against multiple historical generations simultaneously.
- Automatic rollback triggers based on read errors or query quality regression.
- Cross-process or distributed coordination of generation state.
