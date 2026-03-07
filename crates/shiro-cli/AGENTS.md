# shiro-cli

JSON-only CLI with HATEOAS envelope. clap v4 derive macros. All stdout is single-line JSON — never print directly.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add command | `src/commands/<name>.rs` | Create file, add to `mod.rs`, add `Commands` variant + dispatch in `main.rs` |
| Change output shape | `src/envelope.rs` | `CmdOutput`, `NextAction`, `ParamMeta` — golden tests enforce schema |
| Change CLI args | `src/main.rs` | `Commands` enum (clap derive), type enums (`SearchModeArg`, `ReadView`, etc.) |
| Fix error mapping | `src/main.rs` | `suggest_fix()` and `recovery_actions()` |
| Title→DocId resolution | `src/commands/mod.rs` | `resolve_doc_id()` shared helper |

## SDK ACCESS PATTERNS

Commands use two access patterns:
- **High-level**: `Engine::open(home)` — used by `add`, `ingest`, `search`, `explain`
- **Low-level**: direct `Store`/`FtsIndex` via `ShiroHome` — used by `list`, `read`, `remove`, `doctor`, `reindex`, etc.

## COMMAND PATTERN

Every command is a function `run_*(...) -> Result<CmdOutput, ShiroError>`:
1. Accept typed args (from clap)
2. Open `Store` and/or `FtsIndex` via `ShiroHome`
3. Perform operation
4. Return `CmdOutput { result: serde_json::Value, next_actions: Vec<NextAction> }`

Dispatch in `main.rs` calls the function, `print_success()` or `print_error()` handles serialization + exit code.

**Exception**: `completions` bypasses the JSON envelope — outputs raw shell script consumed by shell directly.

## COMMANDS (17 files = 16 commands + mod.rs)

| Command | File | Purpose |
|---------|------|---------|
| add | `add.rs` | Single file: parse → dedup check → stage → index → READY |
| ingest | `ingest.rs` | Batch: 3-phase (bulk SQLite TX → Tantivy commit → mark READY). Walks `.txt/.md/.markdown` |
| search | `search.rs` | BM25 via FtsIndex. `result_id = blake3(query:seg_id)[..16]` prefixed `res_`. Persists for explain |
| read | `read.rs` | Three modes: **Text** (raw, 50k char limit), **Segments** (per-segment+span), **Outline** (first lines) |
| explain | `explain.rs` | Cached result lookup. Vector/taxonomy fields are TODO stubs |
| list | `list.rs` | Fetches limit+1 to detect truncation without COUNT query |
| remove | `remove.rs` | Tombstones via `set_state(Deleted)`. `--purge` also removes from Tantivy |
| doctor | `doctor.rs` | Three checks: home dir, Store open, FtsIndex open. Short-circuits on first failure |
| config | `config.rs` | Sub-enum: `show`/`get`/`set` — fully implemented. Uses `run_show`, `run_get`, `run_set` |
| init | `init.rs` | Create dirs + open Store (migrates) + open FtsIndex. Idempotent |
| root | `root.rs` | No-subcommand: self-documenting JSON listing all commands |
| taxonomy | `taxonomy.rs` | Sub-enum with 5 sub-commands: `add`/`list`/`relations`/`assign`/`import`. Each has `run_*` fn |
| mcp | `mcp.rs` | JSON-RPC stdio server. 2 tools: search + execute |
| capabilities | `capabilities.rs` | Static capability arrays + `schema_version` from Store |
| reindex | `reindex.rs` | Rebuild FTS index from stored segments via `shiro_sdk::ops::reindex` |
| enrich | `enrich.rs` | Run enrichment pipeline on a document via `shiro_sdk::ops::enrich` |
| completions | `completions.rs` | Shell completions (bash/zsh/fish/powershell). Bypasses JSON envelope — raw output |

## SUB-ENUM COMMANDS

`taxonomy` and `config` use clap sub-enums dispatched to multiple `pub run_*` functions:
- `config`: `run_show`, `run_get`, `run_set`
- `taxonomy`: `run_add`, `run_list`, `run_relations`, `run_assign`, `run_import`

## TESTS

- `tests/integration.rs` + `tests/performance.rs`: 26 tests total, each creates `TempDir`
- Spawns real binary via `env!(CARGO_BIN_EXE_shiro)`, `--home` tempdir, `--log-level silent`
- All output parsed as JSON — tests validate envelope schema, exit codes, pipeline correctness
- `cargo test -p shiro-cli` to run
