# MCP Agent Guide

shiro’s MCP server exposes a single tool `execute` using the **codemode pattern**: agents send JavaScript to run server-side; only the returned value is emitted.

This minimizes round trips and lets agents compose multi-step workflows with normal control flow.


## Why codemode?

Traditional MCP integrations require one tool call per operation (add → enrich → search → assign concept → reindex…). Codemode collapses that into one execution:

**Multiple tool calls (typical MCP):**
1. add document  
2. run enrichment  
3. search  
4. assign taxonomy  
5. explain result  

**Single codemode execution:**
```js
const doc = await docs.add({ path: "~/docs/paper.pdf", enrich: true });

const hits = await search.query("reading order", { mode: "hybrid", expand: true, limit: 5 });

await taxonomy.assign(doc.doc_id, ["ai/rag"], { source: "manual", confidence: 0.9 });

return { doc, hits };
````

Benefits:

* fewer tool invocations
* simpler error recovery (try/catch once)
* better composition (loops, branching, batching)


## Running the MCP server

The MCP server is a mode of the `shiro` binary:

```bash
shiro mcp
```

Transport: **stdio** (recommended).


## The `execute` tool

### Input

* `code` (string, required): JavaScript program
* `timeout_ms` (number, optional): execution timeout (default 30_000, max 120_000)
* `home` (string, optional): overrides `SHIRO_HOME` for this execution

### Output (always JSON)

The tool always returns an **agent-first envelope**:

**Success**

```json
{
  "ok": true,
  "command": "shiro.mcp.execute",
  "result": {},
  "next_actions": [
    { "command": "shiro <...>", "description": "..." }
  ]
}
```

**Error**

```json
{
  "ok": false,
  "command": "shiro.mcp.execute",
  "error": { "message": "...", "code": "E_SOMETHING" },
  "fix": "Plain-language suggested fix",
  "next_actions": [
    { "command": "shiro <...>", "description": "..." }
  ]
}
```

### HATEOAS: `next_actions` templates

`next_actions[].command` uses POSIX/docopt-style placeholders:

* `<required>` positional args
* `[--flag <value>]` optional flags

If `params` is present, it is a **typed template**:

```json
{
  "command": "shiro read <doc_id> --outline",
  "description": "Inspect extracted structure",
  "params": {
    "doc_id": { "value": "doc_...", "description": "Document ID" }
  }
}
```

### Truncation (context protection)

If `result` would exceed output limits, shiro truncates and returns a pointer:

```json
{
  "ok": true,
  "command": "shiro.mcp.execute",
  "result": {
    "showing": 20,
    "total": 981,
    "truncated": true,
    "full_output": "/tmp/shiro-output-abc123.json",
    "items": [ ... ]
  },
  "next_actions": [
    {
      "command": "shiro search <query> [--limit <n>]",
      "description": "Request a smaller page",
      "params": { "n": { "default": 20 } }
    }
  ]
}
```


## Execution environment (sandbox)

Codemode runs in an embedded JS VM (no Node runtime).

### Available globals

* `docs` — document ingest/read/list/remove
* `search` — vector/BM25/hybrid search + expansion
* `taxonomy` — SKOS operations + assignments + proposal review
* `config` — read/update configuration
* `maintenance` — doctor/reindex/repair helpers
* `jobs` — long-running operation tracking
* `shiro` — helpers (ids, templates, paging, etc.)

### Security constraints (default)

* **No network access** (`fetch` disabled)
* No arbitrary filesystem access from JS (only via shiro APIs)
* CPU/memory bounded
* Time-limited execution (`timeout_ms`)


## Type definitions (reference)

> These are documentation types. The runtime is JavaScript; return values must be JSON-serializable.

```ts
// IDs are stable strings. Suggested format:
// doc_<base32(blake3)> and seg_<base32(blake3)>
type DocId = string;
type SegmentId = string;
type ConceptId = string;
type JobId = string;

type SearchMode = "vector" | "bm25" | "hybrid";
type ParserMode = "baseline" | "premium";

interface NextAction {
  command: string;
  description: string;
  params?: Record<string, {
    value?: string | number | boolean;
    default?: string | number | boolean;
    enum?: Array<string | number>;
    description?: string;
  }>;
}

interface Envelope<T> {
  ok: true;
  command: string;
  result: T;
  next_actions: NextAction[];
}

