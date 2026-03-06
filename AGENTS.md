# SHIRO KNOWLEDGE BASE

**Generated:** 2026-03-05 | **Commit:** 0b247a2 | **Branch:** master

## OVERVIEW

Local-first PDF/Markdown knowledge engine. Indexes documents into a unified searchable base using hybrid BM25+vector search, SKOS taxonomy, and AI enrichment. Single Rust binary, JSON-only CLI + MCP server (stdio). v0.2.0, MIT/Apache-2.0.

## STRUCTURE

```
shiro/
├── crates/
│   ├── shiro-core/     # Domain types, ports, errors — every crate depends on this
│   ├── shiro-cli/      # JSON-only CLI (clap v4 derive) + HATEOAS envelope
│   ├── shiro-store/    # SQLite persistence (rusqlite, no ORM)
│   ├── shiro-index/    # Tantivy BM25 full-text search
│   └── shiro-parse/    # Plain-text parser (no tree-sitter)
├── docs/
│   ├── ARCHITECTURE.md # Canonical arch reference (Document Graph IR, state machine, ADRs)
│   ├── CLI.md          # CLI output contract (JSON envelope, exit codes, HATEOAS)
│   └── MCP.md          # MCP codemode pattern (JS execute tool, no Node)
└── target/             # Build artifacts (gitignored)
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add a CLI command | `crates/shiro-cli/src/commands/` | Add file + register in `mod.rs` + add variant to `Commands` enum in `main.rs` |
| Change domain types | `crates/shiro-core/src/` | Hub crate — changes propagate everywhere |
| Modify storage schema | `crates/shiro-store/src/lib.rs` | DDL in `open()`, manual migrations |
| Change search behavior | `crates/shiro-index/src/lib.rs` | Tantivy schema + query in single file |
| Change parsing | `crates/shiro-parse/src/lib.rs` | Implements `Parser` trait from core |
| Debug JSON output | `crates/shiro-cli/src/envelope.rs` | All stdout goes through `print_success`/`print_error` |
| Integration tests | `crates/shiro-cli/tests/integration.rs` | Spawns real binary, validates JSON contract |
| Architecture decisions | `docs/ARCHITECTURE.md` | ADRs at bottom, state machine diagrams |

## DATA FLOW

```
File → PlainTextParser.parse() → Document(blake3 DocId, canonical_text, BlockGraph)
     → Store.put_document(Staged) → Store.put_segments() → FtsIndex.index_segments()
     → Store.set_state(Ready)

Search → FtsIndex.search(query) → Vec<FtsHit> → Store.save_search_results()
       → explain retrieves cached results by result_id
```

## CONVENTIONS

- **ALL output is JSON to stdout** — no ANSI, no `--json` flag, no human-readable mode. Logs → stderr via tracing.
- **HATEOAS envelope on every response** — `{ ok, command, result, next_actions }`. Error: `{ ok: false, error: { code, message }, next_actions }`.
- **Content-addressed IDs** — `DocId = blake3(content)` prefixed `doc_`, `SegmentId = blake3(doc_id:index)` prefixed `seg_`, `RunId = timestamp` prefixed `run_`.
- **camino::Utf8PathBuf everywhere** — no `std::PathBuf`. All paths are UTF-8.
- **State machine** — `STAGED → INDEXING → READY`, `INDEXING → FAILED`, `any → DELETED`. Documents searchable ONLY in `Ready`.
- **Ports only for truly external adapters** — `Parser`, `Embedder`, `VectorIndex` traits. SQLite/Tantivy are concrete infrastructure, NOT behind traits.
- **Half-open byte spans** — `[start, end)` invariant enforced at `Span::new()`. Adjacent spans do NOT overlap.
- **Zero unsafe, zero unwrap in production** — all error propagation uses `?`. `unwrap()`/`expect()` confined to `#[cfg(test)]`.
- **ErrorCode dual-tracking** — every `ShiroError` variant maps to an `ErrorCode` with both `as_str()` (JSON) and `exit_code()` (CLI).

## ANTI-PATTERNS (THIS PROJECT)

- **NEVER** use `std::PathBuf` — use `camino::Utf8PathBuf`
- **NEVER** print to stdout directly — all output through `envelope.rs`
- **NEVER** use `unwrap()`/`expect()` in production code
- **NEVER** put SQLite/Tantivy behind trait abstractions — they're concrete infrastructure
- **NEVER** add ANSI/color to CLI output — JSON-only contract

## UNIMPLEMENTED / STUBS

| Feature | Location | Status |
|---------|----------|--------|
| Vector search | `shiro-core/src/ports.rs` (Embedder, VectorIndex) | Traits defined, no impl |
| Config get/set | `shiro-cli/src/commands/config.rs` | Returns error immediately |
| Taxonomy commands | `shiro-cli/src/commands/root.rs` | Listed in self-doc, not in `Commands` enum |
| Reindex command | `shiro-cli/src/commands/root.rs` | Listed in self-doc, not in `Commands` enum |
| MCP server | `shiro-cli/src/commands/root.rs` | Listed in self-doc, not in `Commands` enum |

## GOTCHAS

- `Store.put_segments()` does DELETE+INSERT loop without explicit transaction wrapping — partial segments possible on mid-loop failure
- `FtsIndex.index_segments()` is additive (no dedup guard) — caller must `delete_doc` before re-indexing
- `FtsIndex` creates a new `IndexWriter(50MB)` per write call — simple but expensive
- Parser uses pointer arithmetic on `&str` slices for span offsets — fragile if `canonical_text` is reallocated before span use
- `resolve_doc_id()` in `commands/mod.rs` does title matching via O(n) full list scan

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

- **CI**: fmt → clippy → test → check (ubuntu-latest, stable, `RUSTFLAGS=-D warnings`)
- **Release**: tag-triggered (semver), version parity check, 4-target matrix (x86_64/aarch64 × linux/darwin), SHA256SUMS.txt, GitHub Release
- **Hooks** (lefthook): pre-commit: fmt+clippy | pre-push: check+test
