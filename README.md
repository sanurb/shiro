<h1 align="center">🏯 Shiro</h1>

<p align="center">
  <strong>The local-first knowledge engine for your personal fortress of documents.</strong>
</p>

<p align="center">
  <a href="https://github.com/sanurb/shiro/actions"><img src="https://img.shields.io/github/actions/workflow/status/sanurb/shiro/ci.yml?branch=master&style=for-the-badge&logo=github" alt="CI"></a>
  <a href="https://github.com/sanurb/shiro/releases"><img src="https://img.shields.io/github/v/release/sanurb/shiro?style=for-the-badge&logo=rust" alt="Release"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue?style=for-the-badge" alt="License"></a>
</p>


Works with PDFs AND Markdown files — Index your research papers, books, notes, docs, and any `.md` files in one unified, searchable knowledge base. One binary, zero API costs, total privacy.

> [!TIP]
> **shiro (城)** is a Japanese castle—fortified, organized, and built to withstand sieges. It is a fortress for your documents, where retrieval is fast and structure matters.

## ✨ Features
* **PDF + Markdown** — Index `.pdf` and `.md` files with the same workflow.
* **Local-first** — Everything runs on your machine; your data never leaves your sight.
* **AI Enrichment** — Automatic extraction of titles, summaries, tags, and concepts.
* **SKOS Taxonomy** — Organize documents with professional-grade hierarchical concepts.
* **Vector Search** — Local semantic search powered by [Ollama](https://ollama.com/) embeddings.
* **Hybrid Search** — The best of both worlds: BM25 keyword matching combined with vector similarity.
* **MCP Server** — Seamlessly connect your knowledge to Claude, Cursor, and other AI assistants via the [Model Context Protocol](https://modelcontextprotocol.io).


## 🚀 Quick Start



### 1. Installation
Download the binary for your system from [Releases](https://github.com/sanurb/shiro/releases) or install via Cargo:
```bash
cargo install shiro

```

### 2. Initialize and Index

```bash
# Setup the fortress
shiro init

# Add your library
shiro ingest ~/Documents/Research/

```

### 3. Search Your Knowledge

```bash
# Simple hybrid search
shiro search "quantum computing optimization"

```


## 🔌 Using with AI (MCP)

`shiro` acts as a bridge between your local files and AI agents. To use `shiro` with **Claude Desktop**, add this to your configuration:

```json
"mcpServers": {
  "shiro": {
    "command": "shiro",
    "args": ["mcp"]
  }
}

```

Now your AI assistant can "read" your local papers and notes to answer complex questions with citations.


## 🛠️ Usage & Commands

| Command | Description |
| --- | --- |
| `shiro add <file>` | Parse and index a single document. |
| `shiro search <query>` | Run a hybrid search (Keywords + Vectors). |
| `shiro read <id>` | Preview document content or outline in the terminal. |
| `shiro doctor` | Check the health of your local index and database. |
| `shiro mcp` | Launch the MCP server for AI tool integration. |


## 🤝 Contributing

Contributions are what make the open-source community an amazing place to learn, inspire, and create.

1. **Fork** the Project.
2. **Create** your Feature Branch (`git checkout -b feature/AmazingFeature`).
3. **Commit** your Changes (`git commit -m 'Add some AmazingFeature'`).
4. **Push** to the Branch (`git push origin feature/AmazingFeature`).
5. **Open** a Pull Request.


## 📜 License

Distributed under the **MIT** or **Apache-2.0** License. See `LICENSE` for more information.

**Built with 🦀 by [sanurb**](https://github.com/sanurb)