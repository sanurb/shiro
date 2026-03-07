# ADR-001: Document Graph IR over Tree Model

**Status:** Accepted
**Date:** 2026-03-07

## Context

PDFs routinely produce structures that break tree assumptions: overlapping spans, footnotes that interrupt the main reading flow, margin annotations, and cross-references that link non-adjacent blocks. A DOM-like tree cannot represent these without lossy normalization — silently discarding relationships that the source document actually contains.

Shiro needs an intermediate representation (IR) for parsed documents that preserves these relationships so that downstream consumers (search, context expansion, explain) operate on a **faithful** (preserving the structural relationships present in the source document without lossy normalization) representation of what was actually in the document.

The representation must also be **deterministic** (identical input bytes produce identical output bytes across invocations, given the same processing version) so that segmentation, indexing, and search attribution are reproducible.

## Decision

The **BlockGraph** is the canonical intermediate representation for parsed documents. It is a directed graph where nodes are blocks (containing one or more segments) and edges carry typed relationships (reading order, hierarchy, reference). A deterministic total reading order is maintained as an explicit sequence on the graph, decoupled from the edge set.

This decision governs what Shiro considers a valid parsed document. It does not govern how parsers produce the graph, how indices consume it, or how segments are stored — only the shape of the representation that crosses the parse boundary.

**Boundary:** This ADR decides the IR structure between parsing and all downstream consumers (indexing, search, context expansion). It does not decide storage format, serialization, or query semantics.

**What is canonical:** The BlockGraph is the authoritative representation of a parsed document's structure. When the graph and any derived representation (segments in storage, index entries, search results) disagree, the graph is correct and the derived representation must be rebuilt.

**What is derived:** Segment storage, full-text index entries, vector embeddings, and search results are all derived from the BlockGraph. They do not carry structural authority.

**What is allowed:** Consumers may rely on reading order being a total order over all blocks. Consumers may address content by (DocId, block position, span) for precise hit attribution. Parsers may produce any valid graph topology — the IR imposes no constraint on edge patterns beyond structural validity.

**What is forbidden:** Consumers must not infer reading order from edge traversal. Consumers must not assume a tree structure (single parent per block). No mutation of the graph is permitted after it passes construction-time validation.

### Architecture Invariants

- The BlockGraph is the sole structural authority for a parsed document. If a segment in storage or an index entry contradicts the graph, the graph wins and the downstream representation is stale.
- Reading order is an explicit, stored sequence — never inferred, never reconstructed from edges. Consumers may depend on its stability for a given document version.
- Construction-time validation rejects structurally invalid graphs (cycles, dangling references, missing reading order entries) at ingest time. This is runtime validation, not a type-level guarantee — code that bypasses validation can still construct invalid graphs, but the standard parse pipeline will reject them before they reach any consumer.
- A block's position in reading order is stable for a given document version. Re-parsing the same input bytes with the same processing version produces the same reading order.

### Deliberate Absences

- **Serialization format** is not decided. The graph may be stored as any format that roundtrips faithfully.
- **Edge type vocabulary** is not frozen. New relationship types may be added without revising this ADR.
- **Cross-document references** are not supported. The graph represents a single document.
- **Graph query language** is not provided. Consumers traverse the graph programmatically.
- **Rendering or layout** is out of scope. The BlockGraph is a semantic IR, not a visual representation.

## Consequences

- **Precise search attribution.** Blocks are addressable by (DocId, block position, span), so search results can point to exactly where a match occurred — users see highlighted results in context, not just "this document matched."
- **Deterministic segmentation.** Because reading order is stored rather than inferred, segmentation and context expansion produce identical results on repeated runs. This eliminates a class of "flaky search results" bugs.
- **Faithful representation.** Overlapping spans, multi-parent references, and non-linear reading flows are preserved. Users searching documents with complex layouts (academic papers with footnotes, legal documents with margin annotations) get results that reflect actual document structure.
- **Higher implementation complexity.** A graph IR is more complex to implement and validate than a flat list or tree. Parsers must produce valid graphs, and every consumer must handle non-tree topologies. This is ongoing complexity cost in every parser and every downstream consumer.
- **Validation is a chokepoint.** Construction-time validation adds latency to every document ingest. Malformed documents surface errors at ingest rather than at query time — a trade-off of upfront cost for downstream reliability.
- **Migration cost for new parsers.** Any new parser (e.g., for EPUB, HTML) must produce a valid BlockGraph, which is a higher bar than producing a flat chunk list. Parser authors must understand graph construction and validation constraints.
- **Testing cost.** Graph-based IR requires test infrastructure for generating, validating, and comparing graphs — more complex than testing flat lists or trees.

## Alternatives Considered

- **DOM-like tree:** Cannot represent overlapping spans or multi-parent references without ad-hoc workarounds (e.g., synthetic wrapper nodes, reference-by-ID). Choosing this would force lossy normalization at parse time — footnotes that span multiple sections would be assigned to one parent arbitrarily, and cross-references would degrade to string labels. Rejected because structural fidelity is a core requirement.
- **Flat chunk list:** Simple to implement and sufficient for naive keyword search. Choosing this would mean no structural context expansion (can't walk to a footnote's parent section), no hierarchy-aware search ranking, and no precise sub-document attribution. Rejected because structure is required for faithful context expansion and precise hit attribution.
- **Ordered chunk list with overlap metadata:** A middle ground — flat list of chunks with sidecar metadata recording overlaps, hierarchy, and cross-references. This would be simpler to implement than a full graph and easier for consumers to iterate. However, the sidecar metadata would effectively be a graph encoded as a parallel data structure, adding indirection without reducing complexity. The overlap metadata would also need its own validation, duplicating the invariant-checking work. Rejected because the complexity savings are illusory — the graph structure exists either way, and encoding it directly is more honest and easier to validate.

## Non-Goals

- Shiro is not a general-purpose graph database. No graph query language is exposed.
- No graph mutation API is provided after construction-time validation seals the graph.
- No rendering or layout pipeline — the BlockGraph is a semantic IR only.
