```markdown
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

Unlike traditional search tools that treat documents as flat strings, `shiro` parses content into a **Document Graph IR**, preserving reading order and block relationships. It exposes this data through a deterministic, JSON-native CLI and an **MCP server**, making your private library instantly accessible to AI agents like Claude and Cursor.

## ✨ Key Differentiators

* **Structure-Aware IR:** Documents are modeled as hierarchical blocks with byte-level spans, allowing for precise context windowing in LLM applications.
* **Deterministic JSON CLI:** Built for the Unix philosophy. Every command outputs a structured JSON envelope, perfect for `jq` piping or automated agent consumption.
* **HATEOAS Navigation:** Responses include `next_actions` with typed parameter templates, enabling AI agents to discover commands dynamically.
* **Zero-API Dependency:** Everything—from parsing to BM25 indexing—runs on your hardware. No data leaves your machine.
* **Native MCP Support:** First-class implementation of the [Model Context Protocol](https://modelcontextprotocol.io) for seamless integration with modern AI IDEs and assistants.


## 🚀 Getting Started

### 1. Installation
Install the pre-compiled binary for your architecture:

```bash
# macOS / Linux
curl -sSL [https://get.shiro.dev](https://get.shiro.dev) | sh

```

*Or via Cargo:* `cargo install shiro`

### 2. Initialize your Fortress

```bash
shiro init
shiro ingest ~/Documents/KnowledgeBase

```

### 3. Query with JSON Output

```bash
# Search for specific concepts
shiro search "distributed consensus" | jq '.result.hits[0].snippet'

```


## 🤖 AI Integration (MCP)

`shiro` bridges the gap between your local documents and AI assistants. Add the following to your `claude_desktop_config.json` or Cursor settings:

```json
"mcpServers": {
  "shiro": {
    "command": "shiro",
    "args": ["mcp"]
  }
}

```

This grants your AI assistant the ability to search, read, and summarize your local research papers and notes with full citations.


## 🛠 Project Status & Roadmap

We prioritize transparency. Here is the current implementation status of the engine:

| Feature | Status | Technology |
| --- | --- | --- |
| **Markdown/Text Indexing** | ✅ Stable | Paragraph-boundary segmentation |
| **BM25 Full-Text Search** | ✅ Stable | [Tantivy](https://github.com/quickwit-oss/tantivy) engine |
| **JSON/HATEOAS Layer** | ✅ Stable | Structured CLI output |
| **MCP Server** | 🛠 Beta | Stdio-based protocol implementation |
| **PDF Parsing** | 🏗 In Progress | Structural IR extraction |
| **Vector Search** | 🗺 Roadmap | Local embeddings via Ollama/Llama.cpp |


## 🏗 Architecture

`shiro` is built in Rust for memory safety and uncompromising performance.

* **Source of Truth:** [SQLite](https://sqlite.org) via `rusqlite` for metadata and state management.
* **Search Core:** [Tantivy](https://github.com/quickwit-oss/tantivy) for world-class indexing speed.
* **Deduplication:** Content-addressed IDs using [BLAKE3](https://github.com/BLAKE3-team/BLAKE3) hashes.

## 🤝 Contributing

We welcome contributions that adhere to our core principles of speed, privacy, and structural integrity.

1. Review the [ARCHITECTURE.md](/docs/ARCHITECTURE.md) for design patterns.
2. Ensure all changes pass the quality gate: `cargo test-all && cargo lint`.
3. Open a Pull Request with a clear description of the impact.

**Built with 🦀 by [sanurb**](https://github.com/sanurb) Distributed under the MIT and Apache-2.0 Licenses.