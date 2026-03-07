# shiro (城) Architecture

**Status:** Draft
**Date:** 2026-03-06

## Vision

shiro (城) is a **local-first document knowledge engine** for PDF and Markdown where **retrieval is fast, explainable, and structure-aware**.

The core bet is a **representation choice**: we model documents as a deterministic **Document Graph IR** (blocks + relations + reading order). Every downstream capability—segmentation, embeddings, indexing, hybrid retrieval, taxonomy assignment, and context expansion—is derived from that IR.

## Design Principles

1. **Representation-first**  
   The data model is the program. If a state is invalid (e.g., "hit without location"), it should be **unrepresentable**.

2. **Deterministic core, versioned nondeterminism**  
   Parsing/normalization/segmentation/indexing/ranking/expansion are deterministic. LLM enrichment is treated as nondeterministic unless proven otherwise and is always **versioned**.

3. **SQLite is the source of truth**  
   Metadata, taxonomy, manifests, and document state are authoritative in SQLite. Search indices are derived and rebuildable.

4. **Atomic publish of indices**  
   No partial visibility. Documents become searchable only when both BM25 and vector indices have been built and **activated**.

5. **Single binary, multiple modes**
   One artifact (`shiro`) with CLI mode and MCP server mode. No mandatory daemon. Optional adapters may use subprocess or local HTTP providers, but the default path runs offline.

6. **Adapter boundaries, not framework boundaries**  
   The core does not "know" about specific parsers or model providers. Everything external is behind traits with explicit fingerprints.

7. **Explainability is not optional**  
   Any result must explain: where it came from, how it scored, and how context was expanded.

## System Architecture

### Single Binary, Multiple Modes

shiro ships as **one Rust binary** with CLI and MCP server modes.

```bash
shiro <command>     # fast CLI (direct calls into core SDK)
shiro mcp           # MCP server over stdio (implemented, JSON-RPC 2.0)
```

Why not a separate daemon?

| Separate daemon                | Single binary with modes      |
|  | -- |
| Version skew risk              | Always in sync                |
| Extra packaging + service mgmt | One artifact, user-controlled |
| Always-on footprint            | Runs only when invoked        |
| Harder debugging               | Simple local execution        |

### High-Level Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                          shiro (single binary)                      │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                         Core SDK (lib)                        │  │
│  │  ingest  parse  IR  segment  index  search  config  explain    │  │
│  └───────────────────────────────────────────────────────────────┘  │
│             │                                   │                    │
│             ▼                                   ▼                    │
│  ┌───────────────────┐                ┌──────────────────────────┐  │
│  │      CLI Mode     │                │  MCP Mode (implemented,  │  │
│  │      (clap)       │                │      stdio JSON-RPC)     │  │
│  └───────────────────┘                └──────────────────────────┘  │
│             │                                   │                    │
│             └───────────────┬───────────────────┘                    │
│                             ▼                                        │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                       Ports & Adapters                        │  │
│  │   Parser         Embedder            VectorIndex        FTS    │  │
│  │  (plain/md/pdf) (StubEmbedder)      (FlatIndex)     (tantivy) │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                             │                                        │
│                             ▼                                        │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                         Local Storage                          │  │
│  │  SQLite (source of truth) + Tantivy (BM25) + Vectors (JSONL)  │  │
│  └───────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Core Data Model

### Document Graph IR

PDFs do not behave like clean trees. shiro uses a **graph + a deterministic total reading order**, plus optional hierarchy edges.

**Canonical coordinate space**

* `canonical_text: String`
* All spans are byte offsets `[start, end)` into `canonical_text`.

**Blocks** (`BlockKind` enum in `shiro-core::ir`)

* `Paragraph`, `Heading`, `ListItem`, `TableCell`, `Code`, `Caption`, `Footnote`

**Relations** (`Relation` enum in `shiro-core::ir`)

* `ReadsBefore`: primary reading order constraint
* `CaptionOf`, `FootnoteOf`, `RefersTo`: semantic links

**Reading order**

