# ADR-020: Every Write Has Provenance Metadata

**Status:** Accepted
**Date:** 2026-03-07

## Context

The knowledge base currently tracks processing metadata (parser identity, content hashes) on some write paths, but most mutations — document ingest, segment creation, concept assignments, agent-generated artifacts — carry no structured provenance. An agent operating through the execution interface can mutate stored data in ways that are indistinguishable from user-initiated writes.

For a local-first knowledge engine whose value depends on the integrity of its knowledge graph, the inability to answer "who wrote this, via what operation, and when" is a correctness gap, not a missing feature. Document lifecycle transitions are tracked but the actor driving each transition is not. Processing fingerprints capture determinism of the parse pipeline but not the initiating actor.

## Decision

**Principle:** Every mutation to the knowledge base must record who initiated it, through what operation, and when. The provenance record is immutable once written.

Every write operation MUST attach a *provenance record* — a structured annotation that identifies the origin of a mutation. Conceptually, a provenance record contains:

- **Actor**: the identity of the write initiator — a human user, the system itself, or a specific agent instance.
- **Operation**: the logical operation that produced the write (e.g., ingest, segmentation, enrichment, concept assignment).
- **Timestamp**: the UTC time at which the write occurred.
- **Content hash**: a cryptographic hash of the input artifact at time of write, linking the provenance record to the specific content version it describes.

Provenance is required on all stored entities: documents, segments, enrichments, concept assignments, and any agent-generated artifacts.

Provenance records are **append-only**. Corrections do not mutate existing provenance records; they create new records with updated content and a reference to the superseded record. The superseded record is retained indefinitely.

Provenance is stored in the **authoritative store only** — the same persistent store that holds content. It is NOT stored in derived indices (search indices, embedding indices). *Derived* here means representations computed from authoritative data that can be rebuilt; provenance in those indices would be redundant and would diverge on rebuild.

### Architecture Invariants

- **Every write has provenance.** No mutation to the knowledge base may exist without an associated provenance record. A stored entity without provenance is a data integrity violation.
- **Provenance is immutable and append-only.** Once written, a provenance record is never modified or deleted. Corrections produce new records that reference the superseded one.
- **Provenance lives in the authoritative store only.** Derived indices do not store provenance. On rebuild, derived indices are recomputed from authoritative data; provenance remains in the authoritative store and does not need to be reconstructed.
- **Provenance is queryable.** Provenance records are accessible through the same query interface as content. Users and agents can filter, join, and inspect provenance without out-of-band access to logs or external systems.
- **Actor identity is structured.** The actor field distinguishes human-initiated writes, system-initiated writes, and agent-initiated writes. This distinction is the foundation for trust zone classification (ADR-021).

### Deliberate Absences

- The physical storage layout of provenance (additional columns vs. join table) is not decided by this ADR. Implementations may choose either approach.
- Provenance retention policy (how long superseded records are kept) is not specified. The current decision is to retain indefinitely; a future ADR may introduce compaction.
- Provenance-based access control is not specified. Trust zones (ADR-021) use provenance for classification but access control is a separate concern.
- Read-path provenance (tracking who queried what) is not in scope.
- Propagation of provenance through derived indices is explicitly excluded.

## Consequences

- **Users can answer "who added this and when."** For any piece of content in the knowledge base — a document, a segment, an enrichment, a concept assignment — the provenance record identifies the actor, operation, and timestamp. This is the foundation for audit, trust, and debugging.
- **Agent-generated content is distinguishable.** Writes initiated by agents carry agent-specific actor identity, enabling downstream systems (including trust zones, ADR-021) to classify content by origin without heuristics.
- **Explain output can surface provenance.** The retrieval explain payload can include the provenance chain for any retrieved segment or enrichment, giving consumers full visibility into content origin.
- **Migration cost:** Existing stored entities lack provenance. Migration must backfill existing records with a system actor and a migration operation marker. Records that predate provenance are distinguishable from records that were intentionally written by the system.
- **Storage cost:** One provenance record per write event. Modest relative to segment and enrichment volume, but grows monotonically due to the append-only invariant.
- **Complexity cost:** Every write path in the system must be instrumented to produce provenance. Missing instrumentation is a bug, not a missing feature. This adds a testing burden to every new write operation.
- **Schema cost:** Existing storage tables require schema changes to accommodate provenance. This is a breaking migration.
- **API surface cost:** Query interfaces must support provenance filtering and joining. The explain contract expands to include provenance data.

## Alternatives Considered

- **Log-only audit (stderr/tracing):** Captures events but is not queryable, not durable across restarts, and not joinable to stored content. Logs are ephemeral; the authoritative store is the system of record. Would satisfy debugging needs in the short term but cannot support trust zone classification or structured audit.
- **Provenance in derived indices as well:** Would place provenance records in both the authoritative store and derived indices (search index, embedding index). Rejected because derived indices are rebuildable artifacts — provenance stored there would be duplicated and would diverge on rebuild. The authoritative store is the single source of truth for provenance.
- **Provenance in a separate external system (append-only log):** Splits the source of truth. Queries that need to join content with provenance require cross-system coordination. Rejected — shiro is local-first; a single persistent store is the right boundary.

## Non-Goals

- Not implementing a full event-sourcing or CQRS architecture. The persistent store remains the primary store; there is no event log that can replay state.
- Not tracking read operations. Provenance applies to writes only.
- Not implementing provenance-based access control. Trust zones are addressed in ADR-021.
- Not propagating provenance through derived indices. Search and embedding indices are rebuildable; provenance lives in the authoritative store only.
