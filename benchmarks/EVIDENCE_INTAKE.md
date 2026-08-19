# Release-gate evidence intake

This checklist identifies the external evidence required before Shiro may turn an
`insufficient_evidence` benchmark result into a release claim. It is an intake
contract, not benchmark evidence. Do not replace missing observations with
synthetic documents, generated judgments, guessed hardware, or copied thresholds.

Start from [`manifest.template.json`](manifest.template.json), replace every
`REPLACE_...` value, and set reviewed thresholds. The template is intentionally
incomplete and must not be used as a release result.

## 1. Freeze the corpus

- Select at least **100 real PDF/Markdown documents** representative of the target
  workload, including its actual scan, layout, language, code/table, and size mix.
- Record each stored `doc_id`, canonical source URI, BLAKE3 source hash, and a
  reviewed license or explicit evaluation permission.
- Archive the exact source bytes outside Git when redistribution is prohibited.
- Assign an immutable `corpus_id` and `corpus_version`; any document, license,
  parser-policy, or judgment change requires a new version.
- Verify every manifest document is READY and its recorded source hash matches the
  authoritative store before adjudication begins.

## 2. Capture the workload

- Collect at least **300 real or human-authored queries** from the intended task
  distribution; do not derive all queries from document headings or summaries.
- Preserve frequencies or sampling rules separately so the benchmark distribution
  can be reviewed.
- Include exact names/numbers, ambiguous terminology, multi-document synthesis,
  structural/page questions, and negative/no-answer cases in proportions supported
  by the real workload.
- Give every query a stable `query_id`; edits create a new corpus version.

## 3. Adjudicate relevance

- Use pseudonymous, stable assessor IDs and retain assessor instructions.
- Grade document relevance on the manifest's `0..=3` scale and attach a canonical
  evidence locator (prefer a `blk_` handle plus page/region when available).
- Mark `adjudicated: true` only after conflicts are resolved under a documented
  procedure. Preserve raw independent judgments outside the resolved manifest.
- Review all critical exact-number/name queries and all disagreements; do not infer
  judgments from the current ranker.
- Record inter-assessor agreement and unresolved exclusions in the corpus review
  notes even though those values are not benchmark-manifest fields.

## 4. Declare representative hardware

Capture the exact machine used for the release gate:

- OS and architecture (`uname -s`, `uname -m` or platform equivalents);
- CPU model and logical-core count (`lscpu`, `sysctl -n machdep.cpu.brand_string`,
  or the supported-platform equivalent);
- installed physical memory in bytes;
- power/performance mode, model cache state, and cold/warm run policy in the
  archived run notes.

Set `hardware_profile` only from observed values. A maintainer must explicitly
approve that profile as representative; matching the current developer machine is
not sufficient by itself.

## 5. Review thresholds before running

Thresholds are product decisions, not values selected to make a completed run pass.
Review and freeze:

- minimum candidate Recall@50;
- minimum final nDCG@10 and MRR@10;
- maximum search p95 latency;
- whether the paired nDCG CI lower-bound gate is required.

Commit threshold review notes with the corpus version. Changing a threshold requires
review and a new immutable manifest version.

## 6. Execute and archive

```bash
CARGO_HOME=/tmp/shiro-cargo-home cargo build --locked --release
./target/release/shiro benchmark benchmarks/<reviewed-manifest>.json \
  --warmup-runs 1 --measured-runs 3 > benchmark-result.json
```

Archive together:

- reviewed manifest and its BLAKE3/SHA-256 digest;
- JSON command envelope;
- Shiro commit and binary digest;
- configuration with secrets removed;
- model/fingerprint artifacts;
- hardware and workload-distribution notes;
- stderr logs and CI job URL.

Only `evidence_status: "pass"` on the reviewed representative profile is release
evidence. `insufficient_evidence` and `fail` are never waivable by prose claims.

## 7. Separate evidence still required

The judged benchmark does not replace these independent gates:

- crash injection at every publication boundary on every supported filesystem/OS;
- supported-platform MCP and CLI conformance;
- measured query-cache hit rate before adding a cache;
- user-task call/byte/time evidence before adding an optional human client;
- provider-specific region, retention, consent, and account terms before enabling
  remote model calls.