* `reading_order: Vec<BlockId>` is the authoritative linearization used for retrieval and expansion.

### IR Invariants (enforced)

* Span bounds: `0 <= start <= end <= canonical_text.len()`
* `reading_order` is a permutation of readable blocks (or explicitly excludes metadata)
* `ReadsBefore` must not contradict `reading_order`
* `ReadsBefore` edges must be acyclic (validated via iterative 3-color DFS)
* Overlapping spans are allowed; ordering is defined by `reading_order`

This is the foundation for:

* addressable hits (`doc_id`, `block_id`, `span`)
* deterministic segmentation
* deterministic context expansion

## Persistence, Indexing, and Consistency

### Storage Roles (authoritative vs derived)

**Authoritative (SQLite)** — tables: `documents` (with `fingerprint` column), `segments`, `search_results` (with `fts_gen`, `vec_gen`, `query_digest`), `blobs`, `schema_meta`, `concepts`, `concept_relations`, `concept_closure`, `doc_concepts`, `enrichments`, `generations`, `active_generations`

* Documents + state machine (`DocState`) + processing fingerprint
* Segment metadata and body text
* Cached search results (for `explain`)
* Content-addressed blob store
* Taxonomy concepts and relations (SKOS)
* Enrichment records (heuristic + LLM)
* Generation tracking for index management

**Derived**

* Tantivy BM25 index (staging directory for atomic rebuild)
* FlatIndex vector store (in-memory HashMap + JSONL persistence)

