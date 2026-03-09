<h1 align="center">shiro</h1>

<p align="center">
  <strong>Local-first knowledge engine for structured document retrieval.</strong>
</p>

<p align="center">
  <a href="https://github.com/sanurb/shiro/actions"><img src="https://img.shields.io/github/actions/workflow/status/sanurb/shiro/ci.yml?branch=master&style=flat-square&logo=github&color=181717" alt="CI Status"></a>
  <a href="https://github.com/sanurb/shiro/releases"><img src="https://img.shields.io/github/v/release/sanurb/shiro?style=flat-square&logo=rust&color=e44d26" alt="Latest Release"></a>
  <img src="https://img.shields.io/badge/License-MIT-4caf50?style=flat-square" alt="License">
</p>

---

Shiro transforms PDFs and Markdown files into a unified, structure-aware searchable base on your local machine. Documents are parsed into a BlockGraph intermediate representation that preserves reading order, heading hierarchy, and block relationships. Retrieval is powered by BM25 full-text search via Tantivy, with SKOS taxonomy support and heuristic enrichment. Every command emits deterministic JSON to stdout wrapped in a HATEOAS envelope, making shiro a first-class building block for AI agents, shell pipelines, and automation toolchains.

## Key Differentiators

- **Structure-Aware IR** -- Documents are modeled as hierarchical blocks (headings, paragraphs, code, tables) with byte-level spans, enabling precise context windowing for LLM applications.
- **Deterministic JSON CLI** -- Every command outputs a structured JSON envelope to stdout. No human-readable mode. Built for `jq`, scripts, and agent consumption.
- **HATEOAS Navigation** -- Every response includes `next_actions` with typed parameter templates, enabling AI agents to discover and chain commands dynamically.
- **Zero-API Dependency** -- Parsing, indexing, and search all run locally. No data leaves your machine.
- **Pluggable Parsers** -- Built-in support for Markdown (pulldown-cmark), PDF (pdf-extract), and plain text, plus a Docling adapter for structured PDF extraction via external Python subprocess.

## Installation

### Option A: Shell script (prebuilt binaries)

```bash
curl -sSfL https://raw.githubusercontent.com/sanurb/shiro/master/install.sh | sh
```

Detects your OS and architecture, downloads the latest release from GitHub, and installs the `shiro` binary into `~/.local/bin`. Override with `SHIRO_INSTALL_DIR`:

```bash
SHIRO_INSTALL_DIR=/usr/local/bin \
  curl -sSfL https://raw.githubusercontent.com/sanurb/shiro/master/install.sh | sh
```

### Option B: npm

```bash
npm install -g @sanurb/shiro-cli
```

The npm package does not bundle the binary. A `postinstall` script automatically downloads the correct platform binary from GitHub Releases.

### Option C: Cargo (build from source)

```bash
cargo install shiro-cli
```

The crate name is `shiro-cli`, the npm package is `@sanurb/shiro-cli`, but the executable you run is `shiro`.

## Quick Start

Initialize a knowledge base and ingest documents:

```bash
shiro init
shiro ingest ~/Documents/KnowledgeBase
```

Search for a concept:

```bash
shiro search "distributed consensus"
```

```json
{
  "ok": true,
  "result": {
    "query": "distributed consensus",
    "results": [
      {
        "result_id": "r:a1b2c3",
        "doc_id": "d:9f8e7d",
        "block_idx": 4,
        "block_kind": "paragraph",
        "span_start": 1024,
        "span_end": 1280,
        "snippet": "Raft achieves consensus by electing a leader...",
        "scores": {
          "bm25_score": 12.34,
          "bm25_rank": 1,
          "fused_score": 12.34,
          "fused_rank": 1
        },
        "context_window": null
      }
    ]
  },
  "next_actions": [
    {"action": "read", "params": {"id": "d:9f8e7d"}},
    {"action": "explain", "params": {"result_id": "r:a1b2c3"}}
  ]
}
```

Read the full document:

```bash
shiro read d:9f8e7d
```

Explain how a result was scored:

```bash
shiro explain r:a1b2c3
```

