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
  - [Taxonomy (SKOS)](#taxonomy-skos)
  - [Config](#config)
  - [Maintenance](#maintenance)
  - [MCP server](#mcp-server)
  - [Shell completions](#shell-completions)
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
      { "name": "add", "usage": "shiro add <path|url> [--enrich] [--tags <csv>] [--concepts <csv>] [--parser <baseline|premium>] [--fts-only] [--follow]" },
      { "name": "ingest", "usage": "shiro ingest <dir...> [--glob <pattern>] [--enrich] [--tags <csv>] [--concepts <csv>] [--parser <baseline|premium>] [--max-files <n>] [--fts-only] [--follow]" },
      { "name": "search", "usage": "shiro search <query> [--vector|--bm25|--hybrid] [--limit <n>] [--expand] [--tag <tag>] [--concept <id>] [--doc <doc_id>]" },
      { "name": "read", "usage": "shiro read <doc_id|title> [--outline|--text|--blocks]" },
      { "name": "explain", "usage": "shiro explain <result_id>" },
      { "name": "list", "usage": "shiro list [--tag <tag>] [--concept <id>] [--limit <n>]" },
      { "name": "remove", "usage": "shiro remove <doc_id|title> [--purge]" },
      { "name": "taxonomy", "usage": "shiro taxonomy <subcommand> ..." },
      { "name": "config", "usage": "shiro config <show|get|set> ..." },
      { "name": "doctor", "usage": "shiro doctor [--verify-vector] [--repair]" },
      { "name": "reindex", "usage": "shiro reindex [--fts] [--vector] [--follow]" },
      { "name": "mcp", "usage": "shiro mcp" },
      { "name": "completions", "usage": "shiro completions <shell>" }
    ]
  },
  "next_actions": [
    { "command": "shiro doctor", "description": "Check library health" },
    { "command": "shiro list [--limit <n>]", "description": "List documents" , "params": { "n": { "default": 20, "description": "Max documents" } } }
  ]
}
```

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
  "command": "shiro add ~/docs/paper.pdf --enrich",
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

Long-running commands support `--follow` to emit **NDJSON** (one JSON object per line).
The final line is always a terminal envelope: `type: "result"` or `type: "error"`.

### Stream event types

Terminal?

* `start`: no
* `step`: no
* `progress`: no
* `log`: no
* `event`: no (optional; domain event emitted)
* `result`: **yes**
* `error`: **yes**

### Example: ingest with follow

```bash
shiro ingest ~/papers --enrich --follow
```

```json
{"type":"start","command":"shiro ingest ~/papers --enrich --follow","ts":"..."}
{"type":"step","name":"parse","status":"started","ts":"..."}
{"type":"step","name":"parse","status":"completed","duration_ms":420,"ts":"..."}
{"type":"step","name":"segment","status":"completed","duration_ms":120,"ts":"..."}
{"type":"step","name":"embed","status":"progress","percent":45,"ts":"..."}
{"type":"step","name":"index_fts","status":"completed","duration_ms":900,"ts":"..."}
{"type":"step","name":"index_vec","status":"completed","duration_ms":2100,"ts":"..."}
{"type":"step","name":"activate","status":"completed","duration_ms":12,"ts":"..."}
{"type":"result","ok":true,"command":"shiro ingest ~/papers --enrich --follow","result":{"added":18,"ready":18,"failed":0},"next_actions":[{"command":"shiro list [--limit <n>]","description":"List documents","params":{"n":{"default":20}}}]}
```

### Notes

* Tools that don’t support streaming can read the **last line only**.
* Streaming output must be bounded (see truncation rules).

## Output truncation rules

To protect agent context:

* Lists are **limited by default**.
* Large outputs include pointers to a file path with full content.

Standard fields when truncating:

```json
{
  "showing": 20,
  "total": 4582,
  "truncated": true,
  "full_output": "/tmp/shiro-output-abc123.json",
  "items": [/* … */]
}
```

And `next_actions` must include a “show more” template.

## Exit codes

* `0` success
* `2` usage error
* `10` ingest/parse failure
* `11` index build/activation failure
* `12` search/query failure
* `20` store corruption detected
* `21` lock busy

## Commands

### Library

#### `shiro init`

```bash
shiro init
```

**Result**

* creates storage layout and SQLite schema
* initializes derived index roots (generational)

**Typical next actions**

* `shiro doctor`
* `shiro add <path|url>`

### Ingest

#### `shiro add`

```bash
shiro add <path|url> \
  [--parser <baseline|premium>] \
  [--enrich] \
  [--tags <csv>] \
  [--concepts <csv>] \
  [--fts-only] \
  [--follow]
```

**Arguments**

* `<path|url>`: local path or `http(s)` URL

**Behavior**

* Produces/updates the document and derived indices.
* Document becomes searchable only when status is `READY`.

**Example**

```bash
shiro add ~/docs/paper.pdf --enrich --follow
```

#### `shiro ingest`

```bash
shiro ingest <dir...> \
  [--glob <pattern>] \
  [--parser <baseline|premium>] \
  [--enrich] \
  [--tags <csv>] \
  [--concepts <csv>] \
  [--max-files <n>] \
  [--fts-only] \
  [--follow]
```

**Notes**

* Deterministic ordering is required (stable traversal + sort).
* `--max-files` selects the first N files in that deterministic order.

### Search

#### `shiro search`

```bash
shiro search <query> \
  [--vector|--bm25|--hybrid] \
  [--limit <n>] \
  [--expand] \
  [--max-blocks <n>] \
  [--max-chars <n>] \
  [--tag <tag>] \
  [--concept <concept_id>] \
  [--doc <doc_id>]
```

**Defaults**

* mode: `--hybrid` (RRF fusion)
* `limit`: 10
* `expand`: off
* `max-blocks`: 12 (when expand)
* `max-chars`: 8000 (when expand)

**Search result fields (minimal)**

* stable IDs: `result_id`, `doc_id`, `segment_id`, `block_id`
* location: `span`, optional `page_range`
* scores: vector/bm25 ranks + fused score
* optional expanded context

### Read

#### `shiro read`

```bash
shiro read <doc_id|title> [--outline|--text|--blocks]
```

* `--outline`: structure summary (best-effort on PDFs)
* `--text`: canonical text (may be truncated with `full_output`)
* `--blocks`: list of blocks + reading order indices

### Explain

#### `shiro explain`

```bash
shiro explain <result_id>
```

Must include:

* scoring breakdown (vector/bm25/fused)
* expansion trace (rules + included block ids)
* any taxonomy boosts (concept ids + depths)

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

### Taxonomy (SKOS)

#### `shiro taxonomy list`

```bash
shiro taxonomy list [--limit <n>]
```

#### `shiro taxonomy tree`

```bash
shiro taxonomy tree [<concept_id>] [--depth <n>]
```

#### `shiro taxonomy search`

```bash
shiro taxonomy search <query> [--limit <n>]
```

#### `shiro taxonomy add`

```bash
shiro taxonomy add <concept_id> --label <prefLabel> [--broader <broader_id>] [--alt <csv>] [--note <text>]
```

#### `shiro taxonomy assign`

```bash
shiro taxonomy assign <doc_id|title> <concept_id...> [--confidence <0.0-1.0>] [--source <manual|enriched>]
```

#### `shiro taxonomy proposed`

```bash
shiro taxonomy proposed [--limit <n>]
```

#### `shiro taxonomy accept`

```bash
shiro taxonomy accept <concept_id> --label <prefLabel> [--broader <broader_id>] [--alt <csv>] [--note <text>]
```

#### `shiro taxonomy reject`

```bash
shiro taxonomy reject <concept_id>
```

### Config

#### `shiro config show`

```bash
shiro config show
```

#### `shiro config get`

```bash
shiro config get <key>
```

#### `shiro config set`

```bash
shiro config set <key> <value>
```

### Maintenance

#### `shiro doctor`

```bash
shiro doctor [--verify-vector] [--repair]
```

#### `shiro reindex`

```bash
shiro reindex [--fts] [--vector] [--follow]
```

### MCP server

#### `shiro mcp`

```bash
shiro mcp
```

MCP responses must follow the same envelope shape (`ok/command/result/next_actions` or error variant).

### Shell completions

```bash
shiro completions <bash|zsh|fish|powershell|elvish>
```

## Stable error codes

* `E_PARSE_PDF`
* `E_PARSE_MD`
* `E_INVALID_IR`
* `E_STORE_CORRUPT`
* `E_INDEX_BUILD_FTS`
* `E_INDEX_BUILD_VEC`
* `E_EMBED_FAIL`
* `E_ENRICH_FAIL`
* `E_TAXONOMY_CYCLE`
* `E_LOCK_BUSY`