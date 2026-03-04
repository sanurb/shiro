# shiro (城) Architecture

**Status:** Draft
**Date:** 2026-03-04

## Vision

shiro (城) is a **local-first document knowledge engine** for PDF and Markdown where **retrieval is fast, explainable, and structure-aware**.

The core bet is a **representation choice**: we model documents as a deterministic **Document Graph IR** (blocks + relations + reading order). Every downstream capability—segmentation, embeddings, indexing, hybrid retrieval, taxonomy assignment, and context expansion—is derived from that IR.

## Design Principles

1. **Representation-first**  
   The data model is the program. If a state is invalid (e.g., “hit without location”), it should be **unrepresentable**.

2. **Deterministic core, versioned nondeterminism**  
   Parsing/normalization/segmentation/indexing/ranking/expansion are deterministic. LLM enrichment is treated as nondeterministic unless proven otherwise and is always **versioned**.

3. **SQLite is the source of truth**  
   Metadata, taxonomy, manifests, and document state are authoritative in SQLite. Search indices are derived and rebuildable.

4. **Atomic publish of indices**  
   No partial visibility. Documents become searchable only when both BM25 and vector indices have been built and **activated**.

5. **Single binary, multiple modes**  
   One artifact (`shiro`) with modes (CLI + MCP). No mandatory daemon. Optional adapters may use subprocess or local HTTP providers, but the default path runs offline.

6. **Adapter boundaries, not framework boundaries**  
   The core does not “know” about specific parsers or model providers. Everything external is behind traits with explicit fingerprints.

7. **Explainability is not optional**  
   Any result must explain: where it came from, how it scored, and how context was expanded.

## System Architecture

### Single Binary, Multiple Modes

shiro ships as **one Rust binary** with multiple operational modes:

