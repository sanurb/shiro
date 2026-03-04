# shiro (城) — Design Rules

Non-negotiable constraints for the shiro codebase.

## Architecture
- Hexagonal: `shiro-core` defines ports (traits); adapters live in other crates.
- `shiro-core` has zero adapter dependencies. It must not import `shiro-store`, `shiro-index`, or `shiro-parse`.
- CLI is a thin adapter: dispatches to ports, formats output.

## Data Model
- **Source of truth**: SQLite manifest DB (planned). Until then, JSON manifests with atomic file writes.
- **Staged/promote protocol**: all mutations go through staging first; promotion to live is atomic. Incomplete staging dirs are cleaned on next start.
- **Provenance**: every block carries `canonical_text` (source-faithful archival) and `rendered_text` (normalized for indexing). The two must never be conflated.
- **Content addressing**: `DocId = blake3(content)`. Re-ingesting identical content is a no-op.

## Segmentation
- Segmenter is **structure-first**: primary segmentation follows block boundaries from the parser's structural output.
- Token-count heuristics are only a secondary split within oversized blocks, never the primary strategy.

## Error Handling
- All domain errors flow through `ShiroError`. Adapters map their internal failures into it.
- `ErrorCode` is the stable machine-readable contract. Adding a `ShiroError` variant without an `ErrorCode` mapping is a compile error.
- Exit codes are deterministic: 0 success, 2 usage, 3 not found, 4 invalid state, 5 internal.

## CLI Contract
- JSON is the default output format. Schema version is `"1.0"`.
- Success: `{ schemaVersion, ok: true, command, data, nextActions }`
- Error: `{ schemaVersion, ok: false, command, error: { code, message }, fix?, nextActions }`
- Output contracts are versioned. Breaking changes require a schema version bump.

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
- SQLite: manifest store migration from JSON to SQLite.
