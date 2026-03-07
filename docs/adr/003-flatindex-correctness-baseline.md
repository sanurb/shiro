# ADR-003: FlatIndex as Correctness Baseline

**Status:** Accepted
**Date:** 2026-03-07

## Context

Vector search is a required capability for semantic retrieval. Approximate Nearest Neighbor (ANN) libraries (HNSW, IVF-flat, ScaNN) offer sub-linear query time but introduce recall variance: results differ from exact cosine ranking by design. Without a known-correct implementation to compare against, it is impossible to measure recall quality for any ANN backend — you cannot know how much relevance you are losing.

Shiro needs a **ground truth** (the reference result set against which all approximations are measured) implementation: a brute-force exact search that defines what "correct vector search results" means for a given query and vector set.

## Decision

**FlatIndex** is the ground-truth vector search implementation and the default backend. It performs brute-force cosine similarity over all indexed vectors, producing exact results by construction.

Any future ANN backend must achieve recall >= 0.95 against FlatIndex results on the project's benchmark suite before it can replace FlatIndex as the default in production.

**Boundary:** This ADR decides the correctness baseline for vector search and the gate for introducing approximate backends. It does not decide the embedding model, the embedding API, the vector storage format, or the hybrid ranking strategy.

**What is canonical:** FlatIndex results are the definition of correct ranking for a given query and vector set. They are not measured against an external gold standard — FlatIndex *is* the gold standard within Shiro.

**What is derived:** Any ANN index's results are an approximation of FlatIndex results. Their quality is measured as recall relative to FlatIndex.

**What is allowed:** New vector index backends may be introduced behind the vector index trait boundary. They must demonstrate >= 0.95 recall against FlatIndex on the project benchmark suite. FlatIndex may be replaced as the default once an ANN backend passes this gate.

**What is forbidden:** An ANN backend must not become the default without passing the recall gate. The recall threshold must not be lowered without an explicit ADR revision. Consumers must not assume sub-linear query latency — the default backend is O(n).

### Architecture Invariants

- FlatIndex results are the definition of correct vector ranking. Any approximation is measured against FlatIndex, not against an external relevance benchmark. If an ANN backend disagrees with FlatIndex, the ANN backend is wrong by definition (within the scope of this system's correctness model).
- The recall gate (>= 0.95) applies to the project's benchmark suite. If no benchmark suite exists yet, FlatIndex remains the default — the gate cannot be passed without a measurement. This means the decision holds even in the absence of benchmarking infrastructure; it simply cannot be superseded until that infrastructure exists.
- The vector index trait boundary is the abstraction seam. All consumers go through this trait; swapping FlatIndex for an ANN backend is invisible to callers.
- Vector data is derived from the canonical store (see ADR-002). FlatIndex can be rebuilt from source content and embeddings without data loss.

### Deliberate Absences

- **Benchmark suite composition** is not specified. Which queries, which documents, and what corpus size constitute the benchmark is left to the implementer of any future ANN backend.
- **Who runs the benchmark** is not decided. Whether it is CI, a manual process, or a release gate is an implementation decision.
- **Storage format** for vectors is not prescribed. FlatIndex may use any format that roundtrips correctly and detects corruption.
- **Embedding model and API** are not decided here. The choice of embedding provider, model, dimensions, and authentication is orthogonal to the correctness baseline.
- **Hybrid ranking strategy** (e.g., how BM25 and vector results are fused via RRF) is out of scope for this ADR.

## Consequences

- **Measurable recall quality.** Any future ANN backend has a concrete, automated bar to clear. This prevents "it feels about as good" from being the acceptance criterion for a backend that silently drops relevant results. Users benefit because search quality regressions are caught before deployment.
- **O(n) search latency.** Brute-force search scales linearly with the number of indexed vectors. At the target scale (thousands of documents, tens of thousands of segments), this is acceptable — queries complete in milliseconds. However, **O(n) latency is the primary scaling bottleneck.** At roughly 100K–500K vectors, brute-force search will cross the threshold where query latency becomes user-perceptible (hundreds of milliseconds to seconds). This is the expected crossover point where an ANN backend becomes necessary, not optional.
- **No recall variance in default configuration.** Users get exact cosine results by default. There are no tuning knobs to accidentally misconfigure, no index build parameters that silently degrade quality. The default is correct; speed is the trade-off.
- **ANN introduction is gated, not blocked.** The decision does not prevent ANN backends — it requires them to prove their recall before becoming the default. This means the path from "FlatIndex is too slow" to "ANN backend in production" requires building benchmark infrastructure first, which is an upfront investment.
- **Testing without external dependencies.** A stub embedder (producing deterministic vectors without calling an external API) enables unit testing of the entire vector search pipeline without network access or API keys. This reduces test infrastructure cost and eliminates flaky tests from network issues.
- **Rebuild cost for large corpora.** Because vector data is derived (ADR-002), rebuilding FlatIndex requires re-embedding all segments. For large corpora, this involves significant API call cost and wall-clock time. This cost scales with corpus size and is a real operational concern for recovery scenarios.

## Alternatives Considered

- **Start with HNSW (e.g., via an HNSW library):** Would provide sub-linear query time from day one, which matters if the corpus is large at launch. However, without a ground-truth implementation, there is no way to measure recall — any quality regression would be undetectable. Choosing this would mean faster queries but no way to know if you are returning the right results. Rejected as the initial implementation; acceptable as a future backend once FlatIndex establishes the baseline.
- **No vector search:** Removes semantic retrieval entirely, leaving only BM25 keyword search. This is genuinely simpler — no embedding API dependency, no vector storage, no recall concerns. However, keyword search alone misses semantic matches (synonyms, paraphrases, conceptual similarity), which are a core value proposition of hybrid search. Users searching for "climate impact" would not find documents about "environmental consequences." Rejected because hybrid BM25 + vector search (via RRF fusion) is a core product capability.
- **External vector database (e.g., Qdrant, Milvus):** Would offload vector storage and search to a purpose-built system with built-in ANN support and horizontal scaling. However, this violates Shiro's local-first, single-binary architecture. Users would need to run and maintain a separate service, defeating the "just works" deployment model. Rejected because operational simplicity is a core design constraint.

## Non-Goals

- Not optimizing for million-scale vector corpora in the initial version. Target scale is thousands of documents.
- No GPU acceleration. FlatIndex runs on CPU; this is sufficient at target scale.
- No distributed vector index. All vectors are local to the Shiro home directory.
