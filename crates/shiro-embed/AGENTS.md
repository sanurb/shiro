# shiro-embed

Vector embedding implementations: storage + generation management.

| Type | Trait | File | Purpose |
|------|-------|------|---------|
| `FlatIndex` | `VectorIndex` | `src/flat.rs` (689 lines) | Brute-force cosine similarity, JSONL persistence, blake3 checksums |
| `HttpEmbedder` | `Embedder` | `src/http.rs` | OpenAI-compatible `/v1/embeddings` HTTP endpoint |
| `StubEmbedder` | `Embedder` | `src/stub.rs` | Returns zero vectors, test-only |
| `DeterministicStubEmbedder` | `Embedder` | `src/stub.rs` | Deterministic hash-based vectors, test-only |

## FLAT INDEX INTERNALS

### Dual insert paths (footgun)

- `upsert_with_doc(seg, doc_id, embedding)` — preferred, records real `DocId`
- `upsert(id, embedding)` — `VectorIndex` trait method, uses `"doc_unknown"` placeholder
- **ALWAYS** use `upsert_with_doc` when `DocId` is available

### Persistence

- `flush()` — writes all entries to JSONL, computes blake3 checksum, stores in `checksum` field
- `verify_checksum()` — re-hashes on-disk JSONL, compares to stored checksum
- `build_at()` — writes to staging dir; `promote_staging()` does atomic rename to final path

### Silent data loss on open

`open()` skips malformed lines and dimension-mismatch entries with `tracing::warn`.
No error returned — index starts with whatever parsed successfully.
Check logs if vector count seems low after reload.

### Generation tracking

`gen_id: u64` — monotonic index version. Incremented on rebuild via `build_at()`.
Callers use `gen_id()` to detect stale indexes.

## REQUIRED TRAIT CONTRACTS (from shiro-core ports.rs)

- **Embedder**: deterministic — identical input MUST produce identical output
- **VectorIndex**: idempotent upsert, all `&self` methods must be thread-safe
- Ranking: cosine similarity, stable tie-break on `segment_id`

## HttpEmbedder

Config via `HttpEmbedderConfig`: `base_url`, `model`, `api_key` (optional), `dimensions`.
Batches via `embed_batch()`. Single via `embed()` (delegates to batch).

## KNOWN ISSUES

- `flush()` uses `unwrap()` on `Vec<u8>` write (infallible but unguarded) — would panic if buffer ever replaced with fallible writer
- `open()` silently drops entries on dimension mismatch — no error surface to caller
