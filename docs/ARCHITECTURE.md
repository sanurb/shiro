# shiro (城) Architecture

**Status:** Living document
**Date:** 2026-03-07

## Vision

shiro is a **local-first document knowledge engine** for PDF and Markdown. Retrieval is fast, explainable, and structure-aware.

The core bet: documents are modeled as a deterministic **Document Graph IR** (blocks + relations + reading order). Every downstream capability — segmentation, embeddings, indexing, hybrid retrieval, taxonomy, context expansion — is derived from that IR.

## Design Principles

1. **Representation-first** — the data model is the program. Invalid states are unrepresentable.
2. **Deterministic core, versioned nondeterminism** — parsing, segmentation, indexing, ranking, expansion are deterministic. LLM enrichment is nondeterministic and always versioned.
3. **SQLite is the source of truth** — metadata, taxonomy, manifests, document state are authoritative in SQLite. Search indices are derived and rebuildable.
4. **Atomic publish** — documents become searchable only when indices have been built and activated. No partial visibility.
5. **Single binary, multiple modes** — one artifact with CLI and MCP server modes. No mandatory daemon.
6. **Adapter boundaries, not framework boundaries** — the core does not know about specific parsers or model providers. Everything external is behind traits with explicit fingerprints.
7. **Explainability is not optional** — any result must explain where it came from, how it scored, and how context was expanded.

## System Shape

```
┌─────────────────────────────────────────────────────────────────────┐
│                       shiro (single binary)                         │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                      Core SDK (shiro-sdk)                      │  │
│  │  ingest  parse  IR  segment  index  search  config  explain    │  │
│  └───────────────────────────────────────────────────────────────┘  │
│             │                                   │                    │
│             ▼                                   ▼                    │
│  ┌───────────────────┐                ┌──────────────────────────┐  │
│  │   CLI (shiro-cli)  │                │   MCP (stdio JSON-RPC)   │  │
│  └───────────────────┘                └──────────────────────────┘  │
│             │                                   │                    │
│             └───────────────┬───────────────────┘                    │
│                             ▼                                        │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                     Ports & Adapters                            │  │
│  │  Parser (shiro-parse)    Embedder (shiro-embed)                │  │
│  │  VectorIndex (shiro-embed)    FTS (shiro-index/Tantivy)        │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                             │                                        │
│                             ▼                                        │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                       Local Storage                              │  │
│  │  SQLite (shiro-store)  ·  Tantivy (shiro-index)  ·  Vectors   │  │
│  └───────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Crate Map

```
crates/
├── shiro-core/     # Domain types: IR, config, errors, ids, ports, lock, span, taxonomy
│                   #   Traits: Parser, Embedder, VectorIndex
│                   #   No dependencies on other workspace crates
├── shiro-parse/    # MarkdownParser, PdfParser + segment_document()
│                   #   Depends on: shiro-core
├── shiro-store/    # SQLite persistence: schema v4, WAL mode
│                   #   Depends on: shiro-core
├── shiro-index/    # Tantivy BM25 FTS: index, search, staging rebuild
│                   #   Depends on: shiro-core
├── shiro-embed/    # FlatIndex (VectorIndex), HttpEmbedder, StubEmbedder
│                   #   Depends on: shiro-core
├── shiro-sdk/      # Typed API surface, executor, DSL, RRF fusion, spec registry
│                   #   Depends on: all workspace crates
└── shiro-cli/      # Binary: clap commands, JSON envelope, MCP server
                    #   Depends on: all workspace crates
