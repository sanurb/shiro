# SHIRO KNOWLEDGE BASE

**Generated:** 2026-03-09 | **Commit:** ea154ea | **Branch:** master

## OVERVIEW

Local-first PDF/Markdown knowledge engine. Indexes documents into a unified searchable base using BM25 full-text search, SKOS taxonomy, and heuristic enrichment. Single Rust binary, JSON-only CLI + Code Mode MCP server (stdio). v0.3.0, MIT/Apache-2.0.

Eight crates: **shiro-core** (domain types, ports, errors, EmbeddingFingerprint ADR-012), **shiro-store** (SQLite persistence, schema v5), **shiro-index** (Tantivy BM25 full-text search), **shiro-parse** (Markdown + PDF parsers, emits ReadsBefore edges, SEGMENTER_VERSION=1), **shiro-embed** (vector embedding: FlatIndex with generation management + blake3 checksums + fingerprint sidecar, HttpEmbedder for OpenAI-compatible endpoints, StubEmbedder + DeterministicStubEmbedder for tests), **shiro-sdk** (typed API surface, spec registry, executor — hybrid search via RRF fusion of BM25 + vector, returns EntryPoint results per ADR-007), **shiro-cli** (JSON-only CLI + MCP server), **shiro-docling** (Docling-based structured PDF parser adapter).

## STRUCTURE

```
shiro/
├── crates/
│   ├── shiro-core/     # Domain types, ports, errors — every crate depends on this
│   ├── shiro-cli/      # JSON-only CLI (clap v4 derive) + HATEOAS envelope
│   ├── shiro-store/    # SQLite persistence (rusqlite, no ORM) — Schema v5
│   ├── shiro-index/    # Tantivy BM25 full-text search + generation tracking
│   ├── shiro-parse/    # MarkdownParser, PdfParser (implements Parser trait, emits ReadsBefore edges)
│   ├── shiro-embed/    # Vector embedding: FlatIndex (generation-managed), HttpEmbedder, StubEmbedder
│   ├── shiro-sdk/      # Typed API surface, spec registry, executor — CLI is thin adapter
│   └── shiro-docling/  # Docling-based structured PDF parser — subprocess boundary, crate-private schema
├── docs/
│   ├── ARCHITECTURE.md # Canonical arch reference (Document Graph IR, state machine, ADRs)
│   ├── CLI.md          # CLI output contract (JSON envelope, exit codes, HATEOAS)
│   ├── MCP.md          # MCP codemode pattern (JS execute tool, no Node)
│   └── adr/            # 34 Architecture Decision Records (ADR-001 through ADR-034)
├── scripts/            # version-sync.sh, check-capabilities.sh, tag-release.sh, coverage.sh
└── target/             # Build artifacts (gitignored)
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add a CLI command | `crates/shiro-cli/src/commands/` | Add file + register in `mod.rs` + add variant to `Commands` enum in `main.rs` |
| Change domain types | `crates/shiro-core/src/` | Hub crate — changes propagate everywhere |
| Modify storage schema | `crates/shiro-store/src/lib.rs` | DDL in `open()`, manual migrations, schema v5 |
| Change search behavior | `crates/shiro-index/src/lib.rs` | Tantivy schema + query in single file |
| Change parsing | `crates/shiro-parse/src/lib.rs` | Implements `Parser` trait from core (MarkdownParser, PdfParser) |
| Docling PDF parsing | `crates/shiro-docling/src/` | DoclingParser: subprocess → JSON → IR translation |
| Debug JSON output | `crates/shiro-cli/src/envelope.rs` | All stdout goes through `print_success`/`print_error` |
| Integration tests | `crates/shiro-cli/tests/integration.rs` | Spawns real binary, validates JSON contract |
| MCP tests | `crates/shiro-cli/tests/mcp.rs` | JSON-RPC stdio roundtrip tests (15 tests) |
| Architecture decisions | `docs/ARCHITECTURE.md` | ADRs in `docs/adr/`, state machine diagrams |
| Vector search / embeddings | `crates/shiro-embed/src/` | FlatIndex (generation-managed, blake3 checksums, fingerprint sidecar ADR-012), HttpEmbedder (OpenAI-compatible), StubEmbedder + DeterministicStubEmbedder (test-only) |
| Taxonomy operations | `crates/shiro-store/src/lib.rs` + `crates/shiro-cli/src/commands/taxonomy.rs` | SKOS model: concepts, relations, transitive closure, doc_concepts |
| Enrichment | `crates/shiro-sdk/src/ops/enrich.rs` + `crates/shiro-store/src/lib.rs` | Heuristic provider only (title, summary, tags) |
| MCP server | `crates/shiro-cli/src/commands/mcp.rs` | Code Mode: search(spec_query) + execute(program). JSON-RPC stdio |
| Generation tracking | `crates/shiro-index/src/lib.rs` + `crates/shiro-store/src/lib.rs` | GenerationId(u64), active_generations table, staging build + promote |
| DSL interpreter | `crates/shiro-sdk/src/dsl.rs` | JSON AST: let/call/if/for_each/return. Variable substitution: `$var.path.0.field` |

## DATA FLOW

```
File → Parser.parse() → Document(blake3 DocId, canonical_text, BlockGraph)
     → Store.put_document(Staged) → Store.put_segments()
     → FtsIndex.index_segments() → Store.set_state(Ready)

