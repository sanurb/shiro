# ADR-012: Canonical Embedding Contract and Fingerprint

**Status:** Accepted
**Date:** 2026-03-07

## Context

Embeddings are produced by implementations of the Embedder trait (ADR-011). The vector index stores vectors keyed by segment identity but records no provenance about which model, provider, or configuration produced those vectors.

Processing fingerprints (ADR-004) capture parser-side identity (parser name, parser version, segmenter version), establishing **deterministic** (**deterministic**: identical input bytes produce identical output bytes across invocations, given the same processing version) processing detection on the parsing side. No equivalent exists on the embedding side.

This creates a silent failure mode: if the active embedding configuration changes — model upgrade, provider swap, dimension change — existing vectors become semantically incompatible with all newly produced vectors. Similarity computations proceed without error, producing garbage rankings in hybrid retrieval fusion. No mechanism detects or blocks this.

A more insidious variant: a provider silently changes model behavior while keeping the same model name (retraining, normalization changes, quantization). Model-name-only identity cannot detect this. The fingerprint must include enough configuration surface to catch detectable changes, while acknowledging that undetectable provider-side changes remain a residual risk.

## Decision

**Boundary:** This ADR governs how embedding provenance is recorded, compared, and enforced. It applies to every operation that reads or writes vectors in the index.

An **EmbeddingFingerprint** is defined as: a composite identity **derived** (**derived**: a representation computed from canonical data; rebuildable, not authoritative) from the non-secret embedding configuration, used to detect incompatible vectors.

An EmbeddingFingerprint MUST contain the following fields:

- **provider** — string name identifying the embedding backend (e.g., "openai", "ollama"). Distinguishes backends that may serve the same model name with different behavior.
- **model** — model identifier as passed to the provider (e.g., "text-embedding-3-small"). Combined with provider, this identifies the model uniquely within a deployment.
- **dimensions** — output vector dimensionality as a positive integer. A hard constraint: vectors of different dimensions cannot be compared.
- **normalization** — the normalization policy applied to vectors before storage (e.g., L2, none). Vectors normalized differently are incompatible even if produced by the same model.
- **truncation_policy** — maximum input token count and behavior at the limit (e.g., truncate at end, return error). Affects which content is represented in the vector.
- **chunk_policy** — how input text is prepared before embedding (e.g., full segment text, title-prefixed). Affects the semantic content of the vector.
- **fingerprint_hash** — a cryptographic hash of all non-secret fields above, encoded as a fixed-length hex string. This is the value used for fast equality comparison.

The hash is computed from the non-secret configuration fields listed above. Secret fields (API keys, authentication tokens) MUST be excluded from the hash.

**What is canonical:** The EmbeddingFingerprint stored with the index is the **canonical** (**canonical**: the representation that wins when others disagree; the authority from which all others are derived or rebuilt) record of what produced those vectors. When the active configuration's fingerprint disagrees with the stored fingerprint, the stored fingerprint is authoritative — the index was built with those parameters, and new vectors are incompatible.

**What is derived:** The active EmbeddingFingerprint is derived from the current embedder configuration at runtime.

**What is allowed:** The Embedder trait MUST expose a method to return its EmbeddingFingerprint. Implementations (including test doubles) MUST return a deterministic fingerprint for a given configuration.

**What is forbidden:**

- A fingerprint mismatch is a HARD ERROR, not a warning. On any operation that reads or writes vectors, the active EmbeddingFingerprint MUST be compared against the stored fingerprint. A mismatch MUST cause the operation to fail with a clear, typed error. The system MUST refuse to read or write vectors until the index is rebuilt.
- Mixed-fingerprint search is forbidden. All vectors in a single index MUST share one EmbeddingFingerprint.
- Silent mixed-model search is never permitted. Re-embedding is the only resolution path for a fingerprint mismatch.

### Architecture Invariants

