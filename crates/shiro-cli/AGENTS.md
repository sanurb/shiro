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

## COMMAND PATTERN

Every command is a function `run_*(...) -> Result<CmdOutput, ShiroError>`:
1. Accept typed args (from clap)
2. Open `Store` and/or `FtsIndex` via `ShiroHome`
3. Perform operation
4. Return `CmdOutput { result: serde_json::Value, next_actions: Vec<NextAction> }`

Dispatch in `main.rs` calls the function, `print_success()` or `print_error()` handles serialization + exit code.

## ENVELOPE CONTRACT

```
Success: { "ok": true,  "command": "shiro <cmd>", "result": {}, "next_actions": [...] }
Error:   { "ok": false, "command": "shiro <cmd>", "error": { "code": "...", "message": "..." }, "next_actions": [...] }
```

- `next_actions` carries typed `ParamMeta` (value, default, enum, description) — agents consume this
- Fallback: hard-coded JSON emitted if serde serialization itself fails
- Golden tests in `envelope.rs` enforce exact top-level key sets

## COMMANDS

| Command | File | Purpose |
|---------|------|---------|
| add | `add.rs` | Single file: parse → dedup check → stage → index → READY |
| ingest | `ingest.rs` | Batch: 3-phase (bulk SQLite TX → Tantivy commit → mark READY). Walks `.txt/.md/.markdown` |
| search | `search.rs` | BM25 via FtsIndex. `result_id = blake3(query:seg_id)[..16]` prefixed `res_`. Persists for explain |
| read | `read.rs` | Three views: Text (raw, 50k char limit), Blocks (per-segment+span), Outline (first lines) |
| explain | `explain.rs` | Cached result lookup. Vector/taxonomy fields are TODO stubs |
| list | `list.rs` | Fetches limit+1 to detect truncation without COUNT query |
| remove | `remove.rs` | Tombstones via `set_state(Deleted)`. `--purge` also removes from Tantivy |
| doctor | `doctor.rs` | Three checks: home dir, Store open, FtsIndex open. Short-circuits on first failure |
| config | `config.rs` | `show` returns paths. `get`/`set` are stubs (return error) |
| init | `init.rs` | Create dirs + open Store (migrates) + open FtsIndex. Idempotent |
| root | `root.rs` | No-subcommand: self-documenting JSON listing all commands (incl. unimplemented ones) |

## TESTS

- `tests/integration.rs`: spawns real binary via `env!(CARGO_BIN_EXE_shiro)`, `--home` tempdir, `--log-level silent`
- All output parsed as JSON — tests validate envelope schema, exit codes, pipeline correctness
- `cargo test -p shiro-cli` to run