Search → FtsIndex.search() + optional FlatIndex.search(embedder.embed(query))
       → RRF fusion(k=60, BM25 + vector when available, ADR-014)
       → resolve segment→block via BlockGraph overlap (ADR-007)
       → build context window from reading order
       → Store.save_search_results(with block_idx, block_kind, vector_score/rank)
       → return Vec<EntryPoint> (ADR-007)

Explain → Store.get_search_result(result_id) → stored block_idx/block_kind
        → build retrieval_trace with per-source contributions (ADR-014)
        → entry_point_selection + context_expansion reasoning
```

## CONVENTIONS

- **ALL output is JSON to stdout** — no ANSI, no `--json` flag, no human-readable mode. Logs → stderr via tracing. Exception: `completions` outputs raw shell script, bypassing JSON envelope.
- **HATEOAS envelope on every response** — `{ ok, command, result, next_actions }`. Error: `{ ok: false, error: { code, message }, next_actions }`.
- **Content-addressed IDs** — `DocId = blake3(content)` prefixed `doc_`, `SegmentId = blake3(doc_id:index)` prefixed `seg_`, `RunId = timestamp` prefixed `run_`.
- **camino::Utf8PathBuf everywhere** — no `std::PathBuf`. All paths are UTF-8.
- **State machine** — `STAGED → INDEXING → READY`, `INDEXING → FAILED`, `any → DELETED`. Documents searchable ONLY in `Ready`.
- **Ports only for truly external adapters** — `Parser`, `Embedder`, `VectorIndex` traits. SQLite/Tantivy are concrete infrastructure, NOT behind traits.
- **Half-open byte spans** — `[start, end)` invariant enforced at `Span::new()`. Adjacent spans do NOT overlap.
- **Zero unsafe, zero unwrap in production** — all error propagation uses `?`. `unwrap()`/`expect()` confined to `#[cfg(test)]`. Two known exceptions: `flat.rs` flush (Vec write — infallible), `mcp.rs` ensure_ctx (post-init invariant).
- **ErrorCode dual-tracking** — every `ShiroError` variant maps to an `ErrorCode` with both `as_str()` (JSON) and `exit_code()` (CLI). 21 variants total (including E_EXECUTION_LIMIT, E_DSL_ERROR).
- **ShiroHome paths** — root, db_path, tantivy_dir, staging_tantivy_dir, vector_dir, staging_vector_dir, lock_dir, config_path.
- **Config get/set** — fully implemented with dotted-key TOML support. `config get <key>` reads, `config set <key> <value>` writes.
- **All 17 commands dispatched** — init, add, ingest, search, read, explain, list, remove, doctor, config, capabilities, taxonomy, reindex, mcp, completions, enrich, root.
- **Search returns EntryPoints (ADR-007)** — `EntryPoint` is the public retrieval result: best position in a document to begin reading, with block_idx, block_kind, span, snippet, scores, and context_window. Segment identifiers NEVER appear in public output. Segments are derived operational views (ADR-009).
- **Search** — Hybrid: BM25 + vector cosine via RRF (ADR-014). BM25 always required; vector optional with graceful fallback. Three modes: `hybrid` (default), `bm25`, `vector`. RRF fusion k=60, stable tie-break (score desc, id asc).
- **Processing fingerprints (ADR-004)** — `ProcessingFingerprint` (parser_name, parser_version, segmenter_version) persisted on every add/ingest. `Parser` trait requires `version()` method. `SEGMENTER_VERSION` constant in shiro-parse. Doctor checks for missing fingerprints.
- **Context expansion** — `--expand` on search: alternating before/after from hit segment, budget (max_blocks default 12, max_chars default 8000).
- **Staging→promote atomic rename** — FtsIndex and FlatIndex both build into staging dirs, then `fs::rename()` atomically into place. Prevents partial indices.
- **write.lock for writes only** — file lock in `lock_dir` acquired only for mutating operations. Reads are lock-free.
- **Embedder determinism** — `Embedder` impl must return identical vectors for identical input (trait contract).
- **VectorIndex idempotent + thread-safe** — `upsert` must be safe to call with same ID repeatedly. Implementations must be `Send + Sync`.
- **Taxonomy CLI subcommands** — `taxonomy add`, `taxonomy list`, `taxonomy relations`, `taxonomy assign`, `taxonomy import`.
- **Read modes** — Text (raw, 50k char limit), Segments (per-segment+span), Outline (first lines per block).
- **MCP Code Mode** — two tools only: `shiro.search` (spec discovery) and `shiro.execute` (DSL programs). JSON-RPC 2.0 over stdio.
- **DSL interpreter** — JSON AST with `let`, `call`, `if`, `for_each`, `return`. Variable substitution: `$var.path.0.field`. Hard limits: max_steps=200, max_iterations=100, max_output_bytes=1MiB, timeout=30s.
- **schemars JSON Schemas** — all SDK Input/Output types derive `schemars::JsonSchema`. `spec::generate_schemas()` produces the full schema set.

