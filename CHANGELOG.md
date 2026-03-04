# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.2.0]: https://github.com/sanurb/shiro/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/sanurb/shiro/releases/tag/v0.1.0
