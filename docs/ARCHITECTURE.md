# shiro (城) Architecture

**Status:** Living document
**Date:** 2026-03-07

## Purpose

This document describes shiro's stable architecture at a bird's-eye level: system shape, code map, boundaries, explicit invariants, and cross-cutting concerns.

Decision rationale and tradeoff analysis live in ADRs under `docs/adr/`.

## System Overview

shiro is a local-first knowledge engine for PDF and Markdown, delivered as a single Rust binary with two interfaces:

- JSON-only CLI
- MCP server over stdio (JSON-RPC)

Core flow:

1. Parse source files into a deterministic Document Graph IR.
2. Persist canonical and derived records in SQLite.
3. Build and publish searchable indices via generation-based activation.
4. Execute retrieval through SDK contracts and return explainable results.

## Code Map

Workspace crates and architectural role:

- `shiro-core`: domain types, IDs, errors, ports, invariants
- `shiro-store`: SQLite persistence, schema migration, document lifecycle state
- `shiro-index`: Tantivy BM25 indexing and search
- `shiro-parse`: Markdown/PDF parsers that produce IR + segmentation inputs
- `shiro-embed`: embedding adapters and vector index implementations
- `shiro-sdk`: orchestration, typed operations, retrieval contracts, DSL/spec execution
- `shiro-cli`: JSON envelope adapter and MCP command surface

## Architectural Boundaries

### Product Boundary

- One binary is the product boundary.
- CLI and MCP are adapters over SDK operations.
- No separate mandatory daemon is required.

### Canonical vs Derived Data Boundary

- SQLite is canonical state.
- Search indices (BM25/vector) are derived and rebuildable.
- Derived data must be promotable atomically and recoverable from canonical records.

### Representation Boundary

- Document Graph IR is the canonical structural representation.
- Segments are an operational retrieval view derived from canonical representation.
- Retrieval APIs expose stable retrieval primitives rather than storage internals.

### External Adapter Boundary

- Parser, Embedder, and VectorIndex are explicit ports.
- Provider-specific or runtime-specific implementations must stay below port boundaries.
- Core orchestration must not depend on concrete providers.

### Trust Boundary

- Every write carries provenance.
- Trust zones are assigned by origin and enforced by retrieval policy.
- Promotion across trust zones requires explicit policy-driven actions.

## Explicit Invariants

The following are architectural constraints, not optional behavior:

- IDs are content-addressed or generation-scoped according to domain type (`DocId`, `SegmentId`, `GenerationId`, `RunId`).
- Byte spans are half-open `[start, end)` and validated at creation.
- Document lifecycle follows `STAGED -> INDEXING -> READY` with failure and delete transitions; retrieval serves only ready documents.
- Parsing and segmentation behavior is tracked independently from content identity via processing fingerprints.
- Embedding identity is explicit and versioned; mixed incompatible vector identities are rejected.
- Generation-based publish is the activation model for indices.
- Retrieval fusion uses deterministic ordering and deterministic tie-breaking.
- All command responses are JSON envelopes on stdout (except shell completion script output).
- Write operations use single-writer locking; read paths remain lock-free.

## Cross-Cutting Concerns

### Determinism and Explainability

- Deterministic contracts exist at parse, index activation, and retrieval ordering boundaries.
- Retrieval outputs must remain explainable with source attribution and scoring provenance.

### Idempotency and Recovery

- Ingest and index activation are designed for retry safety.
- Recovery assumes canonical state in SQLite and rebuildability of derived indices.

### Configuration and Evolution

- Configuration is typed, versioned, validated, and migrated.
- Architectural decisions are durable under implementation refactors; ADRs capture rationale and consequences.

### Evaluation and Operations

- Benchmarking is part of release quality control.
- Observability, scale budgets, and corpus-based evaluation are architecture-level concerns tracked via ADRs.

## Related ADRs

Accepted ADRs:

- ADR-001: Document Graph IR over Tree Model
- ADR-002: SQLite as Source of Truth
- ADR-003: FlatIndex as Correctness Baseline
- ADR-004: Processing Fingerprints Separate from Content Identity
- ADR-005: Generation-Based Index Publish
- ADR-006: Persist the Document Graph as a First-Class Stored Representation
- ADR-007: Treat EntryPoint as the Primary Retrieval Result
- ADR-011: Embedding Providers Are Adapters
- ADR-012: Canonical Embedding Contract and Fingerprint
- ADR-014: Hybrid Retrieval Contract
- ADR-020: Every Write Has Provenance Metadata
- ADR-021: Partition Memory into Trust Zones
- ADR-027: Benchmarking Is a Release Gate
- ADR-029: Configuration Is Typed, Versioned, and Migrated
- ADR-031: npm Distribution Strategy
