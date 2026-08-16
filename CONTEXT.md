# Shiro Knowledge Engine

Shiro turns local source documents into stable, explainable reading positions while keeping canonical knowledge separate from rebuildable retrieval views.

## Documents

**Document**:
Content-addressed source material with canonical text and a Document Graph. Its identity changes only when the source bytes change.
_Avoid_: File, record

**Document Graph**:
The canonical structural representation of a Document, including blocks, relationships, and reading order.
_Avoid_: Parse tree, block list

**Segment**:
A derived retrieval view over a span of a Document. Segment boundaries optimize retrieval and are not public reading positions.
_Avoid_: Chunk, passage

**Ingestion**:
The lifecycle that turns source bytes into a ready Document with canonical structure, processing identity, and derived retrieval views.
_Avoid_: Upload, import

**Processing Fingerprint**:
The identity of the parser and segmenter behavior that produced a Document's current derived retrieval views. It is separate from Document identity.
_Avoid_: Document version

## Retrieval

**EntryPoint**:
The best canonical block position in a Document from which to begin reading for a query, with bounded surrounding context.
_Avoid_: Search hit, segment result

**Retrieval Evidence**:
The complete query-time provenance behind an EntryPoint, including contributing retrieval sources, ranks, scores, generations, fusion, and reranking.
_Avoid_: Debug data, score details

**Embedding Fingerprint**:
The non-secret identity of the embedding configuration that defines one compatible vector space. Vectors with different or unknown fingerprints are incompatible.
_Avoid_: Model name, vector version
