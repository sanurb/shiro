# Shiro pdf-inspector probe

Evaluation-only adapter for [`firecrawl/pdf-inspector`](https://github.com/firecrawl/pdf-inspector).
It emits routing and layout evidence as versioned JSON and never creates or publishes a Shiro
canonical document.

## Pin and toolchain boundary

- `pdf-inspector` crate: `1.14.2`
- audited source revision: `2543abe3715848589903754f30b5dca54f6b33a6`
- tool MSRV: Rust 1.88
- Shiro workspace MSRV: Rust 1.75 (unchanged)

This directory declares its own Cargo workspace because `pdf-inspector` depends on `lopdf 0.42`
and the executable lockfile uses a Rust 1.88 dependency graph, which cannot preserve Shiro's
current minimum Rust version. The probe is not linked into the Shiro
binary and is not a production parser profile.

## Run

```bash
cargo run --locked --manifest-path tools/pdf-inspector-probe/Cargo.toml -- document.pdf
```

Stdout contains one discriminated JSON envelope. Stderr is reserved for protocol-write failures.
The probe reads at most 50 MiB and emits at most 10,000 page records. It performs full-page detector
scanning and layout analysis without generating or returning Markdown or extracted document text.

Success:

```json
{
  "status": "success",
  "report": {
    "schema_version": 1,
    "source": { "blake3": "...", "byte_len": 1234 },
    "inspector": {
      "crate_version": "1.14.2",
      "source_revision": "2543abe3715848589903754f30b5dca54f6b33a6",
      "adapter_version": 1,
      "profile": "full_page_layout_analysis"
    },
    "inspection": {
      "pdf_type": "text_based",
      "page_count": 1,
      "detector_confidence": 1.0,
      "processing_time_ms": 4,
      "has_encoding_issues": false,
      "has_complex_layout": false,
      "recommended_route": "native_extract",
      "pages": []
    }
  }
}
```

Failure:

```json
{
  "status": "error",
  "error": {
    "code": "not_pdf",
    "message": "PDF inspector probe analysis failed: ..."
  }
}
```

## Deliberate exclusions

- no OCR, network fallback, or model download;
- no pdf-inspector Markdown as canonical data;
- no Shiro index publication;
- no implicit replacement of `pdf-extract` or Docling;
- no claim that detector confidence is parse-quality confidence.

Promotion requires corpus, fidelity, retrieval, resource, cancellation, supply-chain, and
minimum-Rust gates to be met and recorded in an ADR before this probe becomes a parser profile.
