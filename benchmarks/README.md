# Shiro judged benchmark manifests

`shiro benchmark <manifest.json>` implements the ADR-027 retrieval/rebuild gate.
The canonical schema is the schemars-derived `shiro_sdk::BenchmarkManifest`
(`schema_version: 1`), included by `shiro_sdk::spec::generate_schemas()`.
Unknown fields and unsupported versions are rejected. Use the
[`manifest.template.json`](manifest.template.json) shape and complete the
[`EVIDENCE_INTAKE.md`](EVIDENCE_INTAKE.md) checklist before treating any run as
release evidence; neither file contains judgments or claims.

A manifest freezes:

- corpus ID/version;
- every READY document's `doc_id`, source URI, source hash, and license;
- adjudicated graded judgments (`0..=3`) with assessor IDs and an evidence
  locator;
- the declared hardware profile;
- requested BM25/vector/hybrid/rerank controls;
- reviewed quality and latency thresholds.

The command scopes every query to exactly the listed documents, reports
candidate Recall@50, Precision/Recall/MRR/nDCG@10, deterministic paired
bootstrap confidence intervals against BM25, p50/p95/p99 search latency,
observed RSS, explain completeness, repeated-run determinism, and rankings
before/after a mandatory common-manifest rebuild.

A run is `insufficient_evidence` rather than `pass` unless the manifest has at
least 100 documents, 300 adjudicated queries, all requested pipelines, and a
hardware profile matching the observed OS/architecture. This repository does
not contain the human judgments or representative hardware declaration yet;
those must be supplied and reviewed rather than synthesized.

Thresholds live in each versioned corpus manifest so changing an acceptance
value changes the same reviewed artifact as its judgments and hardware profile.
CI must archive the JSON envelope and treat only `evidence_status: "pass"` as a
release-gate pass.
