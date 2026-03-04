# shiro (城) — Design Rules

Non-negotiable constraints for the shiro codebase.

## Architecture
- `shiro-core` defines domain types and port traits for external adapters (Parser, Embedder, VectorIndex).
- `shiro-core` has zero adapter dependencies. It must not import `shiro-store`, `shiro-index`, or `shiro-parse`.
- Internal infrastructure (SQLite, Tantivy) uses concrete types, not traits (see ADR-001 in ARCHITECTURE.md).
- CLI is a thin orchestration layer: wires adapters, dispatches commands, formats output.

## Data Model
- **Source of truth**: SQLite (`shiro.db`). Documents, segments, state machine, search cache.
- **Staged/promote protocol**: all mutations go through staging first; promotion to live is atomic. Incomplete staging dirs are cleaned on next start.
- **Provenance**: every block carries `canonical_text` (source-faithful archival) and `rendered_text` (normalized for indexing). The two must never be conflated.
- **Content addressing**: `DocId = blake3(content)`. Re-ingesting identical content is a no-op.

## Segmentation
- Segmenter is **structure-first**: primary segmentation follows block boundaries from the parser's structural output.
- Token-count heuristics are only a secondary split within oversized blocks, never the primary strategy.

## Error Handling
- All domain errors flow through `ShiroError`. Adapters map their internal failures into it.
- `ErrorCode` is the stable machine-readable contract. Adding a `ShiroError` variant without an `ErrorCode` mapping is a compile error.
- Exit codes per `docs/CLI.md`: 0 success, 2 usage, 10 parse, 11 index, 12 search, 20 store corrupt, 21 lock busy.

## CLI Contract
- JSON to stdout by default. `--format text` for human-readable output.
- Success: `{ ok: true, command, result, next_actions }`
- Error: `{ ok: false, command, error: { code, message }, fix?, next_actions }`
- See `docs/CLI.md` for the authoritative contract (see ADR-004 in ARCHITECTURE.md).

## Observability
- `tracing` spans per CLI command. `run_id` attached where applicable.
- Logs go to stderr only. stdout is reserved for structured output.

## Testing
- Invariants are enforced at construction time (e.g. `Span::new` rejects `start > end`).
- Property tests (`proptest`) for domain invariants.
- Integration tests against port trait impls, not concrete adapters.

## Future Hooks (not yet implemented)
- Benchmarking: trait methods are designed for easy instrumentation (no performance claims until measured).
- Async: async adapters behind a feature gate when needed. Core stays sync.
- Generational indices: atomic publish via staging dir rename + pointer swap.
- Vector embeddings: in-process model (ONNX) behind Embedder trait.
- SKOS taxonomy: DAG with closure maintenance.
- MCP server: codemode JS VM over stdio.
- Config persistence: config.toml read/write with fingerprinting.
