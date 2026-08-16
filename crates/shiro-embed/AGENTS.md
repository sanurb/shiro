# shiro-embed

Embedding provider adapters and deterministic test doubles. Vector index implementations live in `shiro-index`; `FlatIndex` is re-exported here only for source compatibility.

| Type | Trait | File | Purpose |
|------|-------|------|---------|
| `HttpEmbedder` | `Embedder` | `src/http.rs` | OpenAI-compatible `/v1/embeddings` HTTP endpoint |
| `StubEmbedder` | `Embedder` | `src/stub.rs` | Returns zero vectors, test-only |
| `DeterministicStubEmbedder` | `Embedder` | `src/stub.rs` | Deterministic hash-based vectors, test-only |

## REQUIRED TRAIT CONTRACT (from shiro-core ports.rs)

- **Embedder**: deterministic — identical input MUST produce identical output
- Every implementation exposes an `EmbeddingFingerprint` covering provider, model, dimensions, normalization, truncation, and chunk policy
- Provider-specific behavior stays below the `Embedder` interface from ADR-011

## HttpEmbedder

Config via `HttpEmbedderConfig`: `base_url`, `model`, `api_key` (optional), `dimensions`.
Batches via `embed_batch()`. Single via `embed()` (delegates to batch).

## FlatIndex compatibility export

`shiro_embed::FlatIndex` remains available as a re-export so existing callers compile. New code should import `shiro_index::FlatIndex`, where vector storage and generation management now live.
