---
"@sanurb/shiro-cli": minor
---

Publish configured embeddings for changed documents in bounded batches. Incremental publication reuses fingerprint-compatible unchanged vectors, validates complete FTS/vector generations and artifact digests, and atomically commits canonical staging, document readiness, and both index pointers. Embedding failures leave the prior canonical document and complete searchable manifest active.