```json
{
  "ok": true,
  "result": {
    "result_id": "r:a1b2c3",
    "query": "distributed consensus",
    "query_digest": "blake3:...",
    "fts_generation": 7,
    "doc_id": "d:9f8e7d",
    "block_idx": 4,
    "block_kind": "paragraph",
    "span_start": 1024,
    "span_end": 1280,
    "bm25_score": 12.34,
    "bm25_rank": 1,
    "fused_score": 12.34,
    "fused_rank": 1,
    "retrieval_trace": {
      "pipeline": "bm25_only",
      "stages": ["tokenize", "bm25_rank"],
      "fusion": null
    }
  },
  "next_actions": [...]
}
```

## Search and Retrieval

Shiro search returns block-level results. Each hit identifies the exact block within a document (`block_idx`, `block_kind`) and byte span (`span_start`, `span_end`), along with BM25 scores and ranks.

### Context expansion

Use `--expand` to include surrounding blocks for richer context:

```bash
shiro search "error handling" --expand
```

Context expansion defaults to `max_blocks=12` and `max_chars=8000`. When enabled, each result includes a `context_window` field with the expanded text.

### Scoring

Every result carries a `scores` object:

| Field | Description |
|-------|-------------|
| `bm25_score` | Raw BM25 relevance score |
| `bm25_rank` | Rank position in BM25 results |
| `fused_score` | Fused score (currently equals `bm25_score`) |
| `fused_rank` | Fused rank (currently equals `bm25_rank`) |

**Note:** Hybrid search mode is scaffolded with RRF fusion (k=60) but currently executes BM25-only. Vector embedding infrastructure is implemented (FlatIndex, HttpEmbedder) but not yet wired into the query path. When vector search becomes available, `fused_score` and `fused_rank` will reflect the combined ranking.

## Explainability

Every search result can be explained:

```bash
shiro explain <result_id>
```

The response includes a `retrieval_trace` object describing the full scoring pipeline:

- **pipeline** -- Which retrieval path was used (e.g., `bm25_only`)
- **stages** -- Ordered list of processing stages applied
- **fusion** -- Fusion details when multiple sources contribute (null when single-source)

Each source's contribution is recorded, enabling full audit of how results are ranked.

## Parsing and Adapters

### Built-in parsers