```bash
shiro <command>     # fast CLI (direct calls into core SDK)
shiro mcp           # MCP server over stdio (tool mode for assistants)
````

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
│  │  ingest  parse  normalize  IR  segment  embed  index  search   │  │
│  │  taxonomy  enrichment  manifests  config  explain              │  │
│  └───────────────────────────────────────────────────────────────┘  │
│             │                                   │                    │
│             ▼                                   ▼                    │
│  ┌───────────────────┐                ┌──────────────────────────┐  │
│  │      CLI Mode     │                │        MCP Mode          │  │
│  │      (clap)       │                │     (stdio transport)    │  │
│  └───────────────────┘                └──────────────────────────┘  │
│             │                                   │                    │
│             └───────────────┬───────────────────┘                    │
│                             ▼                                        │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                       Ports & Adapters                        │  │
│  │   Parser     Embedder     Enricher     VectorIndex     FTS     │  │
│  │  (baseline) (in-proc)   (in-proc)     (default)      (tantivy)│  │
│  │   + optional subprocess / HTTP providers                       │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                             │                                        │
│                             ▼                                        │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                         Local Storage                          │  │
│  │  SQLite (source of truth) + Tantivy (BM25) + Vector index      │  │
│  │  Generational indices with atomic publish                       │  │
│  └───────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Core Data Model

### Document Graph IR

PDFs do not behave like clean trees. shiro uses a **graph + a deterministic total reading order**, plus optional hierarchy edges.

**Canonical coordinate space**

* `canonical_text: String`
* All spans are byte offsets `[start, end)` into `canonical_text`.

**Blocks**

* A block is the minimal addressable unit: heading, paragraph, table, code, figure, caption, footnote, etc.

**Relations**

* `ReadsBefore`: primary reading order constraint
* `ParentOf`: best-effort hierarchy (optional)
* `CaptionOf`, `FootnoteOf`, `RefersTo`: semantic links

**Reading order**

* `reading_order: Vec<BlockId>` is the authoritative linearization used for retrieval and expansion.

### IR Invariants (enforced)

* Span bounds: `0 <= start <= end <= canonical_text.len()`
* `reading_order` is a permutation of readable blocks (or explicitly excludes metadata)
* `ReadsBefore` must not contradict `reading_order`
* `ReadsBefore + ParentOf` must be acyclic
* Overlapping spans are allowed; ordering is defined by `reading_order`

This is the foundation for:

* addressable hits (`doc_id`, `block_id`, `span`)
* deterministic segmentation
* deterministic context expansion

## Persistence, Indexing, and Consistency

### Storage Roles (authoritative vs derived)

**Authoritative (SQLite)**

* Documents + state machine
* Block/segment metadata
* Taxonomy (SKOS + closure)
* Enrichment versions (append-only)
* Run manifests (append-only)
* Active index generations pointers

**Derived**

* Tantivy BM25 index (versioned generations)
* Vector index (versioned generations; default backend is pluggable)

### Document State Machine (SQLite)

Documents are searchable only when `READY`.

* `STAGED` → `INDEXING` → `READY`
* `FAILED` (requires repair/retry)
* `DELETED` (tombstone)

### Atomic Publish (generational indices)

Indices are built in staging directories and activated by atomic rename + pointer update.

**FTS**

1. Build to `tantivy/index.<run_id>.staging/`
2. Rename to `tantivy/index.<gen>/`
3. Update `active_fts_gen = <gen>` in SQLite

**Vector**

1. Build to `vector/index.<run_id>.staging/`
2. Rename to `vector/index.<gen>/`
3. Update `active_vec_gen = <gen>` in SQLite

Search always uses `active_*_gen`. Staging never affects live results.

### Locking Model

* Single-writer lock: `~/.shiro/lock/write.lock`
* Multiple readers allowed
* Searches read active generations only

This avoids index corruption and write contention without requiring a daemon.



## Ingest Pipeline

Conceptually:

1. **Ingest**

    * Resolve source (path or URL) → `source_hash`
    * Store download in cache (bounded)

2. **Parse (adapter)**

    * Markdown: comrak AST → blocks + edges
    * PDF baseline: text extraction + deterministic structure heuristics → blocks + reading order
    * Premium parser (optional): subprocess → normalized into same IR

3. **Normalize**

    * Produce `canonical_text` deterministically
    * Recompute/validate spans
    * Create `page_map` for PDFs when available

4. **Segment**

    * Derive segments from blocks in `reading_order`
    * Type-aware splitting (tables by rows, code by blocks, paragraphs by sentence boundaries)
    * Deterministic rendering rules (fingerprinted)

5. **Embed**

    * Compute embeddings via default in-process provider
    * Store vectors + embedder metadata (including model checksums)

6. **Index**

    * Update SQLite (authoritative)
    * Build new Tantivy + vector generations in staging
    * Atomic publish → document transitions to `READY`

### Determinism and Fingerprints

Every step contributes to the document’s reproducibility identity:

* `parser_fp`, `normalizer_fp`, `render_fp`, `segmenter_fp`, `embedder_fp`, `fts_fp`, `vector_fp`

## Search and Retrieval

### Retrieval Modes

* **Vector**: cosine similarity (unit-normalized vectors)
* **BM25**: Tantivy
* **Hybrid**: deterministic fusion (default RRF)

### Hybrid Fusion (RRF, stable ordering)

* Retrieve `topk_vec` and `topk_bm25`
* Fuse by segment id:

`rrf(s) = 1/(k + rank_vec(s)) + 1/(k + rank_bm25(s))`

**Tie-break (stable)**

1. fused_score desc
2. doc_id asc
3. span.start asc
4. segment_id asc

### Context Expansion (graph-native)

For a hit on block `B`:

1. Neighbor blocks around `B` in `reading_order` within budget
2. Include caption/table/figure pairs via `CaptionOf`
3. Include footnotes via `FootnoteOf` or span anchoring
4. Include heading context blocks via deterministic rule

Hard caps (configurable):

* max blocks
* max chars
* optional max page window (PDF)

### Explain (mandatory)

Every result must provide:

* ids: `doc_id`, `segment_id`, `block_id`
* location: `span`, page range, reading order position
* scoring: vector score/rank, BM25 score/rank, fused score
* expansion trace: which rules fired, which blocks were included, budgets consumed

## SKOS Taxonomy (Production semantics)

### Data model

* `concept_id` (recommended `namespace/path`)
* `pref_label`, `alt_labels`, `scope_note`
* `broader/narrower` relations
* `closure` table for fast ancestor/descendant queries
* doc assignments: `(doc_id, concept_id, confidence, source)`

### Constraints

* Taxonomy must be a DAG (no cycles)
* On insertion of `A -> broader B`, verify `closure(B -> A)` does not exist

### Closure maintenance (deterministic, transactional)

On adding an edge, update closure by combining:

* ancestors of `B`
* descendants of `A`
  All inside one SQLite transaction.

### Concept matching (deterministic)

* Normalize labels (Unicode casefold + whitespace collapse)
* Match in order:

    1. exact id
    2. exact pref/alt label match
* If multiple matches: deterministic resolution (lexicographically smallest `concept_id`) unless manual disambiguation enabled
* Embedding-based label match is optional and must be fingerprinted if enabled

## AI Enrichment (local-first, versioned)

### Outputs

Document-level:

* title, summary (2–3 sentences), doc type
* tags (5–10)
* matched SKOS concepts (with confidence)
* proposed concepts (not auto-applied)

### Storage (append-only)

* Enrichment records are versioned by `(doc_id, enricher_fp, prompt_hash, model_id/hash, created_at)`
* Activation is explicit (`is_active` flag)
* No silent overwrites

### Influence on ranking (optional, deterministic)

If enabled:

* tags become indexed fields for BM25
* concepts apply a deterministic boost using closure depth
* explain includes boost contributions (concept ids + depths)

## Interfaces

### CLI (clap v4 derive)

* stdout: versioned JSON envelope by default (`json`, optional `ndjson`, optional `text`)
* stderr: logs only
* stable error codes (enum)

Key commands:

* init, add, ingest, search, read, explain, list, remove
* taxonomy (SKOS ops)
* config get/set
* doctor
* mcp

### MCP Server (stdio)

`shiro mcp` exposes tools:

* add/ingest/search/read/explain
* taxonomy list/tree/search/add/assign/proposed/accept/reject
* config get/set
* stats, doctor

Responses must include:

* schemaVersion
* fingerprints snapshot
* stable IDs
* stable error codes
* pagination/streaming strategy for large results (cursor + ndjson)



## Operational Layout

Default home:

```
~/.shiro/
├── config.toml
├── sqlite/shiro.db                # source of truth
├── tantivy/index.<gen>/           # derived generations
├── vector/index.<gen>/            # derived generations (default backend)
├── cache/downloads/
├── cache/models/
└── manifests/runs/
```



## Internal Module Layout (Stripe-style SDK)

High-level Rust module organization:

```rust
pub mod ingest;     // add/ingest orchestration + state transitions
pub mod parse;      // parser ports + adapters
pub mod ir;         // Document Graph IR + invariants + canonicalization
pub mod segment;    // deterministic rendering + splitting
pub mod embed;      // embedder ports + adapters + model cache
pub mod index;      // BM25 + vector build (staging + publish)
pub mod search;     // query, fusion, filters, context expansion, explain
pub mod taxonomy;   // SKOS DAG + closure maintenance + assignment
pub mod enrich;     // enrichment ports + versioning + activation
pub mod store;      // sqlite schema/migrations + repository layer
pub mod mcp;        // MCP server + tool wiring
pub mod cli;        // clap definitions + output contract
pub mod manifest;   // run manifests + fingerprints
pub mod doctor;     // integrity checks + repair/rebuild hooks
pub mod config;     // config loading + env overrides + fingerprinting
```



## Configuration

Config file: `~/.shiro/config.toml` with env overrides:

* home/log level
* parser mode (baseline/premium)
* segmentation size budgets and rules
* embed/enrich provider + model ids + cache paths
* search defaults (mode, RRF params, topk, expansion budgets)
* taxonomy matching mode

Any value that changes outputs must contribute to the appropriate fingerprint.

## Security Notes

* URL ingestion:

    * scheme allowlist (http/https)
    * size limits and timeouts
    * cached by hash under downloads cache
* Subprocess premium parser (if enabled):

    * explicit opt-in
    * bounded resources where possible
    * output treated as untrusted; validated into IR invariants
* No remote telemetry by default.

## References

- [CLI Reference](CLI.md) - Complete command documentation
- [MCP Guide](MCP.md) - Agent usage patterns