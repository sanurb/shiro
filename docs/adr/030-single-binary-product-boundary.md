# ADR-030: Single Binary Is the Product Boundary

**Status:** Proposed
**Date:** 2026-03-07

## Context

Shiro supports CLI and MCP execution modes. Introducing mandatory sidecars or service fleets would materially change deployment complexity and local-first ergonomics.

## Decision

**Boundary:** This ADR defines packaging and runtime boundary for core product operation. It does not prohibit optional helper tooling.

The primary shiro product is a single binary that includes core ingestion/retrieval capabilities, CLI, and MCP modes.

Optional external services (embedding providers, optional orchestration helpers) may be integrated through adapter boundaries but are not required for baseline operation.

### Architecture Invariants

- Core commands and retrieval paths MUST function without a mandatory always-on daemon.
- CLI and MCP remain interfaces over the same SDK contracts.
- Packaging changes that introduce mandatory multi-process orchestration require explicit ADR revision.
- Single-binary operation remains a supported first-class deployment mode.

### Deliberate Absences

- This ADR does not ban optional remote providers.
- This ADR does not define installer/distribution channel specifics.
- This ADR does not define plugin packaging model.

## Consequences

- Local onboarding and operational surface stay small.
- Process-isolated scale-out patterns require deliberate future design.
- Binary size may grow as capabilities expand.

## Alternatives Considered

- Mandatory daemon + thin clients: richer central orchestration, higher operational burden.
- Split binaries per function: modular packaging, worse UX and version coordination.

## Non-Goals

- Defining npm installer architecture.
- Defining enterprise deployment topologies.
