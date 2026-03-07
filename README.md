<h1 align="center">🏯 shiro (城)</h1>

<p align="center">
  <strong>The local-first knowledge engine for structured document retrieval.</strong>
</p>

<p align="center">
  <a href="https://github.com/sanurb/shiro/actions"><img src="https://img.shields.io/github/actions/workflow/status/sanurb/shiro/ci.yml?branch=master&style=flat-square&logo=github&color=181717" alt="CI Status"></a>
  <a href="https://github.com/sanurb/shiro/releases"><img src="https://img.shields.io/github/v/release/sanurb/shiro?style=flat-square&logo=rust&color=e44d26" alt="Latest Release"></a>
  <img src="https://img.shields.io/badge/Architecture-Local--First-blue?style=flat-square" alt="Local-First">
  <img src="https://img.shields.io/badge/License-MIT%2FApache--2.0-4caf50?style=flat-square" alt="License">
</p>

> [!TIP]
> **shiro** (Japanese for *castle*) is a high-performance, local-first knowledge engine designed to transform fragmented PDFs and Markdown files into a unified, structure-aware searchable base. 

Unlike traditional search tools that treat documents as flat strings, `shiro` parses content into a **Document Graph IR**, preserving reading order and block relationships. It exposes this data through a deterministic, JSON-native CLI, making your private library instantly accessible to AI agents like Claude and Cursor.

## ✨ Key Differentiators

* **Structure-Aware IR:** Documents are modeled as hierarchical blocks with byte-level spans, allowing for precise context windowing in LLM applications.
* **Deterministic JSON CLI:** Built for the Unix philosophy. Every command outputs a structured JSON envelope, perfect for `jq` piping or automated agent consumption.
* **HATEOAS Navigation:** Responses include `next_actions` with typed parameter templates, enabling AI agents to discover commands dynamically.
* **Zero-API Dependency:** Everything—from parsing to BM25 indexing—runs on your hardware. No data leaves your machine.
* **Pluggable Parsers:** Built-in support for Markdown (pulldown-cmark with block graph IR), PDF (pdf-extract), and plain text, with trait-based extensibility for custom formats.

## 🚀 Getting Started

### 1. Installation

You can install `shiro` either from a prebuilt release or via Cargo.

#### Option A: Install via shell script (prebuilt binaries)

```bash
# macOS / Linux
curl -sSfL https://raw.githubusercontent.com/sanurb/shiro/master/install.sh | sh
```

The script:

- **Detects your OS and CPU architecture.**
- **Downloads the latest `shiro` GitHub release for your platform.**
- **Installs the `shiro` binary into `~/.local/bin` by default.**

To change the installation directory, set `SHIRO_INSTALL_DIR` before running the script, for example:

```bash
SHIRO_INSTALL_DIR=/usr/local/bin \
  curl -sSfL https://raw.githubusercontent.com/sanurb/shiro/master/install.sh | sh
```

#### Option B: Install via Cargo

If you prefer building from source or are on an unsupported platform:

```bash
cargo install shiro-cli
```

This installs the `shiro` binary (the **crate name** is `shiro-cli`, but the **executable name** you run is still `shiro`).

### 2. Initialize your Fortress

```bash
shiro init
shiro ingest ~/Documents/KnowledgeBase

```

### 3. Query with JSON Output

```bash
# Search for specific concepts
shiro search "distributed consensus" | jq '.result.results[0].snippet'

```

## 🤖 AI Integration

`shiro`'s deterministic JSON CLI with HATEOAS navigation makes it ideal for AI agent consumption. Agents can discover available commands dynamically via `shiro` (root self-doc) or `shiro capabilities`, then parse structured responses with `jq` or directly.

> See [Code Mode (MCP)](#code-mode-mcp) below for the two-tool stdio server.

## Code Mode (MCP)

Shiro exposes a [Model Context Protocol](https://modelcontextprotocol.io) server over stdio with exactly two tools:

| Tool | Purpose |
|------|---------|
| `shiro.search` | Discover SDK operations, schemas, and examples |
| `shiro.execute` | Run a DSL program against the knowledge base |

### Quick start

```bash
# Start the MCP server
shiro mcp
```

### shiro.search example

```json
{"query": "search", "limit": 5}
```

Returns ranked results with operation specs, parameter schemas, and usage examples.

### shiro.execute example

Multi-step program that searches, reads the top hit, then returns a summary:

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
- **Typed SDK** -- all operations backed by schemars-derived JSON Schemas
- **Deterministic outputs** -- search results are scored and name-sorted
- **Strict validation** -- unknown fields rejected, all inputs schema-checked
- **Stable error codes** -- every error maps to an `E_*` code
- **Safe execution** -- no arbitrary code, hard limits on steps/iterations/bytes/time

## 🛠 Project Status & Roadmap

We prioritize transparency. Here is the current implementation status of the engine:

| Feature | Status | Technology |
| --- | --- | --- |
| **Markdown Parsing** | ✅ Stable | [pulldown-cmark](https://github.com/raphlinus/pulldown-cmark) with Block Graph IR |
| **PDF Parsing** | ✅ Stable | [pdf-extract](https://crates.io/crates/pdf-extract) with loss detection |
| **Plain Text Indexing** | ✅ Stable | Paragraph-boundary segmentation |
| **BM25 Full-Text Search** | ✅ Stable | [Tantivy](https://github.com/quickwit-oss/tantivy) engine |
| **JSON/HATEOAS Layer** | ✅ Stable | Structured CLI output with `next_actions` |
| **MCP Server** | `v0.3.0` | Stdio JSON-RPC 2.0 with DSL interpreter (see [docs/MCP.md](docs/MCP.md)) |
| **Vector Search** | Planned | Traits defined; no embedder implementation yet |
| **SKOS Taxonomy** | `v0.2.0` | SKOS-style concepts with add/list/relations/assign/import |
| **AI Enrichment** | `v0.2.0` | Heuristic enrichment via `--enrich` flag |

## 🏗 Architecture

`shiro` is built in Rust for memory safety and uncompromising performance.

* **Source of Truth:** [SQLite](https://sqlite.org) via `rusqlite` for metadata and state management.
* **Search Core:** [Tantivy](https://github.com/quickwit-oss/tantivy) for world-class indexing speed.
* **Deduplication:** Content-addressed IDs using [BLAKE3](https://github.com/BLAKE3-team/BLAKE3) hashes.

## 🤝 Contributing

We welcome contributions that adhere to our core principles of speed, privacy, and structural integrity.

1. Review the [ARCHITECTURE.md](docs/ARCHITECTURE.md) for design patterns.
2. Review the [CLI Reference](docs/CLI.md) for the output contract.
3. Ensure all changes pass the quality gate: `cargo test && cargo clippy`.
4. Open a Pull Request with a clear description of the impact.