### Schema v3 Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                        SQLite (shiro.db)                         │
│                                                                  │
│  schema_meta          documents              segments            │
│  ┌──────────┐         ┌────────────────┐     ┌──────────────┐   │
│  │ key      │         │ id (DocId)     │◄────│ doc_id       │   │
│  │ value    │         │ path           │     │ segment_id   │   │
│  └──────────┘         │ state          │     │ block_id     │   │
│                       │ fingerprint    │     │ body         │   │
│                       │ content_hash   │     │ span_start   │   │
│                       │ created_at     │     │ span_end     │   │
│                       └────────────────┘     └──────────────┘   │
│                              │                                   │
│  search_results              │          blobs                    │
│  ┌────────────────────┐      │          ┌──────────────┐        │
│  │ result_id          │      │          │ hash         │        │
│  │ doc_id        ─────┼──────┘          │ data         │        │
│  │ segment_id         │                 └──────────────┘        │
│  │ query_digest       │                                          │
│  │ fts_gen            │   concepts          concept_relations    │
│  │ vec_gen            │   ┌────────────┐    ┌────────────────┐  │
│  │ bm25_score         │   │ concept_id │◄───│ subject_id     │  │
│  │ bm25_rank          │   │ label      │    │ predicate      │  │
│  │ fused_score        │   └────────────┘    │ object_id      │  │
│  └────────────────────┘                     └────────────────┘  │
│                                                                 │
│  concept_closure       doc_concepts         enrichments         │
│  ┌────────────────┐    ┌──────────────┐     ┌──────────────────┐│
│  │ ancestor_id    │    │ doc_id       │     │ doc_id           ││
│  │ descendant_id  │    │ concept_id   │     │ title            ││
│  │ depth          │    └──────────────┘     │ summary          ││
│  └────────────────┘                         │ tags             ││
│                                             │ concepts         ││
│  generations           active_generations   │ provider         ││
│  ┌──────────────┐      ┌────────────────┐   │ content_hash    ││
│  │ gen_id       │      │ kind           │   │ created_at      ││
│  │ kind         │      │ gen_id         │   └──────────────────┘ │
│  │ created_at   │      └────────────────┘                        │
│  └──────────────┘                                                │
└──────────────────────────────────────────────────────────────────┘
```

### Document State Machine (SQLite)

Documents are searchable only when `READY`.

* `STAGED` → `INDEXING` → `READY`
* `FAILED` (requires repair/retry)
* `DELETED` (tombstone)

### Atomic Publish (FTS staging)

FTS indices can be rebuilt from scratch via staging:

1. Build to `tantivy_staging/` directory
2. Atomic `rename()` to promote staging → live `tantivy/` directory

The live `tantivy/` index also supports additive writes (individual `index_segments` + commit).

### Locking Model

* Single-writer lock: `~/.shiro/lock/write.lock`
* Multiple readers allowed
* Searches read active generations only

This avoids index corruption and write contention without requiring a daemon.

## Vector Search

### FlatIndex (correctness baseline)

`FlatIndex` is a brute-force cosine-similarity implementation of the `VectorIndex` trait. It exists as a **correctness baseline** — ground truth for recall comparisons when evaluating future ANN backends (see ADR-003).

**Implementation details:**

* In-memory `HashMap<SegmentId, Vec<f32>>` for fast lookup
* JSONL persistence (`vectors.jsonl`) for durability across restarts
* Brute-force cosine similarity over all vectors on every search
* No approximate data structures, no quantization — exact results by design

**`VectorIndex` trait** (defined in `shiro-core`, implemented in `shiro-embed`):

* `upsert(id, vector)` — insert or replace a vector
* `delete(id)` — remove a single vector
* `delete_by_doc(doc_id)` — remove all vectors for a document
* `search(query_vector, limit)` — return top-k by cosine similarity
* `count()` — number of stored vectors
* `dimensions()` — vector dimensionality
* `flush()` — persist in-memory state to JSONL

**`StubEmbedder`** (test-only): returns zero vectors of configurable dimension. Used for integration tests and `doctor --verify-vector`. Not exposed via any CLI search mode.

## Ingest Pipeline

Current implementation:

1. **Parse (adapter via `Parser` trait)**

    * Plain text: UTF-8, paragraph-boundary segmentation, no `BlockGraph`
    * Markdown (`pulldown-cmark`): YAML frontmatter stripped, `BlockGraph` with heading/paragraph/code/list-item blocks, `rendered_text` preserved
    * PDF (`pdf-extract`): text extraction, loss detection for scan-heavy documents, no `BlockGraph`
    * Content-addressed: `DocId = blake3(raw_content_bytes)`; duplicate content short-circuits

2. **Store** — `put_document(STAGED)` into SQLite

3. **Segment** (`segment_document`)

    * If `BlockGraph` present: one segment per non-empty block in `reading_order`
    * If no graph: split `canonical_text` on `"\n\n"` (double newline), one segment per paragraph

4. **Index** — `set_state(INDEXING)` → `put_segments` → `fts.index_segments` → `set_state(READY)`

5. **Embed** (infrastructure exists but not exposed via CLI) — generate vector embeddings for each segment, upsert into FlatIndex, flush to JSONL

For `ingest` (batch): phases are batched — all docs parsed + stored in one SQLite transaction, all segments indexed in one Tantivy commit, all states updated in one transaction.

## Search and Retrieval

### Retrieval Modes

* **BM25**: Tantivy full-text search (implemented)
* **BM25**: Tantivy full-text search (implemented, default mode)
* **Hybrid**: RRF fusion of BM25 + vector search. Currently degrades to BM25-only since vector search is not exposed via CLI.
* **Vector**: not exposed via CLI (infrastructure exists in shiro-embed but StubEmbedder produces zero vectors)
### Hybrid Fusion (RRF, stable ordering)

`rrf(s) = Σ 1/(k + rank_S(s))` where `k = 60` and `S` iterates over retrieval sources (BM25, vector).

**Tie-break (stable)**

1. fused_score desc
2. id asc (lexicographic)

Both `bm25_rank` and `vector_rank` contribute to fusion when a FlatIndex is available. When no vector index is present, hybrid degrades gracefully to BM25-only.

### Context Expansion

Context expansion is triggered via `--expand` on search results. The algorithm expands outward from the hit segment, alternating before and after in reading order.

**Algorithm:**

1. Start from the hit segment's position in the document's segment list
2. Alternate: pick the next segment **before**, then the next segment **after**
3. Stop when either budget is exhausted:
   * `--max-blocks` (default: **12**) — maximum number of segments to include
   * `--max-chars` (default: **8000**) — maximum total character count

The segments table serves as a proxy for blocks since BlockGraph is not persisted to SQLite. Each segment corresponds to one block in reading order.

### Explain

`shiro explain <result_id>` loads the cached `search_results` row and returns:

* ids: `result_id`, `doc_id`, `segment_id`, `block_id`
* location: `span`
* scoring: `bm25.score`, `bm25.rank`, `fused` score
* `retrieval_trace`: pipeline stages, RRF fusion contributions
* `expansion`: rules fired, included block IDs, budget usage

## SKOS Taxonomy

shiro implements a SKOS-based taxonomy system for organizing documents into concept hierarchies.

### Data Model

* **ConceptId**: content-addressed identifier for each concept
* **SkosRelation**: `Broader`, `Narrower`, `Related`

### SQLite Tables

* `concepts` — concept definitions (concept_id, label)
* `concept_relations` — directed relations between concepts (subject_id, predicate, object_id)
* `concept_closure` — transitive closure table (ancestor_id, descendant_id, depth) for efficient hierarchy queries
* `doc_concepts` — maps documents to assigned concepts (doc_id, concept_id)

### Subcommands

* `shiro taxonomy add <label>` — create a new concept
* `shiro taxonomy list` — list all concepts
* `shiro taxonomy relations <concept_id>` — show relations for a concept
* `shiro taxonomy assign <doc_id> <concept_id>` — assign a concept to a document
* `shiro taxonomy import <file>` — bulk import concepts/relations

The closure table is maintained automatically: when a `Broader`/`Narrower` relation is added, transitive paths are computed and inserted. Cycle detection prevents invalid `Broader`/`Narrower` chains (`E_TAXONOMY_CYCLE`).

## AI Enrichment

### Enrichment Pipeline

Enrichment generates structured metadata for documents. Two provider paths exist:

**Heuristic provider** (implemented):

* `title` — first non-empty line of the document
* `summary` — first 500 characters of content
* `tags` — extracted from Markdown headings

**LLM provider** (not exposed):

* Returns `E_INVALID_INPUT` — reserved for future LLM-based enrichment. Not selectable from CLI.

### EnrichmentResult

Each enrichment record contains:

* `doc_id` — source document
* `title`, `summary`, `tags` — extracted metadata
* `concepts` — suggested taxonomy concepts
* `provider` — which provider generated this record (heuristic/llm)
* `content_hash` — hash of the content at enrichment time (for staleness detection)
* `created_at` — timestamp

Records are stored in the `enrichments` table and keyed by `(doc_id, provider)`.

## Generation-based Index Management

Index updates use a generation-based publish model to ensure atomic visibility and safe concurrent reads.

### Core Types

* `GenerationId(u64)` — monotonically increasing generation identifier
* `IndexGeneration` — associates a generation with an index kind (FTS or vector)

### FTS Generation Lifecycle

1. **Build**: `build_from_segments(staging_dir, segments, gen_id)` writes a new Tantivy index into a staging directory tagged with the generation id
2. **Promote**: `promote_staging` performs an atomic three-step swap:
   * `live/` → `backup/`
   * `staging/` → `live/`
   * delete `backup/`
3. **Register**: the new generation id is recorded in SQLite `active_generations`

`gen_dir(base, gen_id)` computes the filesystem path for a given generation.

### Active Generations Table

The `active_generations` table in SQLite tracks which generation is currently live for each index kind. Readers consult this table to discover the active index. Writers atomically update it after a successful promote.

Vector reindex: not implemented (removed from CLI surface).

## Processing Fingerprints

Processing fingerprints track **how** a document was processed, separately from **what** the content is.

### ProcessingFingerprint

```rust
ProcessingFingerprint {
    parser_name: String,       // e.g., "markdown", "pdf", "plaintext"
    parser_version: String,    // parser implementation version
    segmenter_version: String, // segmentation algorithm version
}
```

* `content_hash()` method computes the fingerprint's own hash for change detection
* Stored in the `documents.fingerprint` column (serialized)
* Separate from `DocId` — DocId is content-only (`blake3(raw_bytes)`), while the fingerprint tracks processing version (see ADR-004)

When a parser or segmenter version changes, affected documents can be identified by fingerprint mismatch without invalidating their DocId or any references pointing to them.

## Interfaces

### CLI (clap v4 derive)

* stdout: JSON envelope (`{ ok, command, result, next_actions }`)
* stderr: logs only (tracing with `EnvFilter`)
* stable error codes (enum)

Implemented commands (all 16): `init`, `add`, `ingest`, `search`, `read`, `explain`, `list`, `remove`, `doctor`, `config`, `capabilities`, `taxonomy`, `reindex`, `mcp`, `completions`, `enrich`

See [CLI Reference](CLI.md) for complete documentation.

### MCP Server (stdio) — Code Mode

`shiro mcp` starts a JSON-RPC 2.0 server over stdio. Protocol version `2024-11-05`.

**Transport:** reads newline-delimited JSON from stdin, writes JSON + newline to stdout.

**Supported methods:**

* `initialize` — capability negotiation
* `notifications/initialized` — acknowledged (no response)
* `tools/list` — returns exactly two tools: `shiro.search` and `shiro.execute`
* `tools/call` — dispatches to the requested tool with strict validation

**Code Mode tools:**

| Tool | Input | Output |
|------|-------|--------|
| `shiro.search` | `{query, limit?}` | Ranked `SpecSearchResult[]` with op specs, schemas, examples |
| `shiro.execute` | `{program, limits?}` | `ExecutionResult {value, steps_executed, total_duration_us, trace[]}` |

**DSL node types:** `let`, `call`, `if`, `for_each`, `return`.

**Safety guarantees:**
- No arbitrary code execution — JSON AST interpreter only
- `deny_unknown_fields` on all DSL nodes
- Hard limits: max_steps (200), max_iterations (100), max_output_bytes (1 MiB), timeout (30s)
- Structured execution trace with per-step timing, args hash, error codes
- Deterministic search results (scored, name-sorted tie-break)

### Completions

`shiro completions <shell>` emits raw shell completion scripts for **bash**, **zsh**, **fish**, and **powershell** via `clap_complete`. Output bypasses the JSON envelope (raw shell output).

## Operational Layout

Default home (`~/.shiro/`, overridable via `--home` or `SHIRO_HOME`):

```
~/.shiro/
├── config.toml                    # config (dotted-key TOML, get/set supported)
├── shiro.db                       # SQLite source of truth (WAL mode)
├── tantivy/                       # live Tantivy BM25 index
├── tantivy_staging/               # staging dir for atomic FTS rebuild
├── vectors/                       # live FlatIndex vector store (vectors.jsonl)
├── vectors_staging/               # staging dir for vector rebuild
└── lock/write.lock                # single-writer PID lock
```

## Workspace Crate Layout

```
crates/
├── shiro-core/     # Domain types: IR, config, errors, ids, manifest, ports, lock
│                   #   Traits: Parser, Embedder, VectorIndex
│                   #   No dependencies on other workspace crates
├── shiro-parse/    # Parser implementations: PlainTextParser, MarkdownParser, PdfParser
│                   #   + segment_document() segmenter
│                   #   Depends on: shiro-core
├── shiro-store/    # SQLite persistence: Schema v3, WAL mode, migration support
│                   #   Tables: documents, segments, search_results, blobs,
│                   #   concepts, concept_relations, concept_closure, doc_concepts,
│                   #   enrichments, generations, active_generations, schema_meta
│                   #   Depends on: shiro-core
├── shiro-index/    # Tantivy FTS: index_segments, search, delete, staging rebuild
│                   #   Custom tokenizer: SimpleTokenizer → RemoveLongFilter(40) → LowerCaser
│                   #   Depends on: shiro-core
├── shiro-embed/    # Vector search: FlatIndex (VectorIndex impl), StubEmbedder (Embedder impl)
│                   #   Brute-force cosine similarity, JSONL persistence
│                   #   Depends on: shiro-core
└── shiro-cli/      # Binary (shiro): clap commands, JSON envelope, RRF fusion, HATEOAS
                    #   MCP server (stdio JSON-RPC), completions
                    #   Depends on: all workspace crates