```

## Core Data Model

### Document Graph IR

Documents are modeled as a directed graph with a deterministic total reading order. Defined in `shiro-core::ir`.

- **Coordinate space**: `canonical_text: String` with all spans as byte offsets `[start, end)`.
- **Blocks** (`BlockKind`): `Paragraph`, `Heading`, `ListItem`, `TableCell`, `Code`, `Caption`, `Footnote`.
- **Relations** (`Relation`): `ReadsBefore`, `CaptionOf`, `FootnoteOf`, `RefersTo`.
- **Reading order**: `reading_order: Vec<BlockIdx>` — authoritative linearization for retrieval and expansion.

### IR Invariants

- `Span`: half-open `[start, end)`, enforced at construction (`Span::new() -> Result`)
- `reading_order` is a permutation of readable blocks
- `ReadsBefore` must not contradict `reading_order`
- `ReadsBefore` edges are acyclic (validated via iterative 3-color DFS in `BlockGraph::validate()`)
- 6 `IrViolation` types checked: `SpanOutOfBounds`, `InvalidReadingOrderIndex`, `ReadingOrderIncomplete`, `ReadingOrderDuplicate`, `EdgeOutOfBounds`, `CycleDetected`

### Identity Scheme

| Type | Formula | Prefix | Crate |
|------|---------|--------|-------|
| `DocId` | `blake3(content)` | `doc_` | `shiro-core::id` |
| `SegmentId` | `blake3(doc_id:index)` | `seg_` | `shiro-core::id` |
| `ConceptId` | `blake3(scheme_uri + label)` | `con_` | `shiro-core::taxonomy` |
| `RunId` | timestamp | `run_` | `shiro-core::id` |
| `GenerationId` | monotonic u64 | — | `shiro-core::id` |

### Document State Machine

`DocState` in `shiro-core::manifest`:

```
STAGED → INDEXING → READY
              ↓
           FAILED
