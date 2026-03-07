# shiro-sdk

Typed API surface for the shiro knowledge engine.
CLI and MCP are thin adapters that delegate here. Engine struct delegates to `ops/*`.

## Where to look

| Task                        | Path                  |
|-----------------------------|-----------------------|
| Add/modify an operation     | `src/ops/<name>.rs`   |
| Engine convenience wrapper  | `src/engine.rs`       |
| MCP program dispatch        | `src/executor.rs`     |
| Op discovery for MCP        | `src/spec.rs`         |
| DSL interpreter             | `src/dsl.rs`          |
| Search ranking (RRF)        | `src/fusion.rs`       |

## Constants

- `SCHEMA_VERSION = 2` — embedded in CLI/MCP JSON responses (`lib.rs`)
- `RRF_K = 60.0` — reciprocal rank fusion parameter (`fusion.rs`), deterministic tie-break by segment ID

## Operation pattern

Each op: `pub fn execute(store, ..., input) -> Result<Output, ShiroError>`
Engine delegates: `self.method() → ops::<name>::execute()`.

Exceptions:
- `doctor` is static — takes `&ShiroHome`, no `&self` / no Store
- `reindex` takes `(&ShiroHome, &Store)`, has a second fn `execute_vector()` for vector index rebuild

## Ops

| Op        | Key behavior                                                        |
|-----------|---------------------------------------------------------------------|
| add       | Content-addressed dedup check → parse → stage → index → READY    |
| ingest    | Parse + index. Only op with progress callback (`IngestEvent` enum)  |
| search    | FTS + vector + RRF fusion. Persists results for later `explain`     |
| read      | 3 modes: Text, Segments, Outline. Owns `resolve_doc_id()` (pub(crate)) |
| list      | List documents with state/title                                     |
| remove    | Delete doc + segments. Uses `resolve_doc_id()` from read            |
| explain   | Retrieves persisted search result by result_id, returns context     |
| enrich    | Augments doc metadata. Uses `resolve_doc_id()` from read            |
| reindex   | Rebuilds FTS index; `execute_vector()` rebuilds vector index        |
| doctor    | Static health check against `&ShiroHome`                            |

## Executor (`executor.rs`)

String-matched op dispatch (not enum). JSON `{op, params}` → typed op → `serde_json::Value`.

Three param helpers:
- `str_param(params, key) -> Result<&str>` — required string
- `u64_param(params, key, default) -> u64` — optional with default
- `bool_param(params, key, default) -> bool` — optional with default

## Spec registry (`spec.rs`)

- `static OPS: &[OpSpec]` — 10 entries, sorted by name (deterministic)
- `search_specs(query, limit)` — scored keyword search (AND semantics, score desc + name asc)
- `generate_schemas()` — produces schemars-derived JSON Schemas for all SDK types
- `SpecSearchResult` — includes `spec: &OpSpec` + `score: u32`
- Empty query returns all ops with score=1
- Tests assert `OPS.len() == 10`, sorted order, valid JSON examples

## DSL interpreter (`dsl.rs`)

JSON AST interpreter for Code Mode `shiro.execute`.

**Node types:** `let`, `call`, `if`, `for_each`, `return`.

**Key types:**
- `Node` — tagged enum (`#[serde(tag = "type", deny_unknown_fields)]`)
- `CallTarget` — `{op, params}` for SDK operation calls
- `Limits` — hard execution bounds (max_steps, max_iterations, max_output_bytes, timeout_ms)
- `ExecutionResult` — `{value, steps_executed, total_duration_us, trace[]}`
- `StepTrace` — per-step: step index, node_type, op, args_hash, duration_us, result_summary, error_code

**Variable substitution:** `$var.path.0.field` (JSONPath-like, resolves at runtime).

**Entry point:** `execute_program(home, store, fts, parser, program, limits) -> Result<ExecutionResult>`

## Shared helpers

- `ops::read::resolve_doc_id(store, id_or_title)` — pub(crate), used by read, enrich, remove
