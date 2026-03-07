# ADR-001: Document Graph IR over Tree Model

**Status:** Accepted
**Date:** 2026-03-07

## Context

PDFs routinely produce structures that break tree assumptions: overlapping spans, footnotes that interrupt the main reading flow, margin annotations, and cross-references that link non-adjacent blocks. A DOM-like tree cannot represent these faithfully without lossy normalization.

Shiro represents parsed documents as a directed graph (`BlockGraph` in `shiro-core::ir`). Each node is a `Block` containing one or more `Segment`s; edges (`Edge`) carry a typed relationship (e.g., reading order, hierarchy, reference). A deterministic total reading order is maintained explicitly as `reading_order: Vec<BlockIdx>` on `BlockGraph`, decoupled from the edge set. `BlockGraph::validate()` enforces structural invariants at construction time, detecting six `IrViolation` categories including cycle detection.

## Decision

Use `BlockGraph` with an explicit `reading_order` vector as the canonical intermediate representation. Do not use a tree.

## Consequences

- Blocks are addressable by `(DocId, BlockIdx, Span)` — precise hit attribution for search results.
- Segmentation (`shiro-parse::segment_document`) and context expansion are deterministic because reading order is a stored total order, not inferred from edge traversal.
- `BlockGraph::validate()` makes invalid IRs unrepresentable past the parse boundary; violations surface at ingest time, not at query time.
- More implementation complexity than a flat list or tree, but the model is faithful to real document structure without silent data loss.
- Parsers implementing the `Parser` trait (`shiro-core::ports`) must produce a valid `BlockGraph`; the trait contract enforces determinism.

## Alternatives Considered

- **DOM-like tree**: Cannot represent overlapping spans or multi-parent references without ad-hoc workarounds. Rejected because it forces lossy normalization at parse time.
- **Flat chunk list**: Simple but loses all structural information (section hierarchy, footnote provenance, cross-references). Rejected because structure is required for faithful context expansion.

## Non-Goals

- Shiro is not a general-purpose graph database. No graph query language is exposed.
- No graph mutation API is provided after `BlockGraph` is sealed by `validate()`.
- No rendering or layout pipeline — `BlockGraph` is a semantic IR only.