interface ErrorEnvelope {
  ok: false;
  command: string;
  error: { message: string; code: string };
  fix?: string;
  next_actions: NextAction[];
}

interface Document {
  doc_id: DocId;
  source_uri: string;
  source_hash: string;
  title?: string;
  summary?: string;
  tags?: string[];
  doc_type?: string;
  created_at: string;  // ISO8601
  ingested_at: string; // ISO8601
  status: "STAGED" | "INDEXING" | "READY" | "FAILED" | "DELETED";
}

interface AddResult {
  doc: Document;
  changed: boolean;
}

interface IngestResult {
  added: number;
  ready: number;
  failed: number;
  failures?: Array<{ source: string; code: string; message: string }>;
}

interface SearchHit {
  result_id: string;     // ephemeral per query
  doc_id: DocId;
  segment_id: SegmentId;
  block_id: number;
  span: { start: number; end: number };
  page_range?: { start: number; end: number };
  title?: string;
  snippet: string;
  scores: {
    vector?: { score: number; rank: number };
    bm25?: { score: number; rank: number };
    fused?: number;
  };
  expanded?: {
    included_block_ids: number[];
    text: string;
  };
}

interface SearchResult {
  query: string;
  mode: SearchMode;
  results: SearchHit[];
  truncated?: boolean;
}

interface ExplainResult {
  result_id: string;
  doc_id: DocId;
  segment_id: SegmentId;
  block_id: number;
  span: { start: number; end: number };
  scores: {
    vector?: { score: number; rank: number };
    bm25?: { score: number; rank: number };
    fused?: number;
    taxonomy_boost?: number;
  };
  expansion: {
    rules_fired: string[];
    included_block_ids: number[];
    budgets: { max_blocks: number; max_chars: number; used_blocks: number; used_chars: number };
  };
  taxonomy?: {
    matched_concepts?: Array<{ concept_id: ConceptId; depth: number; contribution: number }>;
  };
}

interface Concept {
  concept_id: ConceptId;
  pref_label: string;
  alt_labels?: string[];
  scope_note?: string;
}

interface ProposedConcept {
  concept_id: ConceptId;
  pref_label: string;
  suggested_broader?: ConceptId;
  reason?: string;
}

interface Job {
  job_id: JobId;
  kind: "ingest" | "reindex" | "enrich";
  status: "queued" | "running" | "completed" | "failed";
  created_at: string;
  updated_at: string;
  result?: unknown;
  error?: { code: string; message: string };
}
```


## API reference

### `docs`

```js
// Add a single document (path or URL)
await docs.add({
  path: "<path|url>",
  parser: "baseline" | "premium",   // default: baseline
  enrich: boolean,                  // default: false
  tags: string[],                   // optional
  concepts: string[],               // optional (concept IDs)
  fts_only: boolean                 // default: false
}) -> AddResult

// Bulk ingest (may return a Job when long-running)
await docs.ingest({
  dirs: string[],
  glob: string,                     // default: **/*.{pdf,md}
  parser: "baseline" | "premium",
  enrich: boolean,
  tags: string[],
  concepts: string[],
  max_files: number,
  fts_only: boolean
}) -> IngestResult | Job

// List documents
await docs.list({
  limit: number,                    // default: 20
  tag: string,
  concept: string
}) -> { showing, total, truncated, items: Document[], full_output? }

// Read a document
await docs.read({
  id: "<doc_id|title>",
  mode: "outline" | "text" | "blocks",
  limit_chars: number               // for text mode; default bounded
}) -> object

// Remove a document
await docs.remove({ id: "<doc_id|title>", purge: boolean }) -> { removed: true }
```

### `search`

```js
await search.query("<query>", {
  mode: "hybrid" | "vector" | "bm25",    // default: hybrid
  limit: number,                         // default: 10
  topk_vec: number,                      // default: 200
  topk_bm25: number,                     // default: 200
  rrf_k: number,                         // default: 60
  expand: boolean,                        // default: false
  max_blocks: number,                     // default: 12
  max_chars: number,                      // default: 8000
  tag: string,
  concept: string,
  doc: string
}) -> SearchResult

await search.explain("<result_id>") -> ExplainResult
```

### `taxonomy` (SKOS)

```js
await taxonomy.list({ limit: number }) -> { showing, total, truncated, items: Concept[] }