any → DELETED
```

Documents are searchable **only** in `READY`.

## Architectural Boundaries

### Port Contracts (`shiro-core::ports`)

| Port | Implementors | Contract |
|------|-------------|----------|
| `Parser` | `MarkdownParser`, `PdfParser` (shiro-parse) | Deterministic: identical input → identical output |
| `Embedder` | `HttpEmbedder` (shiro-embed) | Deterministic: identical input → identical embedding. Provider-agnostic. |
| `VectorIndex` | `FlatIndex` (shiro-embed) | Idempotent upsert. Thread-safe (`Send + Sync`). |

### Storage Boundary

**Authoritative (SQLite via shiro-store)**: documents, segments, search_results, blobs, schema_meta, concepts, concept_relations, concept_closure, doc_concepts, enrichments, generations, active_generations.

**Derived (rebuildable)**: Tantivy BM25 index (shiro-index), FlatIndex vector store (shiro-embed).

### Embedding Boundary

Embedding providers are adapters behind the `Embedder` trait. The core is provider-agnostic. `HttpEmbedder` targets any OpenAI-compatible `/v1/embeddings` endpoint (including Ollama). No architectural dependency on any specific provider, model family, or runtime.

## Cross-Cutting Concerns

### Atomic Index Publish

Both FTS and vector indices use generation-based staging:
1. Build into staging directory tagged with `GenerationId`
2. Atomic rename: staging → live
3. Register active generation in SQLite

### Processing Fingerprints

`ProcessingFingerprint { parser_name, parser_version, segmenter_version }` tracks how a document was processed, separate from content identity (`DocId`). Stored in `documents.fingerprint`.

### Hybrid Retrieval (RRF)

`rrf(s) = Σ 1/(k + rank_S(s))`, k=60 (`RRF_K` in `shiro-sdk::fusion`). Tie-break: fused_score desc, segment_id asc. Degrades to BM25-only when no vector index is present.

### Context Expansion

Triggered via `--expand`. Alternates before/after from hit segment in reading order, bounded by `max_blocks` (default 12) and `max_chars` (default 8000).

### Locking

Single-writer PID file lock (`write.lock`). Reads are lock-free. No daemon required.

### JSON-Only CLI Contract

All stdout is JSON envelope `{ ok, command, result, next_actions }`. Logs → stderr via tracing. Exception: `completions` outputs raw shell script.

## Operational Layout

```
~/.shiro/                              # ShiroHome (--home or SHIRO_HOME override)
├── config.toml                        # Dotted-key TOML config
├── shiro.db                           # SQLite source of truth (WAL mode)
├── tantivy/                           # Live BM25 index
├── tantivy_staging/                   # Staging dir for atomic FTS rebuild
├── vectors/                           # Live FlatIndex (vectors.jsonl)
├── vectors_staging/                   # Staging dir for vector rebuild
└── lock/write.lock                    # Single-writer PID lock
```

## Related ADRs

Decision records live in [`docs/adr/`](adr/). Each is immutable except for status updates.

### Accepted

- [ADR-001: Document Graph IR over Tree Model](adr/001-document-graph-ir.md)
- [ADR-002: SQLite as Source of Truth](adr/002-sqlite-source-of-truth.md)
- [ADR-003: FlatIndex as Correctness Baseline](adr/003-flatindex-correctness-baseline.md)
- [ADR-004: Processing Fingerprints Separate from Content Identity](adr/004-processing-fingerprints.md)
- [ADR-005: Generation-Based Index Publish](adr/005-generation-based-index-publish.md)

### Proposed

- [ADR-006: Persist the Document Graph as a First-Class Stored Representation](adr/006-persist-document-graph.md)
- [ADR-007: Treat EntryPoint as the Primary Retrieval Result](adr/007-entrypoint-as-retrieval-primitive.md)
- [ADR-011: Embedding Providers Are Adapters](adr/011-embedding-providers-are-adapters.md)
- [ADR-012: Canonical Embedding Contract and Fingerprint](adr/012-canonical-embedding-contract.md)
- [ADR-014: Hybrid Retrieval Contract](adr/014-hybrid-retrieval-contract.md)
- [ADR-020: Every Write Has Provenance Metadata](adr/020-every-write-has-provenance.md)
- [ADR-021: Partition Memory into Trust Zones](adr/021-partition-memory-into-trust-zones.md)
- [ADR-027: Benchmarking Is a Release Gate](adr/027-benchmarking-is-a-release-gate.md)
- [ADR-029: Configuration Is Typed, Versioned, and Migrated](adr/029-configuration-typed-versioned-migrated.md)
- [ADR-031: npm Distribution Strategy](adr/031-npm-distribution-strategy.md)

### Planned (not yet written)

- ADR-008: Define the Canonical Retrieval Field Model
- ADR-009: Segments Are a Derived Operational View, Not the Canonical Semantic Unit
- ADR-010: SQLite Remains the Source of Truth; Search Indices Are Derived and Rebuildable
- ADR-013: FlatIndex Is the Ground-Truth Baseline; ANN Backends Must Beat It by Policy
- ADR-015: Query Routing Happens Before Fusion
- ADR-016: PDF Parsing Must Produce Structured Output with Confidence
- ADR-017: Processing Identity Is Separate from Content Identity
- ADR-018: Define Reprocessing Policy for Parser, Segmenter, Embedder, and Enrichment Drift
- ADR-019: Ingest Is Idempotent and Has Explicit Failure/Resume Semantics
- ADR-022: Trust and Provenance Are Retrieval Features, Not Just Audit Fields
- ADR-023: Agent-Generated Writes Require Promotion Rules
- ADR-024: Generation-Based Publish Is the Uniform Activation Model
- ADR-025: Define Scale Targets and Resource Budgets
- ADR-026: Observability Is a First-Class Architectural Concern
- ADR-028: Define the Official Evaluation Corpora
- ADR-030: Single Binary Is the Product Boundary
- ADR-032: Adaptive Retrieval Roadmap Is Phased
- ADR-033: Sync Readiness Without Premature CRDT Adoption
- ADR-034: Managed Forgetting Affects Caches and Ranking Priors, Not Canonical Truth

## References

- [CLI Reference](CLI.md)
- [MCP Guide](MCP.md)
