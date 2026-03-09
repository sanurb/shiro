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

Shiro transforms PDFs and Markdown files into a unified, structure-aware searchable base on your local machine. Documents are parsed into a BlockGraph intermediate representation that preserves reading order, heading hierarchy, and block relationships. Retrieval combines BM25 full-text search (Tantivy) with local vector embeddings (FastEmbed/ONNX) and optional cross-encoder reranking, fused via Reciprocal Rank Fusion. Embeddings and reranking run entirely on-device — no data leaves your machine. Every command emits deterministic JSON to stdout wrapped in a HATEOAS envelope, making shiro a first-class building block for AI agents, shell pipelines, and automation toolchains.

## Key Differentiators

- **Structure-Aware IR** -- Documents are modeled as hierarchical blocks (headings, paragraphs, code, tables) with byte-level spans, enabling precise context windowing for LLM applications.
- **Deterministic JSON CLI** -- Every command outputs a structured JSON envelope to stdout. No human-readable mode. Built for `jq`, scripts, and agent consumption.
- **HATEOAS Navigation** -- Every response includes `next_actions` with typed parameter templates, enabling AI agents to discover and chain commands dynamically.
- **Local Semantic Retrieval** -- Provider-agnostic embedding and reranking boundary with FastEmbed (ONNX Runtime) as the first local implementation. Hybrid search fuses BM25 + vector rankings; reranking refines top-k quality with cross-encoder models. No external service required.
- **Zero-API Dependency** -- Parsing, indexing, embedding, reranking, and search all run locally. No data leaves your machine.
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

Search for a concept (defaults to hybrid mode when embeddings are configured):

```bash
shiro search "distributed consensus"
```

```json
{
  "ok": true,
  "result": {
    "query": "distributed consensus",
    "mode": "hybrid",
    "retrieval_info": {
      "bm25_active": true,
      "vector_active": true,
      "reranker_active": false,
      "reranker_model": null
    },
    "results": [
      {
        "result_id": "res_a1b2c3d4e5f67890",
        "doc_id": "d:9f8e7d",
        "block_idx": 4,
        "block_kind": "PARAGRAPH",
        "span": {"start": 1024, "end": 1280},
        "snippet": "Raft achieves consensus by electing a leader...",
        "scores": {
          "bm25": {"score": 12.34, "rank": 1},
          "vector": {"score": 0.87, "rank": 3},
          "fused": {"score": 0.0318, "rank": 1}
        },
        "context_window": []
      }
    ]
  },
  "next_actions": [
    {"action": "shiro explain <result_id>", "description": "Explain why this result matched"},
    {"action": "shiro list", "description": "List all documents"}
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
    "result_id": "res_a1b2c3d4e5f67890",
    "query": "distributed consensus",
    "query_digest": "blake3:3a7f...",
    "fts_generation": 7,
    "doc_id": "d:9f8e7d",
    "block_idx": 4,
    "block_kind": "PARAGRAPH",
    "scores": {
      "bm25": {"score": 12.34, "rank": 1},
      "vector": {"score": 0.87, "rank": 3},
      "fused": {"score": 0.0318, "rank": 1}
    },
    "retrieval_trace": {
      "pipeline": "hybrid",
      "stages": ["tokenize", "bm25_rank", "embed_query", "vector_rank", "rrf_fusion"],
      "fusion": {"method": "rrf", "k": 60}
    }
  },
  "next_actions": [...]
}
```

## Search and Retrieval

Shiro search returns block-level results. Each hit identifies the exact block within a document (`block_idx`, `block_kind`) and byte span, along with per-source scores and a fused ranking. Markdown and PDF content remains the source material; SQLite is the authoritative store; search indices and vector embeddings are derived artifacts that can be rebuilt at any time.

### Retrieval modes

| Mode | Flag | Behavior |
|------|------|----------|
| **Hybrid** (default) | `--mode hybrid` | BM25 + vector search, merged via Reciprocal Rank Fusion (k=60). Falls back to BM25-only when no embedder is configured. |
| **BM25** | `--mode bm25` | Keyword search only, even when embeddings are available. |
| **Vector** | `--mode vector` | Semantic similarity only. Requires a configured embedding provider. |

### Reranking

Add `--rerank` to apply a cross-encoder model after fusion. Reranking re-scores the top-k fused candidates and re-sorts by cross-encoder relevance, improving precision on the final result set.

```bash
shiro search "error handling in async Rust" --rerank
```

Reranking is optional and non-fatal — if the reranker fails to initialize, search falls back to RRF order.

### Context expansion

Use `--expand` to include surrounding blocks for richer context:

```bash
shiro search "error handling" --expand
```

Context expansion defaults to `max_blocks=12` and `max_chars=8000`. When enabled, each result includes a `context_window` field with the expanded text from the document's BlockGraph reading order.

### Scoring

Every result carries a `scores` object with per-source contributions:

| Field | Present when | Description |
|-------|-------------|-------------|
| `bm25.score` / `bm25.rank` | BM25 active | Raw Tantivy BM25 relevance score and rank |
| `vector.score` / `vector.rank` | Vector active | Cosine similarity score and rank from FlatIndex |
| `fused.score` / `fused.rank` | Always | RRF-merged score across active sources |
| `reranker.score` / `reranker.rank` | `--rerank` | Cross-encoder relevance score and final rank |

Scores are **ordinal within a single result set** — they are not calibrated probabilities and cannot be compared across queries or index generations.

### Example: hybrid search with reranking

