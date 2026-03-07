# ADR-028: Official Evaluation Corpora Govern Retrieval Quality Measurement

**Status:** Proposed
**Date:** 2026-03-07

## Context

Benchmark gates require representative corpora. Ad hoc local datasets produce inconsistent and non-reproducible quality claims.

## Decision

**Boundary:** This ADR defines corpus governance for architectural evaluation. It does not define CI implementation details.

Shiro maintains one or more official, versioned evaluation corpora used for release-quality measurements. Corpus updates are controlled and versioned so benchmark comparisons remain meaningful over time.

### Architecture Invariants

- Quality claims used for release gating MUST reference official corpus versions.
- Corpus version used for any benchmark result MUST be recorded.
- Corpus changes MUST be reviewable and auditable.
- No default-switch decision for retrieval backends may rely on non-official corpora alone.

### Deliberate Absences

- This ADR does not define exact corpus contents.
- This ADR does not require one single corpus for all domains.
- This ADR does not define data labeling tooling.

## Consequences

- Retrieval quality decisions become reproducible and comparable.
- Corpus curation becomes an ongoing maintenance responsibility.
- Benchmark trend interpretation must account for corpus version changes.

## Alternatives Considered

- Developer-local corpora only: convenient but non-reproducible.
- Third-party benchmarks only: easy adoption, poor domain fit for shiro use cases.

## Non-Goals

- Defining model training datasets.
- Defining privacy/legal policy for corpus acquisition.
