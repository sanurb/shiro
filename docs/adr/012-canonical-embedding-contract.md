# ADR-012: Canonical Embedding Contract and Fingerprint

**Status:** Accepted
**Date:** 2026-03-07

## Context

Embeddings are produced by implementations of the `Embedder` trait (`HttpEmbedder`, `StubEmbedder` in shiro-embed). `FlatIndex` stores vectors keyed by `SegmentId` in a JSONL file but records no provenance about which model, provider, or configuration produced those vectors.

`ProcessingFingerprint` (ADR-004) captures `parser_name`, `parser_version`, and `segmenter_version` in the `documents.fingerprint` column of shiro-store, establishing parser-side determinism. No equivalent exists on the embedding side.

This creates a silent failure mode: if the active `HttpEmbedderConfig { base_url, model, api_key, dimensions }` changes—model upgrade, provider swap, dimension change—existing vectors in `FlatIndex` become semantically incompatible with all newly produced vectors. Cosine similarity computations proceed without error, producing garbage rankings in hybrid RRF fusion (`Σ 1/(60 + rank_S(s))`). No mechanism detects or blocks this.

## Decision

Every embedding MUST be tied to an `EmbeddingFingerprint` struct containing:

- `provider`: string name identifying the embedding backend (e.g., `"openai"`, `"ollama"`)
- `model`: model identifier as passed to the provider (e.g., `"text-embedding-3-small"`)
- `dimensions`: `u32` — output vector dimensionality
- `normalization`: enum or string describing the normalization policy applied before storage (e.g., `L2`, `None`)
- `truncation_policy`: max input tokens and behavior at limit (e.g., `truncate_end`, `error`)
- `chunk_policy`: how a `Segment` maps to embedding input (e.g., `full_text`, `title_prefix`)
- `fingerprint_hash`: blake3 hash of the non-secret fields above, encoded as a fixed-length hex string

The fingerprint is derived from the non-secret subset of `HttpEmbedderConfig`—`base_url`, `model`, `dimensions`—plus the policy fields. `api_key` is excluded from the hash.

Storage requirements:

- `FlatIndex` JSONL format MUST include a header record (first line) containing the serialized `EmbeddingFingerprint` for the index.
- shiro-store MAY persist the `fingerprint_hash` in a dedicated column on the `segments` table or a new `embedding_meta` table to enable SQL-level compatibility checks without loading the vector file.

On any operation that reads or writes vectors (`VectorIndex::upsert`, `VectorIndex::search`), the active `EmbeddingFingerprint` MUST be compared against the stored fingerprint. A mismatch MUST return `ShiroError` and block the operation. Reembedding is the only resolution path; silent mixed-model search is not permitted.

The `Embedder` trait MUST expose a `fingerprint() -> EmbeddingFingerprint` method. `StubEmbedder` returns a deterministic stub fingerprint suitable for tests.

## Consequences

- `FlatIndex` becomes self-describing: any reader can verify vector compatibility without external metadata.
- Model upgrades are detectable at the point of first `upsert` or `search` after config change. The system returns a typed error, not degraded output.
- Fingerprint mismatch blocks hybrid search rather than silently producing mixed-model rankings in RRF fusion.
- The JSONL format gains a mandatory header line; existing index files without a header are treated as version-0 and rejected or migrated on open.
- `HttpEmbedderConfig` requires no new fields; the fingerprint is computed from the existing non-secret fields.
- `StubEmbedder` acquires a deterministic `EmbeddingFingerprint`, making test assertions on fingerprint equality possible.

## Alternatives Considered

- **Trust the operator to reindex manually**: No enforcement mechanism. A missed reindex produces undetectable ranking corruption. Rejected.
- **Embed model name in `SegmentId`**: `SegmentId = blake3(doc_id:index)` is a content address tied to document identity, not embedding provenance. Coupling model identity to segment identity breaks content-addressing and makes the same segment appear under different IDs as models change. Rejected.
- **Version the entire vector index opaquely**: A single index-level version number cannot distinguish per-vector provenance differences. Offers no granularity for partial migration or diagnostic output. Rejected.

## Non-Goals

- Tracking per-token attention weights, internal model activations, or any sub-vector embedding state.
- Supporting mixed-model indices: all vectors within a single `FlatIndex` instance MUST share one `EmbeddingFingerprint`. Multi-model scenarios require separate index instances.
- Automatic reembedding on fingerprint mismatch: detection is in scope; automated remediation is left to the CLI layer and operator workflow.
- Encrypting or redacting `api_key` beyond its existing exclusion from the fingerprint hash.
