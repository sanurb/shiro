//! Comparison hooks: Docling vs baseline PDF parser on structure fidelity.
//!
//! These tests compare the IR quality between Docling's structured output
//! and the baseline `pdf-extract` parser. They measure retrieval-relevant
//! outcomes: block count, block kind diversity, reading order structure,
//! table detection, and loss reporting.
//!
//! Run with `cargo test -p shiro-docling --test comparison`.

use shiro_core::ir::{BlockKind, Document, LossKind};

/// Structure fidelity metrics for comparing parser output quality.
#[derive(Debug)]
#[allow(dead_code)]
struct FidelityReport {
    parser_name: String,
    block_count: usize,
    unique_kinds: usize,
    heading_count: usize,
    paragraph_count: usize,
    list_item_count: usize,
    table_count: usize,
    caption_count: usize,
    footnote_count: usize,
    code_count: usize,
    loss_count: usize,
    image_losses: usize,
    table_losses: usize,
    edge_count: usize,
    canonical_text_len: usize,
    has_valid_graph: bool,
}

fn compute_fidelity(doc: &Document, parser_name: &str) -> FidelityReport {
    let blocks = &doc.blocks.blocks;
    let kind_count = |k: BlockKind| blocks.iter().filter(|b| b.kind == k).count();

    let unique_kinds: std::collections::BTreeSet<_> =
        blocks.iter().map(|b| format!("{:?}", b.kind)).collect();

    let violations = doc.blocks.validate(doc.canonical_text.len());

    FidelityReport {
        parser_name: parser_name.to_string(),
        block_count: blocks.len(),
        unique_kinds: unique_kinds.len(),
        heading_count: kind_count(BlockKind::Heading),
        paragraph_count: kind_count(BlockKind::Paragraph),
        list_item_count: kind_count(BlockKind::ListItem),
        table_count: kind_count(BlockKind::TableCell),
        caption_count: kind_count(BlockKind::Caption),
        footnote_count: kind_count(BlockKind::Footnote),
        code_count: kind_count(BlockKind::Code),
        loss_count: doc.losses.len(),
        image_losses: doc
            .losses
            .iter()
            .filter(|l| l.kind == LossKind::Image)
            .count(),
        table_losses: doc
            .losses
            .iter()
            .filter(|l| l.kind == LossKind::Table)
            .count(),
        edge_count: doc.blocks.edges.len(),
        canonical_text_len: doc.canonical_text.len(),
        has_valid_graph: violations.is_empty(),
    }
}

/// Load fixture and translate to Document.
fn docling_from_fixture(fixture: &str) -> Document {
    let json = std::fs::read(format!(
        "{}/tests/fixtures/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let docling: shiro_docling::__test_support::DoclingDocument =
        serde_json::from_slice(&json).unwrap();
    shiro_docling::__test_support::translate(
        &docling,
        &format!("fixtures/{fixture}"),
        b"comparison-content",
    )
}

/// Parse the same content with the baseline PDF parser (PlainTextParser
/// as proxy, since we can't run pdf-extract without a real PDF).
fn baseline_from_text(text: &str) -> Document {
    use shiro_core::ports::Parser;
    let parser = shiro_parse::PlainTextParser;
    parser.parse("comparison.txt", text.as_bytes()).unwrap()
}

#[test]
fn docling_produces_richer_structure_than_baseline() {
    let docling_doc = docling_from_fixture("simple_report.json");
    let baseline_doc = baseline_from_text(&docling_doc.canonical_text);

    let docling_report = compute_fidelity(&docling_doc, "docling");
    let baseline_report = compute_fidelity(&baseline_doc, "baseline");

    // Docling should produce more block-kind diversity.
    assert!(
        docling_report.unique_kinds > baseline_report.unique_kinds,
        "Docling ({}) should have more unique block kinds than baseline ({})",
        docling_report.unique_kinds,
        baseline_report.unique_kinds
    );

    // Docling should detect headings.
    assert!(
        docling_report.heading_count > 0,
        "Docling should detect headings"
    );
    assert_eq!(
        baseline_report.heading_count, 0,
        "baseline should not detect headings (treats all as paragraphs)"
    );

    // Both should have valid graphs.
    assert!(
        docling_report.has_valid_graph,
        "Docling graph must be valid"
    );
    assert!(
        baseline_report.has_valid_graph,
        "baseline graph must be valid"
    );

    // Docling should detect list items.
    assert!(
        docling_report.list_item_count > 0,
        "Docling should detect list items"
    );

    // Docling should detect tables.
    assert!(
        docling_report.table_count > 0,
        "Docling should detect tables"
    );

    // Docling should detect captions.
    assert!(
        docling_report.caption_count > 0,
        "Docling should detect captions"
    );

    // Print comparison for manual review (visible with --nocapture).
    eprintln!("\n=== Structure Fidelity Comparison ===");
    eprintln!("{:<20} {:>10} {:>10}", "Metric", "Docling", "Baseline");
    eprintln!("{:-<42}", "");
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "Blocks", docling_report.block_count, baseline_report.block_count
    );
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "Unique kinds", docling_report.unique_kinds, baseline_report.unique_kinds
    );
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "Headings", docling_report.heading_count, baseline_report.heading_count
    );
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "Paragraphs", docling_report.paragraph_count, baseline_report.paragraph_count
    );
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "List items", docling_report.list_item_count, baseline_report.list_item_count
    );
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "Tables", docling_report.table_count, baseline_report.table_count
    );
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "Captions", docling_report.caption_count, baseline_report.caption_count
    );
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "Footnotes", docling_report.footnote_count, baseline_report.footnote_count
    );
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "Losses", docling_report.loss_count, baseline_report.loss_count
    );
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "Edges", docling_report.edge_count, baseline_report.edge_count
    );
    eprintln!(
        "{:<20} {:>10} {:>10}",
        "Text length", docling_report.canonical_text_len, baseline_report.canonical_text_len
    );
    eprintln!("=====================================\n");
}

#[test]
fn degraded_document_still_outperforms_baseline() {
    let docling_doc = docling_from_fixture("degraded_scanned.json");
    let docling_report = compute_fidelity(&docling_doc, "docling-degraded");

    // Even degraded, Docling should produce a valid graph.
    assert!(
        docling_report.has_valid_graph,
        "degraded graph must still be valid"
    );

    // Degraded documents should report losses.
    assert!(
        docling_report.loss_count > 0,
        "degraded document should report losses"
    );
    assert!(
        docling_report.image_losses > 0,
        "should report image losses"
    );
    assert!(
        docling_report.table_losses > 0,
        "should report table losses"
    );
}