## ANTI-PATTERNS (THIS PROJECT)

- **NEVER** use `std::PathBuf` — use `camino::Utf8PathBuf`
- **NEVER** print to stdout directly — all output through `envelope.rs`
- **NEVER** use `unwrap()`/`expect()` in production code
- **NEVER** put SQLite/Tantivy behind trait abstractions — they're concrete infrastructure
- **NEVER** add ANSI/color to CLI output — JSON-only contract
- **NEVER** clone large dynamic values on read paths when a borrowed reference will do
- **NEVER** use check-then-act filesystem patterns like exists() before read/write when direct I/O with precise error handling is available
- **NEVER** hide ownership or allocation costs in convenience helpers — avoid unnecessary subtree clones and full-file rereads on hot command path
- **NEVER** leak Docling schema types outside `shiro-docling` — translate at the boundary

## UNIMPLEMENTED / STUBS

Hybrid retrieval is fully wired: Engine auto-detects FlatIndex, accepts optional Embedder, and search fuses BM25 + vector via RRF. CLI embedder injection via config is not yet implemented — embedder must be injected programmatically or via a future `shiro embed` command. The `execute_vector` SDK function requires a running embedding server.

## GOTCHAS

- `Store.put_segments()` does DELETE+INSERT loop without explicit transaction wrapping — partial segments possible on mid-loop failure
- `FtsIndex.index_segments()` is additive (no dedup guard) — caller must `delete_doc` before re-indexing
- `FtsIndex` creates a new `IndexWriter(50MB)` per write call — simple but expensive
- Parser uses pointer arithmetic on `&str` slices for span offsets — fragile if `canonical_text` is reallocated before span use
- `resolve_doc_id()` in `commands/mod.rs` does title matching via O(n) full list scan
- `completions` command bypasses the JSON envelope — outputs raw shell script directly to stdout
- `FlatIndex.open()` silently skips malformed/dimension-mismatch entries — check logs if vector count seems low after reload
- `purge_derived()` is deceptive: removes segments and search_results but NOT blocks/edges/reading_order (those are canonical per ADR-006)
- `shiro-sdk/src/dsl.rs` tests use `std::env::temp_dir()` instead of `tempfile::TempDir` — non-deterministic cleanup between runs

## TESTING

- **312 `#[test]` functions total** across inline `#[cfg(test)]` modules and integration tests
- **Integration**: `crates/shiro-cli/tests/integration.rs` (25 tests), `mcp.rs` (15 tests), `performance.rs` (2 tests)
- **Property tests**: proptest in `shiro-core` (BlockGraph, Span, taxonomy invariants)
- **Fixture tests**: `shiro-docling/tests/` with JSON fixtures for Docling→IR translation
- **Test helpers**: `shiro(home, args)` binary runner, `tmp_store()` builder, `test_doc(content)` constructor
- **Test support module**: `shiro-docling` exposes `pub mod __test_support` (#[doc(hidden)]) for fixture tests without docling binary

## COMMANDS

```bash
cargo build                    # Build all crates
cargo test --workspace         # Run all tests (unit + integration)
cargo clippy -- -D warnings    # Lint (CI enforced)
cargo fmt --check              # Format check (CI enforced)
cargo lint                     # Alias: clippy -D warnings
cargo test-all                 # Alias: test --workspace
cargo check-all                # Alias: check --workspace
```

## CI / RELEASE

- **CI**: version-sync → fmt → clippy → test → check → schema-snapshot-check → capabilities-parity-check → MCP-smoke-test → CLI-fixture-smoke-test (ubuntu-latest, stable, `RUSTFLAGS=-D warnings`). Plus nix build job.
- **Coverage**: cargo-llvm-cov on push/PR to master (*.rs or Cargo.toml changes), uploads to Codecov
- **Release**: changeset-driven. Tag-triggered 4-target matrix (x86_64/aarch64 × linux/darwin), SHA256SUMS.txt, GitHub Release + npm publish as `@sanurb/shiro-cli`
- **Version sync**: `scripts/version-sync.sh` — changesets bumps package.json, script propagates to Cargo.toml
- **Rule 0**: `scripts/check-capabilities.sh` — no feature may ship as stub/planned/not_implemented
- **Hooks** (lefthook): pre-commit: fmt+clippy (sequential) | pre-push: test+check (parallel)
