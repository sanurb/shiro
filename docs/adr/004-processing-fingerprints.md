# ADR-004: Processing Fingerprints Separate from DocId

**Status:** Accepted
**Date:** 2026-03-07

## Context

`DocId` is computed as `blake3(raw_content_bytes)` with a `doc_` prefix, making it purely content-addressed. This is intentional: identical content always resolves to the same identifier, enabling deduplication and stable cross-references in `search_results`, taxonomy assignments (`doc_concepts`), and `enrichments`.

When a parser (e.g., `MarkdownParser`, `PdfParser`) or the segmenter (`segment_document()`) is upgraded, the resulting segments may differ even though the raw content bytes have not changed. Without separate version tracking, the system has no way to detect that stored segments are stale and reprocessing is required.

Incorporating parser or segmenter version into `DocId` would solve detection but at an unacceptable cost: every upgrade would produce new `DocId` values, breaking all referential integrity across `search_results`, `doc_concepts`, `enrichments`, and `generations` tables.

## Decision

Processing version is tracked in a dedicated `fingerprint` column on the `documents` table, stored as `ProcessingFingerprint { parser_name, parser_version, segmenter_version }` defined in `shiro-core`.

`DocId` remains content-only (`blake3(raw_content_bytes)`), unchanged by parser or segmenter upgrades.

On ingestion, the current `ProcessingFingerprint` is written alongside the document. At reindex time, documents whose stored fingerprint does not match the fingerprint of the currently active parser are flagged for re-ingestion.

## Consequences

- Parser or segmenter upgrades are detectable by comparing stored fingerprint against the current `ProcessingFingerprint` without altering `DocId`.
- Referential integrity across `search_results`, `doc_concepts`, `enrichments`, and `generations` is preserved across upgrades.
- Reprocessing is targeted: only documents with a fingerprint mismatch need re-ingestion, not the entire corpus.
- The `Parser` trait contract (`name()` + `parse()`) must return a stable, versioned `parser_name` and `parser_version`; implementations that change behavior without bumping version will cause silent staleness — this is a caller responsibility.
- Two documents with identical content but processed by different parser versions will share a `DocId` but differ in `fingerprint`; the latest fingerprint wins on upsert.

## Alternatives Considered

- **Version in DocId**: Appending parser/segmenter version to the hash input would make upgrades detectable but invalidates all downstream references (`search_results`, `doc_concepts`, `enrichments`) on every upgrade. Rejected.
- **No versioning**: Ignoring parser version entirely means upgraded parsers silently serve stale segments until manual reingestion. No automatic staleness detection. Rejected.

## Non-Goals

- Tracking enrichment pipeline versions or embedding model versions in `ProcessingFingerprint` — that is a separate concern addressed in ADR-017.
- Tracking which `GenerationId` produced a given segment (see ADR-005).
- Automatic reprocessing scheduling triggered by fingerprint mismatch — detection only, scheduling is a CLI/SDK concern.
