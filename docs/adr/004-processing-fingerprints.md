# ADR-004: Processing Fingerprints Separate from Content Identity

**Status:** Accepted
**Date:** 2026-03-07

## Context

DocId is a pure content address: identical source bytes always resolve to the same identifier. This stability is essential — DocId is referenced by search results, taxonomy assignments, enrichments, and generation records. It must not change when nothing about the source content has changed.

When a parser or segmenter is upgraded, the resulting segments may differ even though the raw content is unchanged. Without separate version tracking, the system cannot detect that stored segments are stale and need reprocessing.

Incorporating processing version into DocId would solve detection but at an unacceptable cost: every pipeline upgrade would produce new DocId values, breaking all referential integrity across every table that references a document.

## Decision

**Content identity** (a stable identifier derived solely from a document's raw bytes, unchanged across processing pipeline upgrades) and **processing identity** (a record of which parser and segmenter versions produced the current stored segments) are separate concerns.

**Boundary:** This decision governs how content identity and processing identity relate to each other. It does not govern how reprocessing is scheduled, how enrichment or embedding versions are tracked, or how downstream indices detect staleness.

- **DocId** is the content identity. It is derived exclusively from the document's raw bytes and is stable across all processing pipeline changes.
- **ProcessingFingerprint** is the processing identity. It captures the full processing pipeline version: the parser identity, the parser version, and the segmenter version. It is stored alongside each document record.
- On ingestion, the current ProcessingFingerprint is written with the document. At reindex time, any document whose stored fingerprint does not match the currently active pipeline fingerprint is flagged for re-ingestion.

**What is canonical:** DocId is the canonical, immutable identity of a document's content.

**What is derived:** ProcessingFingerprint is metadata about how that content was most recently processed. Segments are derived from the combination of content and processing pipeline.

**What is allowed:** Consumers may compare a stored ProcessingFingerprint against the current pipeline fingerprint to determine staleness. Consumers may use DocId for stable cross-references.

**What is forbidden:** DocId must never incorporate processing version information. ProcessingFingerprint must never be used as a document identity or foreign key.

### Architecture Invariants

- Two documents with identical content but different processing versions share the same DocId but have different ProcessingFingerprints. The most recent processing wins on upsert.
- Stale processing is detectable (fingerprint mismatch) but not automatically remediated. Detection is the system's responsibility; scheduling reprocessing is the caller's responsibility.
- If a parser changes its behavior without bumping its version, the system has no way to detect the resulting silent staleness. This is a known failure mode. Correctness depends on parsers faithfully incrementing their version when output-affecting behavior changes. The system cannot enforce this invariant — it is a contract obligation on parser implementations.
- Referential integrity across all tables that reference DocId is preserved across processing pipeline upgrades because DocId never changes.

### Deliberate Absences

- Enrichment pipeline versions, embedding model versions, and taxonomy versions are not tracked by ProcessingFingerprint. Those are separate concerns for separate decisions.
- Which GenerationId produced a given set of segments is not tracked here (see ADR-005).
- Automatic reprocessing triggered by fingerprint mismatch is not specified. Detection is provided; scheduling is left to the CLI/SDK layer.
- No mechanism is provided to detect silent staleness caused by parser behavior changes without version bumps. Mitigation (e.g., content-based output hashing) is a possible future extension.

## Consequences

- **Staleness detection:** Parser or segmenter upgrades are detectable by comparing stored fingerprint against the current pipeline fingerprint, without altering DocId. Users who upgrade shiro know exactly which documents need reprocessing and can target re-ingestion rather than rebuilding the entire corpus.
- **Referential integrity preserved:** All downstream references (search results, taxonomy assignments, enrichments, generation records) remain valid across pipeline upgrades.
- **Targeted reprocessing:** Only documents with a fingerprint mismatch need re-ingestion, reducing upgrade cost from O(corpus) to O(changed-documents).
- **Parser contract burden:** Every parser implementation must maintain a stable, versioned identity. A parser that changes output-affecting behavior without bumping its version introduces silent staleness that no system-level mechanism can detect. This is a real operational risk.
- **Complexity cost:** Two distinct identity concepts (content identity and processing identity) must be understood and correctly used by all code that interacts with documents. Misuse (e.g., using fingerprint as identity) would break referential integrity.
- **Product outcome:** Users upgrading shiro can run a staleness check and see exactly which documents need reprocessing, rather than guessing or reprocessing everything.

## Alternatives Considered

- **Version embedded in DocId:** Appending parser/segmenter version to the content hash input would make upgrades detectable, but every pipeline upgrade would invalidate all downstream references. Every search result, taxonomy assignment, and enrichment record would need migration on every parser release. The referential integrity cost is prohibitive.
- **No version tracking:** Ignoring parser version entirely means upgraded parsers silently serve stale segments until manual re-ingestion of every document. There is no automatic staleness detection, no way to target reprocessing, and no way for users to know whether their index reflects the current pipeline. Operationally unacceptable for a system that expects parser evolution.

## Non-Goals

- Tracking enrichment pipeline versions or embedding model versions — that is a separate concern.
- Tracking which GenerationId produced a given segment (see ADR-005).
- Automatic reprocessing scheduling triggered by fingerprint mismatch — detection only; scheduling is a CLI/SDK concern.
