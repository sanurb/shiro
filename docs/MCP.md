# MCP Server — Code Mode

> **Status:** Implemented (v0.3.0). `shiro capabilities` reports `"mcp_server": "code_mode"`. The `shiro mcp` command starts a JSON-RPC 2.0 stdio server with two tools: `shiro.search` and `shiro.execute`.

The MCP server exposes shiro's document library and a safe execution environment to AI assistants (Claude, Cursor, etc.) via the [Model Context Protocol](https://modelcontextprotocol.io).

## Protocol

- **Transport:** JSON-RPC 2.0 over stdio
- **Input:** Newline-delimited JSON on stdin
- **Output:** JSON + newline on stdout
- **Protocol version:** `2024-11-05`

## Lifecycle

```
Client                              Server
  │                                    │
  │─── initialize ────────────────────>│
  │<── capabilities, serverInfo ───────│
  │                                    │
  │─── notifications/initialized ─────>│
  │    (no response — notification)    │
  │                                    │
  │─── tools/list ────────────────────>│
  │<── shiro.search, shiro.execute ───│
  │                                    │
  │─── tools/call {shiro.search} ────>│
  │<── SpecSearchResult[] ────────────│
  │                                    │
  │─── tools/call {shiro.execute} ───>│
  │<── ExecutionResult ───────────────│
```

1. Client sends `initialize` → server responds with capabilities (`tools: {}`), server info (`name: "shiro"`, `version`).
2. Client sends `notifications/initialized` → server acknowledges (no response for notifications).
3. Client sends `tools/list` → server returns exactly two tools: `shiro.search` and `shiro.execute`.
4. Client sends `tools/call` → server dispatches to the requested tool with strict input validation.
5. Unknown methods → JSON-RPC error code `-32601` ("method not found").

## Tools

### `shiro.search`

Search for available operations by keyword.

**Input:**

```json
{
  "query": "search for documents",
  "limit": 5
}
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `query` | string | yes | — | Search terms (AND semantics) |
| `limit` | integer | no | 10 | Max results to return |

**Output:** Array of `SpecSearchResult` objects, ranked by score descending with name ascending as tie-break. Each result includes the operation spec, JSON Schema for inputs/outputs, and usage examples.

### `shiro.execute`

Execute a DSL program against the shiro library.

**Input:**

```json
{
  "program": [
    { "let": { "name": "results", "call": { "op": "search", "params": { "query": "machine learning" } } } },
    { "return": { "value": "$results" } }
  ],
  "limits": {
    "max_steps": 100,
    "max_iterations": 50,
    "timeout_secs": 15
  }
}
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `program` | Node[] | yes | — | DSL program (array of nodes) |
| `limits` | Limits | no | defaults | Override execution limits |

**Output:** `ExecutionResult` containing:

| Field | Type | Description |
|-------|------|-------------|
| `value` | any | Return value from the program |
| `steps_executed` | integer | Total DSL steps executed |
| `total_duration_us` | integer | Wall-clock execution time in microseconds |
| `trace` | StepTrace[] | Per-step execution trace with timing, op name, args hash, result summary, error codes |

## DSL Grammar

The DSL is a JSON AST interpreted by a safe, sandboxed interpreter. All node types use `deny_unknown_fields` for strict validation.

### Node Types

#### `let` — Bind a variable to the result of a call

```json
{ "let": { "name": "docs", "call": { "op": "list", "params": {} } } }
```

#### `call` — Execute an operation (result discarded)

```json
{ "call": { "op": "search", "params": { "query": "neural networks" } } }
```

#### `if` — Conditional execution

```json
{
  "if": {
    "condition": "$results",
    "then": [
      { "return": { "value": "$results" } }
    ],
    "else": [
      { "return": { "value": "no results" } }
    ]
  }
}
```

#### `for_each` — Iterate over a collection

```json
{
  "for_each": {
    "collection": "$docs",
    "item": "doc",
    "body": [
      { "call": { "op": "read", "params": { "doc_id": "$doc.id" } } }
    ]
  }
}
```

#### `return` — Return a value from the program

```json
{ "return": { "value": "$results.0.title" } }
```

### Variable Substitution

Variables are referenced with `$` prefix and support path traversal:

- `$var` — simple variable reference
- `$var.field` — object field access
- `$var.0` — array index access
- `$var.path.0.field` — chained path traversal

## Limits and Safety

The DSL interpreter enforces hard limits to prevent abuse:

| Limit | Default | Description |
|-------|---------|-------------|
| `max_steps` | 200 | Maximum total DSL steps (all node evaluations) |
| `max_iterations` | 100 | Maximum iterations per `for_each` loop |
| `max_output_bytes` | 1 MiB | Maximum size of the return value |
| `timeout` | 30s | Wall-clock execution timeout |

**Safety guarantees:**

- No arbitrary code execution — JSON AST interpreter only
- `deny_unknown_fields` on all DSL nodes rejects typos and injection attempts
- All limits are enforced at the interpreter level; clients can lower but not raise defaults
- Structured execution trace provides full auditability

## Error Mapping

Errors are returned as JSON-RPC error responses with structured codes:

| ErrorCode | as_str() | Description |
|-----------|----------|-------------|
| `ExecutionLimit` | `E_EXECUTION_LIMIT` | Execution limit exceeded (steps, iterations, output size, or timeout) |
| `DslError` | `E_DSL_ERROR` | DSL interpretation error (unknown node, invalid variable, type error) |
| `NotFound` | `E_NOT_FOUND` | Referenced document or resource not found |
| `InvalidInput` | `E_INVALID_INPUT` | Invalid tool input (schema validation failure) |
| `Mcp` | `E_MCP` | MCP protocol-level error |

JSON-RPC error codes:

| Code | Meaning |
|------|---------|
| `-32600` | Invalid request (malformed JSON-RPC) |
| `-32601` | Method not found |
| `-32602` | Invalid params (unknown fields, schema mismatch) |
| `-32603` | Internal error |

## Client Configuration

### Claude Desktop

Add to your Claude Desktop config (`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS):

```json
{
  "mcpServers": {
    "shiro": {
      "command": "shiro",
      "args": ["mcp"]
    }
  }
}
```

### Cursor

Add to Cursor's MCP settings (Settings → MCP Servers):

```json
{
  "mcpServers": {
    "shiro": {
      "command": "shiro",
      "args": ["mcp"]
    }
  }
}
```

### Custom Home Directory

To use a non-default library location:

```json
{
  "mcpServers": {
    "shiro": {
      "command": "shiro",
      "args": ["--home", "/path/to/library", "mcp"]
    }
  }
}
```

## See Also

- [CLI Reference](CLI.md) for the agent-first CLI interface
- [Architecture](ARCHITECTURE.md) for design context
