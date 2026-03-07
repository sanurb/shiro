# ADR-016: PDF Parsing Must Produce Structured Output with Confidence Signals

**Status:** Proposed
**Date:** 2026-03-07

## Context

PDF extraction quality varies significantly by document type and parser behavior. Downstream retrieval and trust decisions require visibility into parse reliability.

## Decision

**Boundary:** This ADR governs parser output contract for PDF ingestion. It does not mandate a specific parser engine.

PDF parsing MUST emit:

- Structured block output compatible with canonical document representation
- Reading-order information required by retrieval/context expansion
- Confidence signals sufficient to identify low-reliability extraction paths

Confidence signals are metadata for downstream policy and observability; they do not replace structural validation.

### Architecture Invariants

- PDF parser output MUST satisfy canonical IR validation constraints.
- Missing confidence metadata for PDF parses is a contract violation.
- Low-confidence parses remain ingestible but must be identifiable.
- Confidence metadata MUST be provenance-linked to the parse event.

### Deliberate Absences

- This ADR does not define one confidence scoring formula.
- This ADR does not define UI handling for low-confidence content.
- This ADR does not enforce automatic quarantine behavior.

## Consequences

- Retrieval and enrichment can apply policy-aware handling for uncertain text.
- Parser implementations bear additional metadata responsibilities.
- Evaluation pipelines must validate confidence signal quality.

## Alternatives Considered

- No confidence metadata: simpler parser outputs but no reliability visibility.
- Hard fail low-confidence parses: safer corpus quality but poor recall and user control.

## Non-Goals

- Choosing OCR vendors.
- Defining remediation workflows for failed parse quality.
