# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-03-09

### Added

- **shiro-docling crate** — structured PDF parsing via Docling Python subprocess adapter. Translates DoclingDocument JSON to shiro IR with block-level type mapping, ParseLoss tracking for images/tables, and schema validation.
- **BlockGraph persistence** — BlockGraph stored as first-class representation in SQLite (ADR-006). Enables reliable block-level context expansion and reading order reconstruction.
- Block-level positions in search results and explain output: `block_idx` and `block_kind` now reference canonical graph positions directly.

### Changed

- **Breaking**: Search output now uses block-level positions instead of segment-derived positions. `SearchHit` fields `block_idx`, `block_kind`, `span_start`, `span_end` reference BlockGraph directly.
- Store schema upgraded from v5 to v6 (persists block position in search_results table).

### Fixed

- Store error in `crates/shiro-store/src/lib.rs`.

## [0.3.0] - 2026-03-07

### Added

- **MCP server** (Code Mode) — JSON-RPC 2.0 over stdio with two tools: `shiro.search` (spec discovery) and `shiro.execute` (DSL program execution).
- **DSL interpreter** — JSON AST with `let`, `call`, `if`, `for_each`, `return` nodes. Variable substitution via `$var.path.0.field`. Hard limits: max_steps=200, max_iterations=100, max_output_bytes=1MiB, timeout=30s.
- **Vector embedding infrastructure** (shiro-embed crate) — FlatIndex (brute-force cosine, JSONL persistence, blake3 checksums), HttpEmbedder (OpenAI-compatible endpoints), StubEmbedder + DeterministicStubEmbedder for tests.
- **EmbeddingFingerprint** type (ADR-012) — fingerprint sidecar for generation-managed vector indices.
- **EntryPoint-based structured retrieval** (ADR-007) — search results return block-level positions with scores, context windows, and reading order.
- **Hybrid search scaffold** — RRF fusion (k=60) for BM25 + vector. Currently BM25-only; vector path ready but not wired.
- **Context expansion** — `--expand` flag with `--max-blocks` (default 12) and `--max-chars` (default 8000) for retrieving surrounding blocks.
- **Enrichment** — `shiro enrich` command with heuristic provider (title, summary, tags from content analysis).
- **Capabilities command** — `shiro capabilities` returns structured feature status, command list, parser list, ID schemes.
- **Processing fingerprints** (ADR-004) — parser_name, parser_version, segmenter_version persisted on every add/ingest.
- **Generation tracking** — GenerationId for FtsIndex and FlatIndex, staging build + atomic promote.
- **Explain command** — `shiro explain <result_id>` with retrieval_trace (pipeline, stages, fusion details).
- Store schema v5 with search_results persistence (block_idx, block_kind, scores).
- Spec registry with schemars-derived JSON Schemas for all SDK operations.
- 15 MCP integration tests, hybrid search tests, entry-point explain tests.

## [0.2.0] - 2026-03-04

### Added

- CLI flags per `docs/CLI.md` contract: `--parser`, `--enrich`, `--tags`, `--concepts`,
  `--fts-only`, `--follow` (add/ingest); `--expand`, `--max-blocks`, `--max-chars`,
  `--tag`, `--concept`, `--doc` (search); `--tag`, `--concept` (list);
  `--verify-vector`, `--repair` (doctor).
- Root command tree now lists all 14 CLI.md commands: taxonomy, reindex,
  completions added alongside existing commands.
- Root `next_actions` aligned with CLI.md: `shiro doctor` + `shiro list [--limit <n>]`
  with typed params.
- Golden JSON envelope stability tests (success + error) verifying exact key sets.
- Integration tests: root next_actions contract, exit code contract, new-flags acceptance.
- Unit tests for `truncate_snippet` including Unicode safety.
- `ParserChoice` value enum (`baseline` | `premium`).

### Fixed

- **Exit code contract**: `E_ENRICH_FAIL` now returns exit 10 (ingest/parse),
  `E_NOT_FOUND` returns exit 12 (search/query) per `docs/CLI.md`.
- **Unicode panic**: `truncate_snippet` no longer panics on multi-byte UTF-8
  characters at the truncation boundary.
- **CI branch**: workflow triggers on `master` (actual default branch), not `main`.

### Changed

- `resolve_doc_id` extracted from `read.rs`/`remove.rs` into shared
  `commands/mod.rs` helper (`pub(crate)`).
- `CmdOutput` now derives `Debug`.

### Removed

- Unused `tracing` dependency from `shiro-core`.
- Unused `camino` dependency from `shiro-cli`.

## [0.1.0] - 2026-03-04

### Added

- Initial implementation: shiro-core, shiro-store, shiro-index, shiro-parse, shiro-cli.
- Commands: init, add, ingest, search, read, explain, list, remove, doctor, config.
- JSON envelope contract with HATEOAS next_actions.
- Content-addressed document IDs (blake3).
- SQLite-backed document store with state machine (STAGED -> INDEXING -> READY).
- Tantivy-backed BM25 full-text search.
- Plain-text parser with paragraph-boundary segmentation.
- End-to-end integration test pipeline.

[0.4.0]: https://github.com/sanurb/shiro/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/sanurb/shiro/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/sanurb/shiro/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/sanurb/shiro/releases/tag/v0.1.0