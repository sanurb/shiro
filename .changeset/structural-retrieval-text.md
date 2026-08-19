---
"@sanurb/shiro-cli": minor
---

feat: derive bounded, versioned retrieval text from canonical title and heading structure

Segments now retain source-faithful canonical bodies and separate retrieval text capped at 2 KiB. Markdown and Docling heading depths and section containment survive persistence, while BM25, vectors, and reranking consume deterministic title/heading context. Embedding fingerprints include the retrieval-text version so stale vectors fail closed.
