# ADR-007: Treat EntryPoint as the Primary Retrieval Result

**Status:** Accepted
**Date:** 2026-03-07

## Context

Current search returns segment-level results: a segment identifier plus scoring data. The consumer must then resolve back to a document and block context independently. Segments are an indexing artifact whose granularity is chosen to serve recall, not presentation. A segment boundary need not align with a natural reading boundary.

When a consumer (human or agent) receives a segment identifier, it holds an opaque chunk reference with no directly usable reading position. Context expansion already reconstructs surrounding blocks from the persisted BlockGraph in reading order, but that result is discarded rather than made first-class.

Tying the public retrieval result shape to the internal indexing unit couples consumers to implementation choices and makes any future change to segmentation strategy a breaking API change.

## Decision

**Boundary:** This ADR governs the shape of every retrieval result that crosses the SDK boundary. Anything above the SDK (CLI, MCP server, external consumers) sees EntryPoints. Anything below (indexing, storage, segment management) may use internal representations freely.

An **EntryPoint** is defined as: the best position in a document to begin reading in response to a query, including enough surrounding context for comprehension. It is the public retrieval result — the single type that consumers receive.

An EntryPoint carries the following information:

- **Document identity** — which document the result belongs to (as a DocId).
- **Block position** — which specific block matched the query.
- **Text span** — the byte range within the block's canonical text that is most relevant.
- **Context window** — an ordered sequence of surrounding blocks providing reading context, derived from the BlockGraph in reading order.
- **Scoring metadata** — fused retrieval score, individual ranking components, and any provenance needed for explanation or debugging.

**What is canonical:** The EntryPoint is the canonical (**canonical**: the representation that wins when others disagree; the authority from which all others are derived or rebuilt) retrieval result. All public APIs, CLI output, and MCP responses return EntryPoints.

**What is derived:** The EntryPoint itself is **derived** (**derived**: a representation computed from canonical data; rebuildable, not authoritative) — it is computed at query time from the persisted BlockGraph and index scores. It is not stored as a persistent entity; it is assembled on demand.

**What is allowed:** Consumers may depend on the stable shape of an EntryPoint. Internal indexing layers may use segment identifiers, block indices, and any internal representation they choose.

**What is forbidden:** Segment identifiers MUST NOT appear in any public retrieval result. No consumer above the SDK boundary may be required to resolve a segment identifier to obtain a reading position. Explanation and debugging output MUST render EntryPoints, not raw index artifacts.

Context expansion is an integral part of EntryPoint construction, not a separate post-processing step.

### Architecture Invariants

- An EntryPoint MUST resolve to a valid block in a persisted BlockGraph (hard dependency on ADR-006). An EntryPoint that references a block not present in the graph is a system error, not a degraded result.
- If the BlockGraph for a document is not yet persisted (ADR-006 implementation incomplete), the system MUST NOT return an EntryPoint for that document. It is acceptable to fall back to segment-level results in an internal or transitional mode, but such results MUST NOT be exposed through the public API as EntryPoints.
- When representations disagree, the persisted BlockGraph is the source of truth for block ordering and context window construction.
- Consumers MAY assume that every block referenced in an EntryPoint's context window exists and is ordered for reading. Consumers MUST NOT assume the context window is exhaustive — it is bounded by retrieval-time constraints.

### Deliberate Absences

- The internal mechanism by which a segment match is resolved to a block position is not specified. Implementations may use direct lookup, graph traversal, or any strategy that satisfies the invariants above.
- The maximum size of the context window is not specified here. It is a runtime or configuration concern.
- The serialization format of an EntryPoint (JSON field names, nesting) is not specified. That is an API-layer decision.
- Scoring metadata composition (which components, how fused) is not specified beyond requiring that the fused score and individual ranking components be present.

## Consequences

- **Product outcome:** A user or agent receives a reading position they can use directly — a precise location in a document with surrounding context — instead of an opaque chunk reference that requires further resolution.
- Decouples the public retrieval result shape from the indexing strategy: segmentation granularity can change without breaking consumers.
- **Hard dependency on ADR-006:** Requires a persisted BlockGraph to resolve block positions from segment matches at query time. Until ADR-006 is fully implemented, EntryPoint construction for documents without a persisted graph is blocked.
- **Migration cost:** Existing search result storage and CLI output schemas must change. Consumers depending on segment-level output will break and must be updated.
- **Complexity cost:** Query-time EntryPoint construction adds a resolution step (segment → block → context window) that did not previously exist. This is a new failure mode if the graph is missing or inconsistent.
- **Performance cost:** Assembling context windows requires reading the BlockGraph at query time, adding I/O and computation to every search request.
- One best EntryPoint per document per query is the model. Multi-entry-point ranking within a single document is out of scope.

## Alternatives Considered

- **Return segment identifier plus document identifier (status quo).** This is the current behavior. Every consumer must independently resolve segment identifiers to reading positions. Any change to segmentation is a breaking change for all callers. The resolution logic is duplicated across consumers. Choosing this alternative means accepting permanent coupling between consumers and the indexing strategy.
- **Return document identifier only.** Consumer performs all resolution. Maximally decoupled but shifts the entire burden to callers. No universal resolution API exists, so each consumer would need to implement its own block-level navigation. This makes shiro a document finder, not a knowledge retrieval engine.
- **Return page number.** PDF-specific. Markdown and other non-paginated formats have no page concept. This alternative is not universally applicable across parser implementations and would require format-specific branching in every consumer.

## Non-Goals

- Multi-document entry points (a single query result spanning multiple documents).
- Ranking multiple entry points within a single document.
- Providing a full reading path or navigation graph from an EntryPoint.
- Changing how segments are created, stored, or scored internally.
