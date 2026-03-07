# ADR-003: FlatIndex as Correctness Baseline

**Status:** Accepted
**Date:** 2026-03-07

## Context

Vector search is a required capability. Approximate Nearest Neighbor (ANN) libraries (HNSW, IVF-flat, ScaNN) offer sub-linear query time but introduce recall variance: results differ from exact cosine ranking by design. Without a ground-truth implementation, it is impossible to measure or enforce recall quality for any future ANN backend.

`shiro-embed::FlatIndex` is a brute-force cosine similarity implementation that satisfies the `VectorIndex` trait (`shiro-core::ports`). It persists vectors to JSONL with blake3 checksums for integrity, is Send+Sync, and its `upsert`, `search`, `delete`, `delete_by_doc`, `count`, `dimensions`, and `flush` operations are all idempotent. Embeddings are produced by types implementing the `Embedder` trait; the production implementation is `HttpEmbedder` (OpenAI-compatible `/v1/embeddings`, configured via `HttpEmbedderConfig`).

## Decision

`FlatIndex` is the ground-truth `VectorIndex` implementation. Any future ANN backend added behind the `VectorIndex` trait must achieve recall >= 0.95 against `FlatIndex` results on the project's benchmark suite before it can replace `FlatIndex` in production.

## Consequences

- Vector search is O(n) in the number of indexed vectors. This is acceptable at the target scale of thousands of documents and tens of thousands of segments.
- `StubEmbedder` (test-only) enables unit tests of `VectorIndex` behavior without a live embedding endpoint.
- The `VectorIndex` trait boundary means ANN backends can be introduced without changing `shiro-sdk` or `shiro-cli`.
- blake3 checksums on JSONL entries catch silent corruption before a bad vector contaminates search results.
- The 0.95 recall threshold is a hard gate: an ANN backend that is faster but produces materially different rankings is not acceptable without explicit re-evaluation.

## Alternatives Considered

- **Start with HNSW (e.g., via `hnswlib` or `usearch`)**: Would be faster for large corpora, but provides no ground truth to validate recall against. Any recall regression would be undetectable without a reference implementation. Rejected as the initial implementation; acceptable as a future backend once `FlatIndex` establishes the baseline.
- **No vector search**: Removes semantic retrieval capability. Rejected; hybrid BM25 + vector search (RRF fusion in `shiro-sdk`, k=60) is a core product capability.

## Non-Goals

- Not optimizing for million-scale vector corpora in v1. Target scale is thousands of documents.
- No GPU acceleration. `FlatIndex` runs on CPU; this is sufficient at target scale.
- No distributed vector index. All vectors are local to `ShiroHome`.
