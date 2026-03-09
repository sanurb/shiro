# shiro-docling

Docling-based structured PDF parser adapter. Translates Docling's DoclingDocument JSON into shiro's canonical IR.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Docling serde types | `src/schema.rs` | Private types for DoclingDocument JSON — NEVER leak to other crates |
| Parser implementation | `src/parser.rs` | DoclingParser implements `shiro_core::ports::Parser` |
| IR translation | `src/translate.rs` (604 lines) | DoclingDocument → Document/BlockGraph conversion — complexity hotspot |
| Fixture-backed tests | `tests/translation_tests.rs` | Loads JSON fixtures, validates block count, reading order, spans, BlockGraph |
| Structure comparison | `tests/comparison.rs` | Docling IR vs shiro-parse PlainTextParser fidelity comparison |
| Test fixtures | `tests/fixtures/` | `simple_report.json`, `degraded_scanned.json`, `empty_document.json` |

## DESIGN CONSTRAINTS

- **Docling types are crate-private** — `schema.rs` is `pub(crate)`, never re-exported. `#[allow(dead_code)]` for forward-compat with unread fields.
- **Subprocess boundary** — Docling is invoked as `docling <file> --to json --output <tmpdir>`
- **Deterministic** — same input → same output. Docling version is pinned in parser identity
- **Degraded handling** — partial/uncertain structure → ParseLoss records, not errors
- **No core coupling** — Docling concepts (BoundingBox, RefItem, etc.) never appear in shiro-core types

## TEST SUPPORT

`pub mod __test_support` in `lib.rs` (#[doc(hidden)]) re-exports `DoclingDocument` and `translate()` for fixture tests without requiring the docling subprocess. This is the only non-`#[cfg(test)]` test surface in the project.

## TESTING

- `tests/translation_tests.rs`: fixture-backed, validates block count, reading order, span validity, BlockGraph invariants, table rendering, ParseLoss emission, stable JSON serialization
- `tests/comparison.rs`: FidelityReport — block/kind diversity, heading/table counts, loss counts, graph validity vs PlainTextParser baseline
- Fixtures loaded via `CARGO_MANIFEST_DIR`
- `cargo test -p shiro-docling` to run
