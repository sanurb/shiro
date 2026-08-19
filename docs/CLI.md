# shiro CLI Reference

Reference for the `shiro` command-line tool.

every invocation returns **JSON to stdout**. No tables. No ANSI formatting. No `--json` flag.  
Humans can use `jq`. Agents can parse deterministically.

## Contents

- [Command contract](#command-contract)
- [Global options](#global-options)
- [Root command (self-documenting)](#root-command-self-documenting)
- [HATEOAS: `next_actions`](#hateoas-next_actions)
- [NDJSON streaming (`--follow`)](#ndjson-streaming---follow)
- [Output truncation rules](#output-truncation-rules)
- [Exit codes](#exit-codes)
- [Commands](#commands)
  - [Library](#library)
  - [Ingest](#ingest)
  - [Search](#search)
  - [Read](#read)
  - [Explain](#explain)
  - [List and remove](#list-and-remove)
  - [Capabilities](#capabilities)
  - [Config](#config)
  - [Maintenance](#maintenance)
  - [Taxonomy](#taxonomy)
  - [Enrich](#enrich)
  - [Reindex](#reindex)
  - [Benchmark](#benchmark)
  - [MCP](#mcp)
  - [Completions](#completions)
- [Stable error codes](#stable-error-codes)

## Command contract

### Success envelope

```json
{
  "ok": true,
  "command": "shiro <…>",
  "result": {},
  "next_actions": [
    {
      "command": "shiro <…>",
      "description": "What to do next",
      "params": {
        "param": { "value": "…", "default": "…", "enum": ["…"], "description": "…" }
      }
    }
  ]
}
````

### Error envelope

```json
{
  "ok": false,
  "command": "shiro <…>",
  "error": {
    "message": "What went wrong",
    "code": "E_SOMETHING"
  },
  "fix": "Plain-language suggested fix (actionable)",
  "next_actions": [
    { "command": "shiro <…>", "description": "Recovery step" }
  ]
}
```

> **Stdout is contract output only.** Logs are written to **stderr**.

## Global options

These options apply to all commands.

```bash
shiro --version
shiro --help
shiro --home <path> <command>
shiro --log-level silent|error|warn|info|debug <command>
```

| Option          | Type |    Default | Notes                  |
| --------------- | ---: | ---------: | ---------------------- |
| `--home <path>` | path | `~/.shiro` | Overrides `SHIRO_HOME` |
| `--log-level …` | enum |     `warn` | Logs to stderr         |

## Root command (self-documenting)

Invoking `shiro` with no arguments returns the **command tree** (no help parsing required).

```bash
shiro
```

```json
{
  "ok": true,
  "command": "shiro",
  "result": {
    "description": "shiro — local-first PDF/Markdown knowledge engine",
    "commands": [
      { "name": "init", "usage": "shiro init" },
      { "name": "add", "usage": "shiro add <path|url> [--parser <baseline>] [--follow]" },
      { "name": "ingest", "usage": "shiro ingest <dir...> [--parser <baseline>] [--max-files <n>] [--follow]" },
      { "name": "search", "usage": "shiro search <query> [--mode <bm25>] [--limit <n>] [--expand] [--tag <tag>] [--concept <id>] [--doc <doc_id>]" },
      { "name": "read", "usage": "shiro read <doc_id|title> [--outline|--text|--blocks]" },
      { "name": "explain", "usage": "shiro explain <result_id>" },
      { "name": "list", "usage": "shiro list [--tag <tag>] [--concept <id>] [--limit <n>]" },
      { "name": "remove", "usage": "shiro remove <doc_id|title> [--purge]" },
      { "name": "enrich", "usage": "shiro enrich <doc_id> [--provider <heuristic>]" },
      { "name": "taxonomy", "usage": "shiro taxonomy <subcommand> ..." },
      { "name": "config", "usage": "shiro config <show|get|set> ..." },
      { "name": "doctor", "usage": "shiro doctor [--verify-vector]" },
      { "name": "reindex", "usage": "shiro reindex [--fts] [--follow]" },
      { "name": "mcp", "usage": "shiro mcp [--home <path>]" },
      { "name": "completions", "usage": "shiro completions <shell>" },
      { "name": "capabilities", "usage": "shiro capabilities" }
    ]
  },
  "next_actions": [
    { "command": "shiro doctor", "description": "Check library health" },
    { "command": "shiro list [--limit <n>]", "description": "List documents", "params": { "n": { "default": 20, "description": "Max documents" } } }
  ]
}
```

> **Note:** All 16 commands are fully dispatched in v0.3.0.

## HATEOAS: `next_actions`

Every response includes `next_actions`: **commands the agent can run next**.

### Template syntax

* `<required>` positional args
* `[--flag <value>]` optional flags

### When a command is a template

If a `next_actions[i].params` object is present, `command` is a **template** with typed placeholders.

Param metadata:

* `value`: pre-filled from the current response context
* `default`: value if omitted
* `enum`: valid choices
* `description`: meaning and intent

Example:

```json
{
  "ok": true,
  "command": "shiro add ~/docs/paper.pdf",
  "result": { "doc_id": "01K...", "status": "READY" },
  "next_actions": [
    {
      "command": "shiro read <doc_id> --outline",
      "description": "View extracted structure",
      "params": {
        "doc_id": { "value": "01K...", "description": "Document ID (ULID)" }
      }
    },
    {
      "command": "shiro search <query> [--expand] [--limit <n>]",
      "description": "Search the library",
      "params": {
        "query": { "description": "Search query" },
        "expand": { "default": true, "description": "Include structure-based context" },
        "n": { "default": 10, "description": "Number of results" }
      }
    }
  ]
}
```

## NDJSON streaming (`--follow`)

`shiro ingest --follow` emits NDJSON progress events to **stderr**. The final JSON result is still written to **stdout** as a normal envelope.

### Stream event types (stderr)

| Event | Terminal? | Fields |
| --- | --- | --- |
| `start` | no | `event`, `total_files` |
| `indexed` | no | `event`, `doc_id`, `path`, `segments` |
| `skipped` | no | `event`, `path`, `reason` |
| `failed` | no | `event`, `path`, `code`, `message` |
| `complete` | yes | `event`, `added`, `ready`, `failed` |

### Example: ingest with follow

```bash
shiro ingest ~/papers --follow 2>progress.jsonl
```

stderr (`progress.jsonl`):
```json
{"event":"start","total_files":2}
{"event":"indexed","doc_id":"doc_993d...","path":"/home/user/papers/a.md","segments":3}
{"event":"indexed","doc_id":"doc_529a...","path":"/home/user/papers/b.txt","segments":1}
{"event":"complete","added":2,"ready":2,"failed":0}
```

stdout (normal envelope):
```json
{"ok":true,"command":"shiro ingest","result":{"added":2,"ready":2,"failed":0,"failures":[]},"next_actions":[...]}
```

## Output truncation rules

To protect agent context:

* `list` fetches `limit + 1` to detect truncation and returns `truncated: true` when more exist.
* `read --view text` truncates `canonical_text` to 50,000 bytes and returns `truncated: true`.

Standard fields when truncating:

```json
{
  “showing”: 20,
  “total”: 21,
  “truncated”: true,
  “items”: [/* … */]
}
```

## Exit codes

* `0` success
* `1` generic failure (I/O error, taxonomy cycle)
* `2` usage/config error (invalid input, config error)
* `10` ingest/parse failure (PDF, Markdown, IR, embed, enrich)
* `11` index build/activation failure (FTS, vector)
* `12` search/query failure (search failed, not found)
* `20` store corruption detected
* `21` lock busy

## Commands

### Library

#### `shiro init`

```bash
shiro init
```

Creates `<home>/`, `<home>/tantivy/`, `<home>/lock/`, initializes SQLite schema and Tantivy index.

**Result**

```json
{ "created": true, "home": "/Users/you/.shiro" }
```

### Ingest

#### `shiro add`

```bash
shiro add <path> [--parser <auto|plaintext|markdown|pdf|docling>]
```

**Arguments**

* `<path>`: local file path. Use `acquire-url` for network sources.

**Behavior**

* Content-addressed deduplication: if `doc_id` already exists, returns existing doc (`changed: false`).
* Pipeline: parse → `STAGED` → segment → FTS index → `READY`.
* Parser selection is explicit or inferred from the local extension.
* When an embedder is configured, changed segments are embedded in bounded batches and unchanged compatible vectors are reused before common FTS/vector activation.

**Result**

```json
{ "doc_id": "doc_...", "status": "READY", "title": "...", "segments": 3, "changed": true }
```

**Example**

```bash
shiro add ~/docs/notes.md
```

#### `shiro acquire-url`

```bash
shiro acquire-url <https-url> \
  [--parser <auto|plaintext|markdown|pdf>] \
  [--max-bytes <n>] [--timeout-ms <n>] [--max-redirects <n>]
```

HTTPS is required unless `--allow-http` is explicit. Every DNS resolution and redirect is revalidated against private/local/non-routable targets. Shiro enforces total time and response-byte limits, validates PDF or UTF-8 signatures, stores content in the CAS, and atomically records requested/final URLs, redirects, MIME evidence, signature, hash, and canonical source provenance.

#### `shiro ingest`

```bash
shiro ingest <dir...> \
  [--parser <baseline>] \
  [--max-files <n>] \
  [--follow]
```

**Behavior**

* Walks directories, filters by extension (`.txt`, `.md`, `.markdown`).
* Files processed in deterministic sorted order.
* 3-phase pipeline: (1) parse all + store in one SQLite transaction, (2) bulk FTS index in one Tantivy commit, (3) bulk state update.
* `--max-files` selects the first N files in that deterministic order.
* After a new document reaches `READY`, the AutoTagger compares heuristic title/tag evidence and, when configured, embedding similarity against existing concepts. Matches are persisted only as fully attributed `PROPOSED` model-enrichment records. They do not affect concept-filtered retrieval until explicitly promoted with `shiro enrich-model resolve`.
* Automatic proposals are enabled by default. Disable them with `shiro config set ingest.auto_concept_proposals false`. No setting auto-accepts proposals.

**Result**

```json
{ "added": 5, "ready": 5, "failed": 0, "failures": [], "concept_proposals": [{ "proposal_id": "proposal_...", "status": "PROPOSED", "trust_zone": "PROPOSED" }] }
```

### Search

#### `shiro search`

```bash
shiro search <query> \
  [--mode <bm25>] \
  [--limit <n>] \
  [--expand] \
  [--max-blocks <n>] \
  [--max-chars <n>] \
  [--tag <tag>] \
  [--concept <concept_id>] \
  [--doc <doc_id>]
```

**Defaults**

* `--mode`: `hybrid` (gracefully falls back to BM25 when vector retrieval is unavailable)
* `--limit`: 10
* `--expand`: off (when enabled, expands context via `expand_context()` using structure-aware alternating before/after from hit segment)
* `--max-blocks`: 12 (context expansion budget)
* `--max-chars`: 8000 (context expansion budget)
* `--tag`, `--concept`, `--doc`: filter facets. A concept filter matches assignments to that concept or any transitive descendant recorded in `concept_closure`; filtering by a narrower concept does not match assignments to its broader ancestors.

**Notes**

* `hybrid`, `bm25`, and `vector` modes are supported when their configured runtime providers are available.
* Search results are immutable snapshots persisted for later `explain`.

**Result**

```json
{
  "query": "...",
  "mode": "bm25",
  "fts_generation": 1,
  "hits": [{
    "result_id": "res_...",
    "evidence_handle": "blk_...",
    "doc_id": "doc_...",
    "block_idx": 0,
    "span_start": 0,
    "span_end": 100,
    "snippet": "..."
  }]
}
```

#### `shiro search-pack`

```bash
shiro search-pack <query>... \
  [--mode <hybrid|bm25|vector>] \
  [--per-query-limit <n>] [--limit <n>] \
  [--include-content] [--rerank] \
  [--tag <tag>] [--concept <concept_id>] [--doc <doc_id>]
```

Runs the queries in one operation, deduplicates results by `evidence_handle`, and ranks the union by reciprocal-rank contributions across queries. Each hit reports `matched_queries` and a query-ID-to-`result_id` map. Snippets and context are omitted unless `--include-content` is set.

### Read

#### `shiro read`

```bash
shiro read <doc_id|title|evidence_handle> [--view <outline|text|blocks>] [--page <n>]
```

* `shiro read blk_...`: returns the stored canonical block and an `evidence_resolution` with `ACTIVE` or `SUPERSEDED` status and an optional `superseded_by` handle. Superseded handles are never silently redirected.
* `--page <n>`: returns canonical blocks attributed to a one-based source page; requires parser-provided source locators.
* `--view text` (default): `canonical_text`, truncated to 50,000 bytes. Fields: `doc_id`, `title`, `status`, `text`, `truncated`
* `--view blocks`: canonical blocks in reading order. Each block includes `index`, `kind`, optional one-based `heading_level`, byte span, source-faithful `body`, and parser-neutral `source_locators`.
* `--view outline`: first line of each segment. Fields: `doc_id`, `title`, `status`, `outline: [{ index, preview }]`

ID resolution: `blk_` values are stable evidence handles, `doc_` values are document IDs, and other values are matched against titles.

### Explain

#### `shiro explain`

```bash
shiro explain <result_id>
```

Takes a `result_id` from a prior `search` call. Returns:

* IDs: `result_id`, `query`, `doc_id`, `segment_id`, `block_id`
* Location: `span`
* Generation: `generation.fts_gen`, `generation.vec_gen`
* Scoring: `scores.bm25.{score, rank}`, `scores.vector.{score, rank}` (when available), `scores.fused`
* `retrieval_trace`: pipeline stages, RRF fusion contributions (`k=60`), vector scores when available
* `expansion`: rules fired, included block IDs, budget usage

### List and remove

#### `shiro list`

```bash
shiro list [--tag <tag>] [--concept <concept_id>] [--limit <n>]
```

Default `limit` should be conservative (e.g., 20) and use truncation fields when exceeded.

#### `shiro remove`

```bash
shiro remove <doc_id|title> [--purge]
```

* default: tombstone (rebuildable)
* `--purge`: attempt immediate removal from derived indices

### Capabilities

#### `shiro capabilities`

```bash
shiro capabilities
```

Returns a machine-readable capability manifest: version, schema version, state machine transitions, ID schemes, available parsers, feature implementation status, and storage backends.

### Config

#### `shiro config show`

```bash
shiro config show
```

Returns resolved paths: `home`, `db_path`, `tantivy_dir`, `config_path`, `lock_dir`.

#### `shiro config get` / `shiro config set`

```bash
shiro config get <key>
shiro config set <key> <value>
```

Reads and writes individual keys from `config.toml` using dotted-key notation (e.g., `search.limit`). `ingest.auto_concept_proposals` is a boolean and defaults to `true`; setting it to `false` opts out of automatic PROPOSED concept assignments.

Type inference on `set`: values are parsed as `i64`, then `f64`, then `bool` (`true`/`false`), falling back to `String`.

**Example**

```bash
shiro config get search.limit
shiro config set search.limit 20
```

**Result (get)**

```json
{ "key": "search.limit", "value": 20 }
```

### Maintenance

#### `shiro doctor`

```bash
shiro doctor [--verify-vector]
```

Runs 6 diagnostic checks: `home_directory`, `sqlite_store`, `fts_index`, `schema_version`, `document_states`, `fts_consistency`.

* `--verify-vector`: checks FlatIndex integrity at `vectors.jsonl` with 384 dimensions.

**Result**

```json
{ "healthy": true, "checks": [{ "name": "sqlite_store", "status": "ok", "message": "..." }] }
```

### Taxonomy

SKOS-based taxonomy management with concepts, relations, and document assignment.

#### `shiro taxonomy add`

```bash
shiro taxonomy add <label> [--scheme <uri>] [--definition <text>]
```

Creates a new concept. Returns `concept_id`.

#### `shiro taxonomy list`

```bash
shiro taxonomy list [--scheme <uri>]
```

Lists all concepts, optionally filtered by scheme URI.

#### `shiro taxonomy search` / `browse`

```bash
shiro taxonomy search <query> [--limit <n>]
shiro taxonomy browse [--root <concept_id>] [--max-depth <n>] [--max-nodes <n>]
```

Search covers preferred labels, alternate labels, definitions, and scheme URIs. Browse returns a bounded relation graph; every concept includes a non-empty `text_fallback`.

#### `shiro taxonomy relations` / `relate`

```bash
shiro taxonomy relations <concept_id>
shiro taxonomy relate <from_concept_id> <to_concept_id> \
  --kind <broader|narrower|related>
```

`relations` inspects outgoing relations. `relate` authors a directed SKOS relation and rebuilds `concept_closure` atomically. `BROADER` means the target is the source's ancestor; `NARROWER` expresses the inverse direction and contributes identically to closure. Hierarchy cycles are rejected with `E_TAXONOMY_CYCLE`, and neither the relation nor closure changes on rejection. `RELATED` is associative and does not affect hierarchical closure.

#### `shiro taxonomy assign`

```bash
shiro taxonomy assign <doc_id> <concept_id> [--confidence <f64>]
```

Assigns a concept to a document with optional confidence score.

#### `shiro taxonomy import`

```bash
shiro taxonomy import <path>
```

Imports concepts and relations from a SKOS JSON file. Expected format includes `broader`, `narrower`, and `related` relation arrays. The transitive `concept_closure` table is rebuilt once after all relation writes. A closure rebuild scans the full hierarchy, so rebuilding once keeps bulk-import cost to one graph expansion instead of repeating it for every imported edge.

### Enrich

#### `shiro enrich`

```bash
shiro enrich <doc_id> [--provider <heuristic>]
```

Runs enrichment on a document.

* `--provider`: `heuristic` (default, only supported provider). LLM provider returns `E_INVALID_INPUT`.
* Heuristic enrichment: `title` = first non-empty line, `summary` = first 500 characters, `tags` = extracted markdown headings.

**Result**

```json
{ "doc_id": "doc_...", "provider": "heuristic", "title": "...", "summary": "...", "tags": ["heading1", "heading2"] }
```

#### `shiro enrich-model`

```bash
shiro enrich-model propose --file <proposal.json>
shiro enrich-model resolve <proposal_id> \
  --action <promote|reject> --actor <id> --approval <id>
```

Model output requires provider/model/actor, data-region, retention-policy, consent, and labeled concept evidence. It remains `PROPOSED` and cannot affect trusted filters until explicit promotion. Rejecting a promoted proposal removes only assignments introduced by that proposal and does not overwrite manual assignments.

### Reindex and reprocessing

#### `shiro reindex`

```bash
shiro reindex
```

Rebuilds derived indices from the SQLite source of truth. FTS and any configured
vector index are validated in generation-specific immutable locations, then
activated through one common corpus manifest.

#### `shiro reprocess`

```bash
shiro reprocess --parser <name> [--doc <doc_id>]... \
  [--target <parse|derived|all>] [--include-vector] \
  [--resume-manifest-id <id>] [--execute] \
  [--max-documents <n>] [--max-source-bytes <n>] \
  [--max-model-calls <n>] [--embedding-batch-size <n>]
```

Dry-run is the default. The plan reports selected documents, stale processing identity, transitive artifacts, source/model/batch estimates, hard blockers, and exact rollback generations. Execution first verifies the rollback manifest and artifacts, then reparses persisted sources and/or publishes complete generations within the declared limits.

### Benchmark

#### `shiro benchmark`

```bash
shiro benchmark <manifest.json> [--warmup-runs <n>] [--measured-runs <n>]
```

Evaluates a versioned, adjudicated corpus manifest against the current engine,
then performs a mandatory reindex and verifies ranking integrity. Results report
quality metrics, paired confidence intervals, latency percentiles, observed RSS,
explain completeness, determinism, and an explicit `pass`, `fail`, or
`insufficient_evidence` status. See [`benchmarks/README.md`](../benchmarks/README.md).

### MCP

#### `shiro mcp`

```bash
shiro mcp [--home <path>]
```

Start MCP Code Mode server (JSON-RPC 2.0 over stdio). Reads newline-delimited JSON from stdin, writes JSON + newline to stdout.

Accepts the `--home` flag to override the default library location.

**Tools exposed:**

| Tool | Input | Output |
|------|-------|--------|
| `shiro.search` | `{query, limit?}` | Ranked `SpecSearchResult[]` with op specs, schemas, examples |
| `shiro.execute` | `{program, limits?}` | `ExecutionResult {value, steps_executed, total_duration_us, trace[]}` |

See [MCP Guide](MCP.md) for protocol details, DSL grammar, and client configuration.

### Completions

#### `shiro completions`

```bash
shiro completions <shell>
```

Generates shell completion scripts. Supported shells: `bash`, `zsh`, `fish`, `powershell`.

> **Note:** Outputs raw shell script to stdout. This command bypasses the JSON envelope.
## Stable error codes

| Code | Exit | Description |
| --- | --- | --- |
| `E_IO` | 1 | I/O error |
| `E_TAXONOMY_CYCLE` | 1 | Taxonomy cycle detected |
| `E_INVALID_INPUT` | 2 | Invalid input |
| `E_CONFIG` | 2 | Configuration error |
| `E_PARSE_PDF` | 10 | PDF parse error |
| `E_PARSE_MD` | 10 | Markdown parse error |
| `E_INVALID_IR` | 10 | Invalid IR |
| `E_EMBED_FAIL` | 10 | Embedding failed |
| `E_ENRICH_FAIL` | 10 | Enrichment failed |
| `E_INDEX_BUILD_FTS` | 11 | FTS index build failed |
| `E_INDEX_BUILD_VEC` | 11 | Vector index build failed |
| `E_NOT_FOUND` | 12 | Document/result not found |
| `E_SEARCH_FAILED` | 12 | Search failed |
| `E_STORE_CORRUPT` | 20 | Store corruption detected |
| `E_LOCK_BUSY` | 21 | Write lock busy |
| `E_MCP` | 1 | MCP server error |
| `E_SCHEMA_MIGRATION` | 1 | Schema migration failed |
| `E_GENERATION_CONFLICT` | 1 | Generation conflict during index swap |
| `E_EXECUTION_LIMIT` | 1 | Execution limit exceeded (steps, iterations, output size, or timeout) |
| `E_DSL_ERROR` | 1 | DSL interpretation error (unknown node, invalid variable reference, type error) |