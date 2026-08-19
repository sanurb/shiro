# ADR-038: External content and model output enter through policy gates

**Status:** Accepted  
**Date:** 2026-08-16

## Context

URL acquisition and model enrichment add useful workflow breadth but cross local trust boundaries. Unbounded fetches expose SSRF/resource risks; immediately accepted model concepts mix generated organization with source truth.

## Decision

URL acquisition accepts HTTP(S) only, requires HTTPS by default, rejects embedded credentials, and uses a custom resolver that fails the request if any DNS answer is private, local, link-local, multicast, documentation, benchmarking, or otherwise non-routable. Redirects are followed manually and revalidated. Total time, bytes, and redirect count are bounded. PDF magic or valid UTF-8 text is required. Requested/final URLs, redirect chain, MIME evidence, detected signature, byte count, and content hash are committed atomically with CAS source bytes and the canonical document aggregate.

Model providers do not write taxonomy or retrieval metadata directly. Their output is accepted only as an attributed `PROPOSED` record carrying provider, model, actor, data region, retention policy, consent reference, labeled concepts, and confidence. Every concept has a preferred text label and therefore a text fallback. Explicit promotion requires a resolving actor and approval reference. Promotion never overwrites an existing manual or other assignment. Rejection reverses only assignments inserted by that proposal; concept records may remain because they can be shared.

MCP write operations require three authority signals: server startup with `--allow-writes`, per-call actor identity, and per-call approval identity. Authorization and success/failure are append-only audited under a generated run ID. Read/write authority is discoverable in operation specs.

## Consequences

- Network acquisition is safe by default but does not support authenticated URLs.
- DNS is validated at the resolver actually used for connection, avoiding a separate check/connect resolution gap.
- Provider calls remain adapter/account concerns; Shiro can import their output without inventing region, retention, or consent facts.
- Proposed model output cannot affect default retrieval filters.
- Hosts retain responsibility for presenting human confirmation UI; Shiro enforces and records the resulting authority token.