```bash
shiro search "distributed consensus" --rerank | jq '.result.results[0].scores'
```

```json
{
  "bm25": {"score": 12.34, "rank": 1},
  "vector": {"score": 0.87, "rank": 3},
  "fused": {"score": 0.0318, "rank": 1},
  "reranker": {"score": 0.95, "rank": 1}
}
```

## Explainability

Every search result can be explained:

```bash
shiro explain <result_id>
```

The response includes a `retrieval_trace` object describing the full scoring pipeline:

- **pipeline** -- Which retrieval path was used (`hybrid`, `bm25`, or `vector`)
- **stages** -- Ordered list of processing stages applied (e.g., `tokenize → bm25_rank → embed_query → vector_rank → rrf_fusion`)
- **fusion** -- Fusion method and parameters when multiple sources contribute (`{"method": "rrf", "k": 60}`)

Each source's contribution (BM25 rank, vector rank, reranker score) is recorded per result, enabling full audit of how candidates were ranked and merged.

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

Configuration is stored as TOML at `<shiro-home>/config.toml`.

### Enabling local embeddings (FastEmbed)

Set the embedding provider to `fastembed` to enable fully local vector search with no external service:

```bash
shiro config set embed.provider fastembed
shiro config set embed.model AllMiniLML6V2    # 384-dim, fast default
```

After configuring the embedder, build the vector index from existing documents:

```bash
shiro reindex    # rebuilds FTS index (always safe to run)
```

**When you change the embedding model**, you must rebuild the vector index — vectors from different models live in incompatible spaces. Use `shiro reindex` after changing `embed.model`.

### Configuration keys

| Key | Type | Description |
|-----|------|-------------|
| `search.limit` | u32 | Maximum search results |
| `embed.provider` | string | Embedding provider: `"fastembed"` (local ONNX) or `"http"` (OpenAI-compatible endpoint) |
| `embed.model` | string | Embedding model name (e.g., `AllMiniLML6V2`, `BGEBaseENV15`, `NomicEmbedTextV15`) |
| `embed.base_url` | string | Base URL for HTTP provider (e.g., `http://localhost:11434/v1`) |
| `embed.dimensions` | usize | Expected embedding dimensions (auto-detected for FastEmbed) |
| `embed.api_key` | string | API key for HTTP provider |
| `embed.cache_dir` | string | Directory for cached ONNX models (FastEmbed) |
| `rerank.provider` | string | Reranker provider: `"fastembed"` |
| `rerank.model` | string | Reranker model name (default: `BGERerankerBase`) |
| `rerank.top_k` | usize | Number of candidates to rerank |

The embedding and reranking boundaries are **provider-agnostic**: any implementation of the `Embedder` / `Reranker` traits works. FastEmbed is the first practical local implementation; the HTTP adapter supports Ollama, llama.cpp, vLLM, or any OpenAI-compatible embedding endpoint.

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
| BM25 full-text search | Stable | Tantivy engine, block-level results |
| Hybrid search | Stable | RRF fusion (k=60) merging BM25 + vector; graceful fallback to BM25-only |
| Vector embedding | Stable | FlatIndex (cosine, JSONL-persisted) with FastEmbed (local ONNX) and HTTP adapters |
| Reranking | Stable | Post-fusion cross-encoder reranking via FastEmbed (`--rerank`) |
| JSON / HATEOAS layer | Stable | Structured output with `next_actions` on every response |
| SKOS taxonomy | Implemented | Add, list, relations, assign, import |
| Completions | Implemented | Shell completions generation |
| Enrichment | Heuristic-only | Title, summary, tags from content analysis |
| MCP server | Code Mode | Stdio JSON-RPC 2.0 with DSL interpreter |
| BlockGraph persistence | Stable | First-class stored representation (store schema v6) |
| Context expansion | Stable | `--expand` with configurable max_blocks and max_chars |
| Processing fingerprints | Stable | BLAKE3-based dedup on every add/ingest |

## Architecture

Shiro is a Rust workspace with nine crates:

| Crate | Role |
|-------|------|
| `shiro-core` | Domain types, config, error handling, port traits (`Embedder`, `VectorIndex`, `Reranker`) |
| `shiro-store` | SQLite persistence (schema v6), BlockGraph storage — the authoritative store |
| `shiro-index` | Tantivy BM25 full-text search, generation tracking, staging/promote |
| `shiro-parse` | Markdown, PDF, and plaintext parsers |
| `shiro-docling` | Docling subprocess adapter for structured PDF |
| `shiro-embed` | FlatIndex (cosine-similarity vector store), HttpEmbedder, embedding port definitions |
| `shiro-fastembed` | FastEmbed adapter — local ONNX embeddings and cross-encoder reranking |
| `shiro-sdk` | Operation registry, DSL interpreter, RRF fusion, hybrid search orchestration |
| `shiro-cli` | CLI entry point (16 commands), published to crates.io |

Embedding providers implement the `Embedder` trait; reranking providers implement the `Reranker` trait. No provider is architecturally privileged — switching providers requires re-embedding (vector spaces are incompatible across models).

For design decisions and ADRs, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Contributing

Contributions that uphold speed, privacy, and structural integrity are welcome.

1. Review the [Architecture](docs/ARCHITECTURE.md) for design patterns and ADRs.
2. Review the [CLI Reference](docs/CLI.md) for the output contract.
3. Ensure all changes pass the quality gate: `cargo test && cargo clippy`.
4. Open a Pull Request with a clear description of the impact.

## License

Licensed under the MIT License. See [LICENSE](LICENSE).
