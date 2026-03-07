# ADR-008: Canonical Retrieval Field Model

**Status:** Proposed
**Date:** 2026-03-07

## Context

Retrieval results currently aggregate signals from multiple sources (BM25, vector, and future sources). Without a canonical retrieval field model, each source can expose slightly different result payloads, which increases coupling in consumers and makes explain output inconsistent.

## Decision

**Boundary:** This ADR defines the canonical retrieval result field model at the SDK boundary. It does not define internal storage schemas, ranking algorithms, or UI rendering.

A retrieval result MUST expose a stable core field set:

- `entry_point`: canonical location pointer for the hit
- `doc_id`: document identity
- `segment_id` or equivalent source-local locator
- `fused_rank` and `fused_score`
- `source_contributions`: per-source rank and raw score attribution
- `generation_snapshot`: active generation identifiers for contributing indices

All retrieval sources MUST normalize into this canonical model before returning results to CLI/MCP consumers.

### Architecture Invariants

- Retrieval responses crossing the SDK boundary MUST use the canonical field model.
- New retrieval sources may add source-specific metadata, but MUST NOT remove or redefine canonical fields.
- Explainability fields are mandatory, not optional.
- Canonical field semantics are stable across index generations.

### Deliberate Absences

- This ADR does not freeze wire-level JSON key casing or versioning strategy.
- This ADR does not define pagination semantics.
- This ADR does not define scoring calibration across queries.

## Consequences

- Consumers can depend on one stable retrieval schema.
- Adding new retrieval sources no longer requires consumer-specific branching for core fields.
- Schema evolution cost increases: incompatible field changes require explicit migration/versioning.

## Alternatives Considered

- Source-specific payloads only: simpler short-term, but creates long-term consumer coupling and explain inconsistency.
- Minimal schema with opaque metadata blob: lower immediate coordination, but weak contracts and poor static validation.

## Non-Goals

- Defining ranking algorithms.
- Defining storage schema for search result caches.
