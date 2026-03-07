# ADR-006: Persist the Document Graph as a First-Class Stored Representation

**Status:** Accepted
**Date:** 2026-03-07

## Context

BlockGraph — the graph of typed blocks, inter-block edges, and reading order — is constructed during parsing but is currently not persisted. After ingest, only segments (a derived flattening of the graph) are stored. The document record holds the canonical text and a ProcessingFingerprint, but not the graph topology.

This means:

- Block-level structure (block kind, span, reading order) and inter-block relationships (edges with relation types) are discarded after ingest.
- Context expansion uses segments as a structural proxy, which loses inter-block relationships that the graph captured.
- The explain command cannot reference block-level structure, edges, or reading order.
- Any block-level feature (graph traversal, structural retrieval, dependency ordering) requires re-parsing, coupling the read path to parser availability and determinism (**deterministic** here meaning: identical input bytes produce identical output bytes across invocations, given the same processing version).

## Decision

**Boundary:** This decision governs what structural information is persisted for each document and the relationship between the graph representation and the segment representation. It does not govern how segments are derived from the graph, how search indices consume blocks, or what query interface is exposed over the graph.

BlockGraph must be persisted as a first-class stored representation. It is not a transient ingest artifact.

**What is canonical** (the authoritative representation from which all others are derived): The persisted graph is **canonical**. It is the source of truth for a document's structural representation. Segments are **derived** from it. If the graph and segments disagree, the graph wins.

**What must be persisted:**
- **Blocks:** Each block with its kind, byte-offset span in the canonical text, and its position in reading order (if present).
- **Edges:** Each inter-block relationship with its relation type, source block, and target block.

**What is allowed:** The write path must persist the graph and segments in the same transaction that writes the document record. The read path may reconstruct segments from the graph if needed.

**What is forbidden:** A document in Ready state must not exist without a persisted graph. The graph must not be treated as an optional or lazily-computed artifact. Segments must not be treated as the authoritative structural representation when the graph is available.

### Architecture Invariants

- Every document in Ready state must have a persisted graph. A document without a graph cannot be in Ready state. If a document record exists in Ready state but its graph is missing, the system is in an inconsistent state requiring re-ingestion.
- The graph is canonical; segments are derived. Any operation that can use graph structure must prefer it over segment adjacency.
- The write path persists graph and segments atomically (in the same transaction). There is no state where a document has segments but no graph, or a graph but no document record.
- The parser's determinism requirement (captured in ProcessingFingerprint, see ADR-004) becomes load-bearing: if re-parse is ever used to reconstruct a graph (e.g., during migration), the result must be identical to the original parse output for the same content and processing version.

### Deliberate Absences

- No graph query language is introduced. The query interface over persisted blocks and edges is not specified by this ADR.
- No rendering or layout information is stored. Span offsets are byte positions in canonical text, not visual coordinates.
- Internal parser AST nodes are not persisted. Only the BlockGraph abstraction (the domain-level graph of blocks and edges) is stored.
- How segments are derived from the graph (the segmentation algorithm) is not specified here.
- The specific persistence format (relational tables, serialized blob, or other) is an implementation choice not governed by this ADR, provided the required data (blocks with kind/span/reading-order, edges with relation/source/target) is queryable.
- Changes to vector index persistence or segment identity derivation are not part of this decision.

## Consequences

- **Block-level precision:** Search results and explain output can reference individual blocks with their kind, span, and position in reading order, rather than opaque segment chunks. This is the primary product outcome: users get block-level precision in search results and explain output instead of chunk-level.
- **Structural context expansion:** Context expansion can traverse actual graph edges (containment, reference, sequence) rather than using segment adjacency as a lossy proxy.
- **Read-path independence from parsers:** Structural features no longer require re-parsing. The read path is decoupled from parser availability and determinism (except during migration).
- **Schema migration required:** Existing databases that predate graph persistence must be migrated. Migration requires re-parsing all documents (the graph was not previously stored and cannot be reconstructed without the parser). Databases that cannot be re-parsed (e.g., if the original source files are unavailable) must reject affected documents until re-ingested. This is a real migration cost.
- **Storage cost:** Storage grows proportionally with block and edge count. For typical documents this is modest, but pathologically large documents (thousands of blocks) will have a measurable storage footprint.
- **Complexity cost:** Two structural representations (graph and segments) coexist. Developers must understand which is canonical and when to use which. The invariant that graph and segments are written atomically must be maintained by all write paths.
- **Parser determinism is load-bearing:** The requirement that parsers produce identical output for identical input (at the same processing version) is no longer just a nice property — it is required for migration correctness. A non-deterministic parser makes graph reconstruction during migration unreliable.
- **Testing cost:** Write paths must be tested for atomic persistence of graph and segments. Migration paths must be tested for correct graph reconstruction from re-parse.

## Alternatives Considered

- **Keep graph transient; re-parse on demand.** Avoids schema change and storage cost. Requires the parser to be present and deterministic at query time. Adds latency to every structural query. Couples the read path to the parse path — if the parser is upgraded between ingest and query, the re-parsed graph may differ from the original. Unacceptable for a system that promises stable structural results.
- **Store BlockGraph as a serialized blob.** Avoids new relational structure. Simpler migration (one column addition). However, precludes relational queries over blocks and edges (filtering by block kind, traversing edges, joining blocks to segments). Every consumer must deserialize the entire graph to access any part of it. Rejected because queryability of individual blocks and edges is a core motivation.
- **Store only reading order without the full edge graph.** Simpler schema with lower storage cost. Loses relation-typed edges entirely. Structural queries that need edge semantics (containment, reference, dependency) become impossible without re-parsing. Rejected because edge relationships are the primary value of persisting the graph.

## Non-Goals

- Introducing a graph query language or graph database. Querying persisted graph data is done through the existing storage interface.
- Storing rendering or layout information (visual coordinates, page positions).
- Persisting parser-internal AST nodes beyond the BlockGraph abstraction.
- Changing vector index persistence formats or segment identity derivation.
