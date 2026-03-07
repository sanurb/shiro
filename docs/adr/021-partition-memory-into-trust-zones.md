# ADR-021: Partition Memory into Trust Zones

**Status:** Accepted
**Date:** 2026-03-07

## Context

The knowledge base holds user-ingested documents, system-derived segments, heuristic and LLM enrichments, concept assignments, and generation metadata in a single persistent store with no structural trust differentiation. All stored content participates equally in retrieval: full-text search and vector search treat agent-generated content identically to user-verified content.

An agent operating through the execution interface can produce enrichments, concept assignments, and annotations that are written to the store and immediately surface in retrieval results. There is no mechanism to distinguish these writes from user-verified content, defer them for review, or exclude them from default queries. This is knowledge poisoning by design.

The document lifecycle state machine manages processing stages but does not model trust — a document that has completed processing may have been agent-enriched without any user review. Index generation identifiers track version currency but not the trustworthiness of the content in those versions.

## Decision

**Principle:** Stored data is partitioned by trust origin. Default retrieval excludes content that hasn't been verified. Promotion from unverified to verified requires explicit action.

A **trust zone** is a classification of stored content by its origin and verification status, used to control retrieval visibility. All content in the knowledge base is assigned to exactly one of four trust zones:

- **Canonical**: user-verified content that has been explicitly ingested. Highest trust. Immutable after ingest — corrections require re-ingestion under a new content-addressed identifier, not mutation of existing records. This is the authoritative representation of user knowledge.
- **Derived**: content produced by *deterministic* system processes from Canonical content. *Deterministic* here means identical input bytes produce identical output bytes across invocations, given the same processing version. Derived content is rebuildable from Canonical content and the processing version that produced it. Trusted by construction.
- **Proposed**: content produced by non-deterministic or agent processes — enrichments, concept assignments, annotations, and any artifact written by an agent actor (per ADR-020 provenance). Untrusted until explicitly promoted. NOT included in default retrieval.
- **Quarantined**: content flagged by automated validation or user review as suspect. Excluded from ALL retrieval paths until explicitly cleared or deleted. Separate from Proposed — quarantine is a verdict, not a default state.

Trust zone assignment happens at **write time** based on the actor and operation recorded in the provenance record (ADR-020). Trust zone is immutable after assignment except through explicit promotion or demotion operations. Trust is never inferred from content — it is determined by origin.

**Default retrieval** MUST include only Canonical and Derived content. Including Proposed or Quarantined content requires explicit opt-in from the caller. This opt-in must be a deliberate parameter in the query, not a global configuration toggle.

**Promotion** from Proposed to Canonical or Derived requires explicit user action. No automatic promotion path exists. The promotion mechanism is defined in ADR-023.

### Architecture Invariants

- **Every stored entity has exactly one trust zone.** There is no "unclassified" state. Trust zone is assigned at write time and is part of the entity's metadata.
- **Default retrieval includes only Canonical and Derived.** This is the foundational safety guarantee. Any query that does not explicitly opt in to other zones sees only verified content.
- **Trust zone assignment is immutable except through explicit promotion/demotion.** Trust does not change as a side effect of processing, re-indexing, or time. Only an explicit operation can change an entity's trust zone.
- **Trust is not inferred from content.** An entity's trust zone is determined by its provenance (who wrote it, through what operation), not by analysis of its content.
- **Quarantined Canonical cascades to Derived.** When a Canonical document is quarantined, all Derived content produced from it (segments, enrichments produced by deterministic processes) becomes orphaned. Orphaned Derived content MUST be excluded from retrieval. Whether orphaned content is explicitly quarantined, deleted, or simply filtered out by the retrieval layer when its source is quarantined is an implementation choice — but it MUST NOT appear in results.
- **Source of truth.** The trust zone stored in the authoritative persistent store is the authority. Derived indices (search index, embedding index) may cache or replicate trust zone information for filtering, but on disagreement, the authoritative store wins.

### Deliberate Absences

- The physical storage representation of trust zones (column type, encoding) is not specified by this ADR.
- Automatic demotion of Derived content when its Canonical source is quarantined — the invariant requires exclusion from retrieval, but the mechanism (cascade, lazy filter, eager update) is not decided.
- Trust propagation through concept graph edges is not specified. Whether a concept relation derived from Proposed content inherits the Proposed zone is left for a future decision.
- Cryptographic trust verification (e.g., signing Canonical content) is not in scope.
- Role-based access control is not in scope. Trust zones describe content origin, not user permissions.
- Trust zone assignment for content that predates this ADR — migration strategy (backfill rules: existing documents as Canonical, existing system-produced content as Derived) is implementation-level, not architectural.

## Consequences

- **Users can safely let agents write to their knowledge base.** Agent-produced content lands in the Proposed zone and is invisible to default queries. Users' search results are never polluted by unverified agent output. This is the core product outcome.
- **Trust is queryable and filterable at the storage layer.** No application-level post-filtering is required for the default case. Retrieval components can apply trust filtering early, before ranking.
- **Explain output includes trust zones.** The retrieval explain payload MUST include the trust zone of each result component (segment, enrichment, concept) so the caller knows the trust composition of any answer.
- **Migration cost:** Existing stored content must be assigned trust zones. This requires a schema migration and backfill of all existing entities.
- **Complexity cost:** Every write path must determine the correct trust zone based on provenance. Every retrieval path must enforce trust zone filtering. These are new invariants that must be tested.
- **Performance cost:** Trust zone filtering adds a predicate to every retrieval query. If filtering is applied as a post-search step (for indices that don't natively support trust zones), there is a cost proportional to the number of results filtered out.
- **API surface cost:** Query interfaces gain a trust zone opt-in parameter. Explain output expands to include trust zone per result component. Consumers must handle the new field.
- **Testing cost:** Each trust zone × retrieval path combination requires test coverage. Promotion/demotion flows require integration tests.

## Alternatives Considered

- **Separate databases per trust level:** Eliminates cross-trust queries cleanly, but breaks referential integrity (a Proposed enrichment cannot reference a Canonical segment across database boundaries), complicates connection management, and adds operational surface area for a local-first tool. Would provide stronger isolation but at significant complexity cost.
- **Tag-based trust (free-text labels):** Too loose — no enforcement at the query layer, no schema-level contract, trivially bypassed or inconsistently applied. Would be more flexible but sacrifices the safety guarantee that default retrieval excludes unverified content.
- **Binary trust (trusted/untrusted):** Simpler than four zones but conflates Derived (rebuildable, deterministic) with Canonical (user-verified) and conflates Proposed (awaiting review) with Quarantined (flagged as suspect). Loses the ability to distinguish between "not yet reviewed" and "reviewed and rejected." The four-zone model captures meaningful distinctions in content origin that affect user trust decisions.

## Non-Goals

- Not implementing role-based access control. Trust zones describe content origin, not user permissions.
- Not implementing cryptographic trust verification. Content integrity is handled by content-addressed hashing; signatures are out of scope.
- Not implementing trust propagation through concept graph edges. Trust is assigned at write time by the actor, not inferred from graph topology.
- Not implementing automatic demotion of Derived content when a dependency is quarantined. The exclusion-from-retrieval invariant covers the safety requirement; the demotion mechanism is a future concern.
