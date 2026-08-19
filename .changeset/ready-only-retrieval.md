---
"@sanurb/shiro-cli": patch
---

fix: enforce authoritative READY-only eligibility across BM25, vector, hybrid, reranking, context expansion, and explain

fix: persist immutable per-search explain snapshots so repeated queries cannot alias stale evidence

fix: reprocess READY documents when parser or segmenter fingerprints drift

fix: atomically retain raw source artifacts, parser losses, trust classification, and immutable write provenance
