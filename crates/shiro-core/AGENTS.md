# shiro-core

Pure domain crate. Zero adapter dependencies. Every other crate depends on this; it depends on nothing workspace-internal.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add/change domain types | `src/ir.rs` | Document, BlockGraph, Block, Edge, Segment, BlockIdx |
| Add error variant | `src/error.rs` | Add to `ShiroError` enum AND map in `ErrorCode` (both `as_str` + `exit_code`) |
| Change ID scheme | `src/id.rs` | DocId, SegmentId, VersionId, RunId — all with prefix validators |
| Concept/taxonomy IDs | `src/taxonomy.rs` | ConceptId — content-addressed `con_<hash>` from scheme URI + label |
| Add external adapter port | `src/ports.rs` | Only for truly pluggable adapters (NOT SQLite/Tantivy) |
| Change config/paths | `src/config.rs` | `ShiroHome` manages `~/.shiro` layout |
| Change state machine | `src/manifest.rs` | DocState transitions, RunManifest |
| Change span logic | `src/span.rs` | Half-open `[start, end)` with proptest coverage |
| Write locking | `src/lock.rs` | Single-writer PID file lock |
| Parse loss tracking | `src/ir.rs` | ParseLoss, LossKind enum |

## KEY TYPES

| Type | File | Role |
|------|------|------|
| `Document` | `ir.rs` | Core IR: canonical_text (single coordinate space), BlockGraph, metadata |
| `BlockGraph` | `ir.rs` | Arena (`Vec<Block>`) + `Vec<Edge>` + `reading_order: Vec<BlockIdx>`. Has `validate()` → 6 `IrViolation` types incl. cycle detection |
| `Segment` | `ir.rs` | Derived from Document by segmenter. Not stored in Document |
| `ParseLoss` | `ir.rs` | Records fidelity losses during parse. `kind: LossKind` + context |
| `LossKind` | `ir.rs` | Image, Table, Math, Media, Layout, Encoding, Other |
| `Span` | `span.rs` | Half-open byte range into canonical_text. Invariant: `start <= end` |
| `DocId` | `id.rs` | `blake3(content)` → `doc_<hex>`. Newtype over String |
| `SegmentId` | `id.rs` | `blake3(doc_id:index)` → `seg_<hex>` |
| `VersionId` | `id.rs` | Content-addressed `ver_<hex>` |
| `RunId` | `id.rs` | Timestamp-based `run_<secs>.<nanos>` (NOT content-addressed) |
| `ConceptId` | `taxonomy.rs` | `blake3(scheme_uri + label)` → `con_<hex>` |
| `ShiroError` | `error.rs` | 19 variants, each mapped to `ErrorCode` |
| `ErrorCode` | `error.rs` | Dual: `as_str()` for JSON + `exit_code()` for CLI |
| `DocState` | `manifest.rs` | State machine: Staged→Indexing→Ready/Failed, any→Deleted |
| `ShiroHome` | `config.rs` | Path precedence: explicit arg > `SHIRO_HOME` env > `~/.shiro` |

## PORT CONTRACTS (ports.rs)

| Port | Impls | Contract |
|------|-------|----------|
| `Parser` | MarkdownParser, PdfParser (shiro-parse), DoclingParser (shiro-docling) | `name()` + `version()` + `parse()`. MUST be deterministic: identical input → identical output |
| `Embedder` | HttpEmbedder (shiro-embed) | `embed()`/`embed_batch()`/`dimensions()`. MUST be deterministic: identical input → identical embedding |
| `VectorIndex` | FlatIndex (shiro-embed) | `upsert()`/`search()`. MUST be idempotent on upsert, thread-safe (`&self` concurrent calls) |

## LOCK CONVENTION (lock.rs)

- Single-writer PID file lock (`write.lock`)
- Read operations do NOT require lock
- Only write operations acquire `WriteLock`
- `WriteLock` auto-released on drop
- Returns `ShiroError::LockBusy` if another process holds lock

## INVARIANTS

- `Span::new()` returns `Result` — construction enforces `start <= end`
- `DocId::from_stored()` / `SegmentId::from_stored()` validate prefix — reject malformed IDs at boundary
- `ErrorCode` mapping is exhaustive by construction — adding a `ShiroError` variant without `ErrorCode` mapping is a compile error
- `DocState` serializes as `SCREAMING_SNAKE_CASE` JSON strings
- All paths use `camino::Utf8PathBuf` — never `std::PathBuf`
- `BlockGraph::validate()` checks 6 violation types: SpanOutOfBounds, InvalidReadingOrderIndex, ReadingOrderIncomplete, ReadingOrderDuplicate, EdgeOutOfBounds, CycleDetected
