# ADR-021: Partition Memory into Trust Zones

**Status:** Accepted
**Date:** 2026-03-07

## Context

`shiro-store` (SQLite, schema v4, WAL mode) holds user-ingested documents, system-derived segments, heuristic and LLM enrichments, concept assignments, and generation metadata in a single database with no structural trust differentiation. All rows participate equally in retrieval: BM25 search via `shiro-index` (Tantivy) and vector search via `shiro-embed` (`FlatIndex`) treat agent-generated content identically to user-verified content.

An agent operating via the MCP `shiro.execute` interface (JSON-RPC over stdio, `shiro-cli`) can produce enrichments, concept assignments, and annotations that are written to `shiro-store` and immediately surface in retrieval results. There is no mechanism to distinguish these writes from user-verified content, defer them for review, or exclude them from default queries. This is knowledge poisoning by design.

The `DocState` machine (Staged → Indexing → Ready/Failed → Deleted) manages processing lifecycle but does not model trust — a Ready document may have been agent-enriched without any user review. `GenerationId(u64)` tracks index versions but not the trustworthiness of the content in those versions.

## Decision

All data stored in `shiro-store` is assigned to one of four trust zones:

- **Canonical**: user-ingested raw documents and their parsed content. Highest trust. Immutable after ingest — no UPDATE path exists for canonical content; corrections re-ingest under a new `DocId` (blake3 of content).
- **Derived**: segments (`SegmentId`), BM25 index generations (`GenerationId`), vector embeddings, and enrichments produced by deterministic system processes (MarkdownParser, PdfParser, HttpEmbedder with fixed model+version). Trusted by construction — these are rebuildable from Canonical content given the same `ProcessingFingerprint`.
- **Proposed**: agent-generated content — enrichments, concept assignments, annotations, and any artifact written by an actor with the `agent:<id>` provenance prefix (ADR-020). Untrusted until explicitly promoted. Not included in default retrieval.
- **Quarantined**: content flagged by automated validation or user review as suspect. Excluded from all retrieval paths until explicitly cleared or deleted. Separate from Proposed — quarantine is a verdict, not a default state.

Trust zone is stored as a column (`trust_zone ENUM('canonical','derived','proposed','quarantined')`) on each relevant table in `shiro-store` — documents, segments, enrichments, doc_concepts, concept_relations. It is NOT enforced via separate databases or schemas; referential integrity across trust zones is preserved.

Default retrieval (BM25 via `shiro-index`, vector search via `shiro-embed`, RRF fusion with k=60 in `shiro-sdk`) includes `canonical` and `derived` rows only. Proposed and Quarantined require explicit opt-in via query parameter (e.g., `--include-proposed` flag in `shiro-cli`, or `include_trust_zones` field in SDK spec).

Promotion from **Proposed → Derived** requires explicit user action and is defined in ADR-023. No automatic promotion path exists.

## Consequences

- Agent-generated content cannot silently pollute retrieval results. A `shiro.execute` call that writes enrichments produces Proposed rows; they are invisible to default queries until a user promotes them.
- Trust is queryable and filterable at the storage layer — no application-level post-filtering required.
- `explain` output from the SDK executor MUST include the `trust_zone` of each result component (segment, enrichment, concept) so the caller knows the trust composition of any answer.
- Migration required: add `trust_zone` column to documents, segments, enrichments, doc_concepts, concept_relations tables; backfill existing rows (documents → canonical, segments/enrichments → derived); schema_meta version increments.
- `FlatIndex` (JSONL, `shiro-embed`) stores embeddings for segments; trust filtering happens at the `shiro-store` layer before or after vector search, not inside `FlatIndex` itself.
- Tantivy index (`shiro-index`) does not natively store trust zones; filtering is applied as a post-search step joining against `shiro-store` — or Proposed/Quarantined documents are excluded from index promotion entirely.

## Alternatives Considered

- **Separate databases per trust level:** Eliminates cross-trust SQL joins cleanly, but breaks referential integrity (a Proposed enrichment cannot foreign-key a Canonical segment across database files), complicates `shiro-store` connection management, and adds operational surface area for a local-first tool. Rejected.
- **Tag-based trust (free-text labels):** Too loose — no enforcement at the query layer, no schema-level contract, trivially bypassed. Rejected.
- **No trust zones:** Acceptable for a single-user toy system where the user is the only actor. Not acceptable once MCP `shiro.execute` enables agent writes. Rejected for production use.

## Non-Goals

- Not implementing role-based access control (RBAC). Trust zones describe content origin, not user permissions.
- Not implementing cryptographic trust verification (e.g., signing canonical content). Content integrity is handled by blake3 hashing in `DocId` and `SegmentId`; signatures are out of scope.
- Not implementing trust propagation through concept graph edges (`concept_relations`, `concept_closure`). A concept relation derived from a Proposed enrichment does not automatically become Proposed — trust is assigned at write time by the actor, not inferred from graph topology.
- Not implementing automatic demotion (Derived → Proposed) if a dependency is quarantined. This is a future concern.
