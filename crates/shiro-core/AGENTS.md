# shiro-core

Pure domain crate. Zero adapter dependencies. Every other crate depends on this; it depends on nothing workspace-internal.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add/change domain types | `src/ir.rs` | Document, BlockGraph, Block, Edge, Segment, BlockIdx |
| Add error variant | `src/error.rs` | Add to `ShiroError` enum AND map in `ErrorCode` (both `as_str` + `exit_code`) |
| Change ID scheme | `src/id.rs` | DocId, SegmentId, RunId — all with prefix validators |
| Add external adapter port | `src/ports.rs` | Only for truly pluggable adapters (NOT SQLite/Tantivy) |
| Change config/paths | `src/config.rs` | `ShiroHome` manages `~/.shiro` layout |
| Change state machine | `src/manifest.rs` | DocState transitions, RunManifest |
| Change span logic | `src/span.rs` | Half-open `[start, end)` with proptest coverage |

## KEY TYPES

| Type | File | Role |
|------|------|------|
| `Document` | `ir.rs` | Core IR: canonical_text (single coordinate space), BlockGraph, metadata |
| `BlockGraph` | `ir.rs` | Arena (`Vec<Block>`) + `Vec<Edge>` + `reading_order: Vec<BlockIdx>` |
| `Segment` | `ir.rs` | Derived from Document by segmenter. Not stored in Document |
| `Span` | `span.rs` | Half-open byte range into canonical_text. Invariant: `start <= end` |
| `DocId` | `id.rs` | `blake3(content)` → `doc_<hex>`. Newtype over String |
| `SegmentId` | `id.rs` | `blake3(doc_id:index)` → `seg_<hex>` |
| `RunId` | `id.rs` | Timestamp-based `run_<secs>.<nanos>` (NOT content-addressed) |
| `ShiroError` | `error.rs` | 15 variants, each mapped to `ErrorCode` |
| `ErrorCode` | `error.rs` | Dual: `as_str()` for JSON + `exit_code()` for CLI. 10 stable + 5 extension |
| `DocState` | `manifest.rs` | State machine: Staged→Indexing→Ready/Failed, any→Deleted |
| `ShiroHome` | `config.rs` | Path precedence: explicit arg > `SHIRO_HOME` env > `~/.shiro` |
| `Parser` | `ports.rs` | Trait: `name()` + `parse()`. Only port with production impl (PlainTextParser) |
| `Embedder` | `ports.rs` | Trait: embed/embed_batch/dimensions. **No production impl yet** |
| `VectorIndex` | `ports.rs` | Trait: upsert/search. **No production impl yet** |

## INVARIANTS

- `Span::new()` returns `Result` — construction enforces `start <= end`
- `DocId::from_stored()` / `SegmentId::from_stored()` validate prefix — reject malformed IDs at boundary
- `ErrorCode` mapping is exhaustive by construction — adding a `ShiroError` variant without `ErrorCode` mapping is a compile error
- `DocState` serializes as `SCREAMING_SNAKE_CASE` JSON strings
- All paths use `camino::Utf8PathBuf` — never `std::PathBuf`
