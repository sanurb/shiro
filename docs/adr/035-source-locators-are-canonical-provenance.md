# ADR-035: Source locators are canonical provenance

**Status:** Accepted
**Date:** 2026-08-16

## Context

Canonical block spans locate text within `Document::canonical_text`, but they do not locate evidence in the source artifact. Docling already supplies page numbers, bounding boxes, and page dimensions. Shiro currently discards those fields during translation.

Without persisted source locators, search and explain cannot identify the source page or region without reparsing. Parser-specific coordinate types cannot be exposed because Shiro owns the canonical document model.

ADR-006 deliberately excluded rendering and page information. This decision supersedes that exclusion while preserving ADR-006's canonical graph and span invariants.

## Decision

A block may contain zero or more parser-neutral `SourceLocator` values. Each locator contains:

- a validated one-based page number;
- an optional parser-native page region;
- the region's coordinate origin when known;
- optional page dimensions in the same coordinate space.

Source locators are persisted with the canonical `BlockGraph`. Canonical text spans remain the authoritative text coordinate system. Source locators are provenance projections and do not replace spans, reading order, or graph relations.

Adapters translate their private schemas at the parser seam. Third-party schema types must not appear in `shiro-core`, storage interfaces, SDK results, or generated schemas.

Parsers must not fabricate missing coordinates, dimensions, or origins. Unknown values remain absent or explicitly unspecified. Invalid locator data is rejected or recorded as parse loss; it must not be silently normalized into plausible coordinates.

## Invariants

- Page numbers are one-based and nonzero.
- Region coordinates and page dimensions are finite.
- Page dimensions, when present, are positive.
- Region values remain in the parser's declared coordinate space; Shiro does not assume normalization.
- Locator order is deterministic and follows parser provenance order.
- Replacing a block graph atomically replaces its locators.
- Loading a partial or invalid persisted locator fails as store corruption.
- Existing documents without locators remain valid and require no fabricated backfill.

## Consequences

- Search, read, and explain can later return source page and region evidence without reparsing.
- SQLite requires a one-to-many block-locator table and schema migration.
- Parser snapshots change when previously discarded provenance is retained.
- Parser fingerprints must change when locator translation behavior changes.
- Geometry-aware retrieval remains derived and optional; this decision does not add visual retrieval.

## Alternatives considered

### Store Docling provenance JSON

Rejected. It leaks an external schema into canonical storage and makes other parsers second-class.

### Store one locator per block

Rejected. A canonical block can derive from multiple pages or source regions.

### Normalize all coordinates

Rejected. Normalization without a known origin, unit, and page size fabricates precision and can make round trips lossy.

### Keep locators as derived data

Rejected. Reconstructing them requires the original parser and can change after parser upgrades.

## Non-goals

- OCR confidence calibration.
- Page rendering or image storage.
- Region-based retrieval.
- Cross-parser geometry reconciliation.
- A universal source-character coordinate system.
