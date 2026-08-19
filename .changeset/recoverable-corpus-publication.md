---
"@sanurb/shiro-cli": patch
---

fix: publish validated FTS and vector generations through one recoverable corpus manifest

Full reindex now preserves the previous complete view until immutable generation artifacts, sidecars, fingerprints, counts, and digests validate. One SQLite activation updates the common manifest and both index pointers. Incremental FTS writes deactivate stale vectors before publication. Successful activation also removes interrupted generation directories that no retained manifest references while preserving generation audit rows.
