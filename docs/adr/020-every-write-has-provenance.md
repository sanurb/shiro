# ADR-020: Every Write Has Provenance Metadata

**Status:** Accepted
**Date:** 2026-03-07

## Context

`shiro-store` currently records `provider` (heuristic/llm) and `content_hash` on enrichment rows, but all other write paths — document ingest, segment creation, concept assignments, agent-generated artifacts produced via `shiro.execute` — carry no structured provenance. An agent operating through the MCP `shiro.execute` interface can mutate stored data in ways that are indistinguishable from user-initiated writes. For a local-first knowledge engine whose value is the integrity of its knowledge graph, the inability to answer "who wrote this, via what operation, and when" is a correctness gap, not just a missing feature.

`DocState` transitions (Staged → Indexing → Ready/Failed → Deleted) are tracked but the actor driving each transition is not. `ProcessingFingerprint { parser_name, parser_version, segmenter_version }` captures determinism of the parse pipeline but not the initiating actor.

## Decision

Every write operation MUST attach a provenance record with the following fields:

- `actor`: one of `user`, `system`, or `agent:<id>` — identifies the write initiator
- `operation`: the SDK operation name (e.g., `ingest_document`, `assign_concept`, `enrich`) that produced the write
- `timestamp`: UTC Unix milliseconds at time of write
- `source_hash`: blake3 content hash of the input artifact at time of write (mirrors `DocId` / `SegmentId` conventions already in `id.rs`)

Provenance is required on the following entities:

- **documents** (`shiro-store`): ingest source and initiating actor
- **segments** (`shiro-store`): derived from which parse invocation and which `parser_name`/`parser_version`
- **enrichments** (`shiro-store`): already has `provider` and `content_hash`; these are subsumed into the unified provenance schema
- **concept assignments** (`shiro-store` `doc_concepts` / taxonomy): who assigned, via what SDK operation
- **agent-generated artifacts**: MCP execution trace records linking `shiro.execute` call to written rows

Provenance is stored in SQLite (`shiro-store`) alongside the records it describes — either as additional columns on existing tables or as a `provenance` join table keyed by `(entity_type, entity_id)`. It is NOT stored in the Tantivy index (`shiro-index`) or the `FlatIndex` JSONL file (`shiro-embed`).

Provenance records are **immutable once written**. Corrections do not UPDATE existing rows; they INSERT new records with updated content and a reference to the superseded record. The superseded record is retained.

## Consequences

- Full audit trail for all mutations is queryable via SQL against `shiro-store`.
- Agent-generated content (actor prefix `agent:`) is distinguishable from user and system content at query time without out-of-band logging.
- `explain` output from SDK executor can surface the provenance chain for any retrieved segment or enrichment.
- Storage overhead is one provenance row per write event — modest relative to segment and enrichment volume.
- Schema migration required: existing tables (documents, segments, doc_concepts) need provenance columns or a join table; schema_meta version bumps past v4.
- The `enrichments` table `provider` column becomes a derived view of the unified provenance `actor` field to avoid redundancy.

## Alternatives Considered

- **Log-only audit (stderr/tracing):** Captures events but is not queryable, not durable across restarts, and not joinable to stored content. Rejected — logs are ephemeral, storage is the system of record.
- **Provenance in a separate system (e.g., external append-only log):** Splits the source of truth. Queries that need to join content with provenance require cross-system coordination. Rejected — shiro is local-first; a single SQLite database is the right boundary.
- **No provenance:** Acceptable for a single-user toy system. Not acceptable for a production knowledge engine where agent writes are possible and trust matters.

## Non-Goals

- Not implementing a full event-sourcing or CQRS architecture. SQLite remains the primary store; there is no event log that can replay state.
- Not tracking read operations. Provenance applies to writes only.
- Not implementing provenance-based access control. Trust zones are addressed in ADR-021.
- Not propagating provenance through derived indices. Tantivy and FlatIndex are rebuildable artifacts; provenance lives in the authoritative store only.