```

## Configuration

Resolution precedence for `ShiroHome`:

1. `--home <path>` CLI argument
2. `SHIRO_HOME` environment variable
3. `~/.shiro` (platform default)

`config get <key>` / `config set <key> <value>` — fully implemented with dotted-key TOML support (e.g., `shiro config get search.default_limit`). `config show` returns resolved paths and current configuration.

### Capabilities

`shiro capabilities` returns a machine-readable summary:

* `schemaVersion`: **2** (bumped after removing unimplemented feature claims)
* `runtime.schema_version`: **3** (current store schema)
* Features:
  * `fts_bm25` — implemented
  * `hybrid_search` — bm25_only (falls back to BM25 when no vector index)
  * `taxonomy` — implemented
  * `enrichment` — heuristic_only
  * `mcp_server` — code_mode (search + execute tools, JSON AST DSL)
  * `completions` — implemented
## Security Notes

* URL ingestion:

    * scheme allowlist (http/https)
    * size limits and timeouts
    * cached by hash under downloads cache
* No remote telemetry by default.

## Architecture Decision Records

### ADR-001: Document Graph IR over tree model

PDFs produce overlapping spans, footnotes, and cross-references that break tree assumptions. A directed graph with explicit reading order handles these naturally. The IR is validated on construction; invalid states are unrepresentable.

### ADR-002: SQLite as source of truth

All authoritative state lives in SQLite. Search indices (Tantivy, FlatIndex) are derived and can be rebuilt from SQLite data. This simplifies backup, migration, and recovery.

### ADR-003: FlatIndex as correctness baseline

**Context:** Vector search needs an implementation for development and testing. Approximate Nearest Neighbor (ANN) libraries (HNSW, IVF) offer better performance at scale but introduce recall variance.

**Decision:** FlatIndex (brute-force cosine similarity) is the ground truth implementation. Any future ANN backend is an optimization that **must produce recall ≥ 0.95 vs FlatIndex** on the project's benchmark suite before replacing it as default.

**Consequences:** Search is O(n) in vector count. Acceptable for the target scale (thousands of documents). When scaling requires it, an ANN adapter can be added behind the `VectorIndex` trait without changing any consumer code.

### ADR-004: Processing fingerprints separate from DocId

**Context:** `DocId` is `blake3(raw_content_bytes)` — purely content-addressed. When a parser or segmenter is upgraded, reprocessing is needed even though content hasn't changed. Using DocId for this would invalidate all references (search results, taxonomy assignments, enrichments).

**Decision:** Processing version is tracked in a separate `fingerprint` column on the `documents` table via `ProcessingFingerprint { parser_name, parser_version, segmenter_version }`. DocId remains content-only.

**Consequences:** Parser upgrades can be detected by fingerprint mismatch without breaking referential integrity. Reprocessing is targeted: only documents whose fingerprint doesn't match the current parser version need re-ingestion.

### ADR-005: Generation-based index publish

**Context:** Readers must never see a partially-built index. Additive writes (single doc → commit) are fine for incremental adds, but full reindexes need atomic visibility.

**Decision:** Each full reindex builds into a staging directory tagged with a `GenerationId(u64)`. Promotion is a three-step atomic swap (live→backup, staging→live, delete backup). The `active_generations` table in SQLite records which generation is live per index kind.

**Consequences:** Concurrent readers see a consistent snapshot. Failed builds leave staging in place without corrupting live. The generation table enables future features like rollback and multi-version reads.

## References

- [CLI Reference](CLI.md) - Complete command documentation
- [MCP Guide](MCP.md) - Agent usage patterns