- The fingerprint stored with the index MUST match the fingerprint of the active embedder configuration for any read or write operation to proceed. On any mismatch, the system MUST refuse to read or write vectors until the index is rebuilt with the current configuration.
- All vectors within a single index instance MUST share one EmbeddingFingerprint. Multi-fingerprint indices are forbidden; multi-model scenarios require separate index instances.
- An index without a stored fingerprint (legacy data predating this ADR) MUST be treated as incompatible and rejected or migrated on first access.
- Consumers MAY assume that if a vector operation succeeds, all vectors in the result were produced by the same embedding configuration.
- Consumers MUST NOT assume that fingerprint equality guarantees identical model behavior — undetectable provider-side changes (silent retraining, quantization changes) are a residual risk that the fingerprint cannot eliminate.

### Deliberate Absences

- The specific hash algorithm is not mandated. Implementations must use a cryptographic hash that produces a fixed-length output; the choice of algorithm is an implementation decision.
- The storage format for the fingerprint (JSON header, database column, separate file) is not specified. Only the requirement that the fingerprint be stored with the index and checked on every operation is mandated.
- Per-vector fingerprint tracking is not specified. This ADR requires index-level fingerprint consistency, not per-vector provenance.
- Automatic re-embedding on fingerprint mismatch is not specified. Detection and hard-error behavior are in scope; automated remediation is a CLI/operator workflow concern.
- Detection of undetectable provider-side changes (same model name, silently different behavior) is acknowledged as a residual risk and is not addressed. The fingerprint catches all configuration-visible changes but cannot guard against provider opacity.

## Consequences

- **Product outcome:** Users see a clear, actionable error when they change embedding models or providers instead of silently degraded search quality. The error directs them to re-embed their content.
- The vector index becomes self-describing: any reader can verify vector compatibility without external metadata.
- Model upgrades and provider changes are detectable at the point of first index access after configuration change. The system returns a typed error, not degraded output.
- **Migration cost:** Existing index files without a fingerprint header are incompatible and must be rebuilt. This is a one-time cost at adoption but affects all existing deployments.
- **Complexity cost:** Every Embedder implementation must correctly produce a fingerprint. The fingerprint comparison must be performed on every index operation, adding a check to the hot path. The fingerprint contract is a new invariant that all future Embedder implementations must satisfy.
- **Performance cost:** Fingerprint comparison on every operation adds negligible overhead (hash equality check). The real cost is re-embedding when configurations change, which is proportional to the size of the indexed corpus.
- **API surface cost:** The Embedder trait gains a new required method (fingerprint generation). This is a breaking change for existing Embedder implementations, which must be updated.
- **Testing cost:** Test doubles must produce deterministic fingerprints. Tests that create indices must include valid fingerprints.

## Alternatives Considered

- **Trust the operator to reindex manually.** No enforcement mechanism. A missed reindex produces undetectable ranking corruption — similarity scores look plausible but are meaningless across incompatible vector spaces. Choosing this alternative means accepting that any configuration change is a potential silent data corruption event.
- **Embed model identity in the segment identifier.** Segment identity is a content address tied to document identity, not embedding provenance. Coupling model identity to segment identity breaks content-addressing semantics and causes the same logical segment to appear under different identifiers as models change. This would cascade into invalidation of all segment-level references.
- **Single opaque index version number.** A monotonic version cannot distinguish which aspect of the configuration changed (model, dimensions, normalization). It offers no granularity for diagnostic output and makes it impossible to determine whether a specific change actually requires re-embedding. It also cannot detect rollbacks to a previous configuration.

## Non-Goals

- Tracking per-token attention weights, internal model activations, or any sub-vector embedding state.
- Supporting mixed-fingerprint indices. Multi-model scenarios require separate index instances.
- Automatic re-embedding on fingerprint mismatch. Detection and hard-error behavior are in scope; automated remediation is a CLI/operator workflow concern.
- Encrypting or redacting API keys beyond their existing exclusion from the fingerprint hash.