await taxonomy.tree({ root: "<concept_id>", depth: number }) -> object

await taxonomy.search("<query>", { limit: number }) -> Concept[]

await taxonomy.add({
  concept_id: "<id>",
  label: "<prefLabel>",
  broader: "<concept_id>",
  alt: string[],
  note: string
}) -> Concept

await taxonomy.assign("<doc_id|title>", ["concept/id", "..."], {
  confidence: number,             // default: 1.0
  source: "manual" | "enriched"   // default: manual
}) -> { assigned: number }

await taxonomy.proposed({ limit: number }) -> ProposedConcept[]

await taxonomy.accept({
  concept_id: "<id>",
  label: "<prefLabel>",
  broader: "<concept_id>",
  alt: string[],
  note: string
}) -> Concept

await taxonomy.reject("<concept_id>") -> { rejected: true }
```

### `config`

```js
await config.show() -> object
await config.get("<key>") -> { key: string, value: unknown }
await config.set("<key>", "<value>") -> { updated: true }
```

### `maintenance`

```js
await maintenance.doctor({ verify_vector: boolean, repair: boolean }) -> object
await maintenance.reindex({ fts: boolean, vector: boolean }) -> Job | { ok: true }
```

### `jobs`

```js
await jobs.get("<job_id>") -> Job
await jobs.wait("<job_id>", { timeout_ms: number }) -> Job
await jobs.list({ status: "queued"|"running"|"completed"|"failed", limit: number }) -> Job[]
```


## Usage patterns

### 1) Add → search → explain

```js
const add = await docs.add({ path: "~/docs/paper.pdf", enrich: true });

const res = await search.query("reading order", { mode: "hybrid", expand: true, limit: 5 });

const exp = res.results.length ? await search.explain(res.results[0].result_id) : null;

return { add, top: res.results[0], exp };
```

### 2) Bulk ingest with job polling

```js
const r = await docs.ingest({ dirs: ["~/papers"], enrich: true });

if (r.job_id) {
  const done = await jobs.wait(r.job_id, { timeout_ms: 120000 });
  return done;
}

return r;
```

### 3) Accept proposed concepts and assign

```js
const proposed = await taxonomy.proposed({ limit: 50 });

for (const p of proposed) {
  // Accept with suggested broader if present; otherwise place under a known root
  await taxonomy.accept({
    concept_id: p.concept_id,
    label: p.pref_label,
    broader: p.suggested_broader ?? "meta/proposed"
  });
}

return { accepted: proposed.length };
```

### 4) Taxonomy-guided search

```js
// Search within a concept scope (config may expand to descendants)
const hits = await search.query("disk index", { concept: "systems/storage", expand: true, limit: 10 });
return hits;
```


## Error handling

`execute` returns an error envelope on uncaught errors. Inside codemode, use try/catch:

```js
try {
  return await docs.add({ path: "~/docs/bad.pdf" });
} catch (e) {
  // You can return your own structured result; shiro will still wrap it.
  return { recovered: false, message: String(e) };
}
```

Common recoveries:

* `E_LOCK_BUSY`: another writer running → retry later or run `doctor`
* `E_PARSE_PDF`: try `parser: "premium"` if configured
* `E_EMBED_FAIL`: verify model cache/config; run `maintenance.doctor()`
* `E_TAXONOMY_CYCLE`: revise broader relation; do not attempt to force insert


## Best practices

1. **Prefer codemode composition**
   Do multi-step workflows in one `execute` call.

2. **Use `next_actions`**
   If the result includes IDs (doc/job/result), shiro suggests follow-up command templates.

3. **Bound your outputs**
   Always pass `limit` for list/search. If you need more, paginate and avoid dumping huge text.

4. **Treat enrichment as advisory**
   Use enrichment to populate metadata; do not assume it is deterministic.


## Limitations

* Default execution timeout: 30s (max 120s)
* Output is bounded; large results are truncated with `full_output` pointers
* No network calls from JS
* No direct filesystem access from JS (use shiro APIs)
* Single-writer library lock; concurrent writes return `E_LOCK_BUSY`

## See also

* [CLI Reference](CLI.md) — agent-first CLI contract and commands
* [Architecture](ARCHITECTURE.md) — core design