| Parser | Format | Technology |
|--------|--------|------------|
| `markdown` | `.md` files | [pulldown-cmark](https://github.com/raphlinus/pulldown-cmark) with BlockGraph IR |
| `pdf` | `.pdf` files | [pdf-extract](https://crates.io/crates/pdf-extract) |
| `plaintext` | `.txt` and other text | Paragraph-boundary segmentation |

### Docling adapter (structured PDF)

The `shiro-docling` crate provides a high-fidelity PDF parser that delegates to the [Docling](https://github.com/DS4SD/docling) Python library via subprocess. It extracts tables, figures, and reading order that basic PDF extraction misses.

**Setup:**

```bash
pip install docling
```

**Usage:**

```bash
shiro add document.pdf --parser premium
shiro ingest ./papers --parser premium
```

Docling requires the external `docling` Python binary to be available on `PATH`. The adapter communicates via subprocess, not network calls.

## SKOS Taxonomy

Shiro supports SKOS-style taxonomies for organizing documents by concept:

```bash
# Add a concept
shiro taxonomy add "Machine Learning"

# List all concepts
shiro taxonomy list

# Define relationships between concepts
shiro taxonomy relations "Machine Learning" --broader "Computer Science"

# Assign a concept to a document
shiro taxonomy assign d:9f8e7d "Machine Learning"

# Import a SKOS taxonomy file
shiro taxonomy import taxonomy.ttl
```

## Enrichment

Heuristic enrichment extracts metadata from document content:

```bash
shiro enrich d:9f8e7d
```

The heuristic provider analyzes content to extract:

- **title** -- Derived from headings or first significant text
- **summary** -- Condensed description of the document
- **tags** -- Keywords extracted from content analysis

Enrichment is heuristic-only; no external AI services are called.

## Configuration

Manage configuration with `shiro config`:

```bash
shiro config get search.limit
shiro config set search.limit 20
```

### Configuration keys

| Key | Type | Description |
|-----|------|-------------|
| `search.limit` | u32 | Maximum search results |
| `embed.base_url` | string | Embedding service base URL |
| `embed.model` | string | Embedding model name |
| `embed.dimensions` | usize | Expected embedding dimensions |
| `embed.api_key` | string | Embedding service API key |

The `embed.*` keys configure the vector embedding infrastructure. They take effect once vector search is wired into the query path.

## Code Mode (MCP)

Shiro exposes a [Model Context Protocol](https://modelcontextprotocol.io) server over stdio with exactly two tools:

| Tool | Purpose |
|------|---------|
| `shiro.search` | Discover SDK operations, schemas, and examples |
| `shiro.execute` | Run a DSL program against the knowledge base |

Start the MCP server:

```bash
shiro mcp
```

### DSL

The `shiro.execute` tool accepts a program written in a small deterministic DSL with these statements:

| Statement | Description |
|-----------|-------------|
| `let` | Bind a variable to a call result |
| `call` | Invoke an SDK operation |
| `if` | Conditional branching |
| `for_each` | Iterate over a collection |
| `return` | Produce the program output |

### Limits

| Limit | Value |
|-------|-------|
| Max steps | 200 |
| Max iterations | 100 |
| Max output bytes | 1 MiB |
| Timeout | 30s |

### Example program

Search, read the top hit, and return a summary:

```json
{
  "program": [
    {"type": "let", "name": "results", "call": {"op": "search", "params": {"query": "error handling", "limit": 3}}},
    {"type": "let", "name": "top_hit", "call": {"op": "read", "params": {"id": "$results.hits.0.doc_id"}}},
    {"type": "return", "value": {"query": "$results.query", "title": "$top_hit.title", "content": "$top_hit.content"}}
  ]
}
```

### Guarantees

- **Two tools only** -- `shiro.search` and `shiro.execute`
- **Typed SDK** -- All operations backed by schemars-derived JSON Schemas
- **Deterministic outputs** -- Search results are scored and name-sorted
- **Strict validation** -- Unknown fields rejected, all inputs schema-checked
- **Stable error codes** -- Every error maps to an `E_*` code
- **Safe execution** -- No arbitrary code; hard limits on steps, iterations, bytes, and time

## Project Status

| Feature | Status | Notes |
|---------|--------|-------|
| Markdown parsing | Stable | pulldown-cmark with BlockGraph IR |
| PDF parsing | Stable | pdf-extract with loss detection |
| Docling adapter | Stable | Structured PDF via Python subprocess (`pip install docling`) |
| Plain text indexing | Stable | Paragraph-boundary segmentation |
| BM25 full-text search | Stable | Tantivy engine |
| JSON / HATEOAS layer | Stable | Structured output with `next_actions` on every response |
| SKOS taxonomy | Implemented | Add, list, relations, assign, import |
| Completions | Implemented | Shell completions generation |
| Enrichment | Heuristic-only | Title, summary, tags from content analysis |
| MCP server | Code Mode | Stdio JSON-RPC 2.0 with DSL interpreter |
| Hybrid search | BM25-only (scaffold) | RRF fusion (k=60) scaffolded; falls back to BM25 at runtime |
| Vector embedding | Infrastructure-only | FlatIndex and HttpEmbedder implemented; not wired into query path |
| BlockGraph persistence | Stable | First-class stored representation (store schema v6) |
| Context expansion | Stable | `--expand` with configurable max_blocks and max_chars |
| Processing fingerprints | Stable | BLAKE3-based dedup on every add/ingest |

## Architecture

Shiro is a Rust workspace with eight crates:

| Crate | Role |
|-------|------|
| `shiro-core` | Domain types, config, error handling |
| `shiro-store` | SQLite persistence (schema v6), BlockGraph storage |
| `shiro-index` | Tantivy BM25 full-text search, generation tracking, staging/promote |
| `shiro-parse` | Markdown, PDF, and plaintext parsers |
| `shiro-docling` | Docling subprocess adapter for structured PDF |
| `shiro-embed` | FlatIndex (vector storage), HttpEmbedder, embedding traits |
| `shiro-sdk` | Operation registry, DSL interpreter, RRF fusion |
| `shiro-cli` | CLI entry point (16 commands), published to crates.io |

For design decisions and ADRs, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Contributing

Contributions that uphold speed, privacy, and structural integrity are welcome.

1. Review the [Architecture](docs/ARCHITECTURE.md) for design patterns and ADRs.
2. Review the [CLI Reference](docs/CLI.md) for the output contract.
3. Ensure all changes pass the quality gate: `cargo test && cargo clippy`.
4. Open a Pull Request with a clear description of the impact.

## License

Licensed under the MIT License. See [LICENSE](LICENSE).
