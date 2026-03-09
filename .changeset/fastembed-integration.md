---
"@sanurb/shiro-cli": minor
---

feat: production-ready FastEmbed integration with ADR-012 fingerprint enforcement

- Add `fingerprint()` to Embedder trait (ADR-012) — breaking change for implementors
- FlatIndex fingerprint sidecar: store, load, enforce mismatch as hard error
- Internalize fastembed model enums behind string-based config (ADR-011)
- Schema v6: reranker_score/rank columns in search_results
- Reranker stage in explain trace (ADR-014)
- Fingerprint enforcement on vector index open in CLI
- Hard errors on unknown model names (no silent fallbacks)
- 11 new tests across 4 crates
