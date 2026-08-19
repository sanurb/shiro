# MCP Server — Code Mode

> **Status:** Implemented (v0.3.0). `shiro capabilities` reports `"mcp_server": "code_mode"`. The `shiro mcp` command starts a JSON-RPC 2.0 stdio server with two tools: `shiro.search` and `shiro.execute`.

The MCP server exposes shiro's document library and a safe execution environment to AI assistants (Claude, Cursor, etc.) via the [Model Context Protocol](https://modelcontextprotocol.io).

## Protocol

- **Transport:** JSON-RPC 2.0 over stdio
- **Input:** Newline-delimited JSON on stdin
- **Output:** JSON + newline on stdout
- **Modern protocol:** `2026-07-28` (per-request `_meta`, `server/discover`, stateless requests)
- **Legacy compatibility:** `2025-11-25`, `2025-06-18`, and `2024-11-05` initialization handshakes
- **Schemas:** JSON Schema 2020-12 input/output schemas; tool results include both text and `structuredContent`

The modern behavior follows the official [versioning contract](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning), including `-32022` responses with supported versions.

## Lifecycle

Modern clients call `server/discover` (optional but recommended on stdio), then include these fields in every request's `params._meta`:

```json
{
  "io.modelcontextprotocol/protocolVersion": "2026-07-28",
  "io.modelcontextprotocol/clientInfo": {"name": "host", "version": "1"},
  "io.modelcontextprotocol/clientCapabilities": {}
}
```

The server returns `resultType: "complete"` and server identity metadata on every successful response. Unsupported modern versions return `-32022`; missing required capabilities return `-32602`. Legacy clients may still use `initialize` and `notifications/initialized` before `tools/list`/`tools/call`. Both eras expose exactly two tools.

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

**Output:** `structuredContent.results` contains `SpecSearchResult` objects ranked by score descending with name ascending as tie-break. Each result includes schemas, examples, and an `authority` value of `read` or `write`. The same JSON is serialized in a text content block for compatibility.

### `shiro.execute`

Execute a DSL program against the shiro library.

**Input:**

```json
{
  "program": [
    { "type": "let", "name": "results", "call": { "op": "search", "params": { "query": "machine learning", "mode": "bm25", "limit": 5 } } },
    { "type": "return", "value": "$results" }
  ],
  "limits": {
    "max_steps": 100,
    "max_iterations": 50,
    "timeout_ms": 15000
  }
}
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `program` | Node[] | yes | — | DSL program (array of nodes) |
| `limits` | Limits | no | defaults | Override execution limits |
| `actor_id` | string | for writes | — | Actor identity recorded for each write-class operation |
| `approval_id` | string | for writes | — | Host/policy approval reference recorded for each write |

The host must also start Shiro with `shiro mcp --allow-writes`. Without all three authority signals, write-class operations fail closed. Authorization and outcomes are append-only audited with a generated run ID. Read-only programs require none of these fields.

**Output:** `structuredContent.result` contains an `ExecutionResult`; the same JSON is serialized as text for compatibility:

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
{ "type": "let", "name": "docs", "call": { "op": "list", "params": {} } }
```

#### `call` — Execute an operation (result discarded)

```json
{ "type": "call", "op": "search", "params": { "query": "neural networks", "mode": "bm25", "limit": 5 } }
```

#### `if` — Conditional execution

```json
{
  "type": "if",
  "condition": "$results",
  "then": [{ "type": "return", "value": "$results" }],
  "else": [{ "type": "return", "value": "no results" }]
}
```

#### `for_each` — Iterate over a collection

```json
{
  "type": "for_each",
  "collection": "$docs.documents",
  "item": "doc",
  "body": [
    { "type": "call", "op": "read", "params": { "id": "$doc.doc_id" } }
  ]
}
```

#### `return` — Return a value from the program

```json
{ "type": "return", "value": "$results.hits.0.title" }
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
| `timeout_ms` | 30000 | Wall-clock execution timeout |

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
| `-32022` | Unsupported modern protocol version (response includes supported versions) |

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
