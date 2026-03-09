//! Fixture-backed integration tests for the Docling → shiro IR translation.
//!
//! These tests validate:
//! - Reading order preservation from Docling's body tree
//! - Block kind mapping (heading, paragraph, list_item, table, caption, footnote)
//! - Table content rendering as pipe-delimited text
//! - ParseLoss emission for images and degraded tables
//! - Span validity (every span indexes correctly into canonical_text)
//! - BlockGraph invariant validation
//! - Stable JSON serialization (deterministic output)
//! - Title extraction strategies

// We test the translation layer directly by deserializing fixture JSON
// into the private schema types, which is possible because the integration
// test links against the crate and can use `pub(crate)` re-exports via
// a test-only helper.

/// Load a fixture file from tests/fixtures/.
fn load_fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"))
}

mod simple_report {
    use super::*;
    use shiro_core::ir::{BlockKind, LossKind};

    fn parse_fixture() -> shiro_core::ir::Document {
        // Use DoclingParser with a pre-built JSON file.
        // Since we can't invoke docling CLI in tests, we test the translation
        // layer through the public types by constructing a Document from fixture JSON.
        let json = load_fixture("simple_report.json");
        let docling: shiro_docling::__test_support::DoclingDocument =
            serde_json::from_slice(&json).unwrap();
        shiro_docling::__test_support::translate(
            &docling,
            "fixtures/simple_report.pdf",
            b"fake-pdf-bytes-for-content-addressing",
        )
    }

    #[test]
    fn reading_order_matches_body_tree() {
        let doc = parse_fixture();
        // Body tree order: heading, paragraph, [picture→loss], heading, paragraph,
        // list_item, list_item, caption, table, heading, paragraph, footnote
        // Picture is omitted (loss), so we get 11 blocks.
        assert_eq!(
            doc.blocks.blocks.len(),
            11,
            "expected 11 blocks (picture omitted as loss)"
        );
        assert_eq!(doc.blocks.reading_order.len(), 11);

        // Verify sequential reading order.
        for (i, idx) in doc.blocks.reading_order.iter().enumerate() {
            assert_eq!(idx.0, i, "reading_order[{i}] should be {i}");
        }
    }

    #[test]
    fn block_kinds_are_correct() {
        let doc = parse_fixture();
        let kinds: Vec<_> = doc.blocks.blocks.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            vec![
                BlockKind::Heading,   // Executive Summary
                BlockKind::Paragraph, // This report...
                BlockKind::Heading,   // Methodology
                BlockKind::Paragraph, // We employed...
                BlockKind::ListItem,  // Survey of 500...
                BlockKind::ListItem,  // Financial data...
                BlockKind::Caption,   // Table 1: ...
                BlockKind::TableCell, // table content
                BlockKind::Heading,   // Conclusions
                BlockKind::Paragraph, // Overall performance...
                BlockKind::Footnote,  // Based on fiscal year...
            ],
            "block kinds do not match expected order"
        );
    }

    #[test]
    fn footnote_in_body_produces_block() {
        let doc = parse_fixture();
        let last = doc.blocks.blocks.last().expect("should have blocks");
        assert_eq!(
            last.kind,
            BlockKind::Footnote,
            "last block in body should be the footnote"
        );
        assert!(
            last.canonical_text.contains("fiscal year"),
            "footnote text should be preserved"
        );
    }

    #[test]
    fn spans_index_into_canonical_text() {
        let doc = parse_fixture();
        for (i, block) in doc.blocks.blocks.iter().enumerate() {
            let extracted = &doc.canonical_text[block.span.start()..block.span.end()];
            assert_eq!(
                extracted, block.canonical_text,
                "block {i} span does not match canonical_text"
            );
        }
    }

    #[test]
    fn block_graph_validates() {
        let doc = parse_fixture();
        let violations = doc.blocks.validate(doc.canonical_text.len());
        assert!(
            violations.is_empty(),
            "BlockGraph validation failed: {violations:?}"
        );
    }

    #[test]
    fn reads_before_edges_form_chain() {
        let doc = parse_fixture();
        let n = doc.blocks.blocks.len();
        assert_eq!(
            doc.blocks.edges.len(),
            n - 1,
            "should have n-1 ReadsBefore edges"
        );
        for (i, edge) in doc.blocks.edges.iter().enumerate() {
            assert_eq!(edge.from.0, i, "edge {i} from should be {i}");
            assert_eq!(edge.to.0, i + 1, "edge {i} to should be {}", i + 1);
            assert_eq!(
                edge.relation,
                shiro_core::Relation::ReadsBefore,
                "all edges should be ReadsBefore"
            );
        }
    }

    #[test]
    fn image_produces_parse_loss() {
        let doc = parse_fixture();
        let image_losses: Vec<_> = doc
            .losses
            .iter()
            .filter(|l| l.kind == LossKind::Image)
            .collect();
        assert_eq!(
            image_losses.len(),
            1,
            "one picture in fixture should produce one Image loss"
        );
        assert!(image_losses[0].message.contains("picture"));
    }

    #[test]
    fn table_rendered_as_text() {
        let doc = parse_fixture();
        let table_block = doc
            .blocks
            .blocks
            .iter()
            .find(|b| b.kind == BlockKind::TableCell)
            .expect("should have a table block");
        assert!(
            table_block.canonical_text.contains("Department"),
            "table should contain header text"
        );
        assert!(
            table_block.canonical_text.contains("Engineering"),
            "table should contain row data"
        );
        assert!(
            table_block.canonical_text.contains("92%"),
            "table should contain cell values"
        );
    }

    #[test]
    fn title_extracted_from_name() {
        let doc = parse_fixture();
        assert_eq!(
            doc.metadata.title.as_deref(),
            Some("simple_report"),
            "title should come from Docling's 'name' field"
        );
    }

    #[test]
    fn doc_id_is_content_addressed() {
        let doc = parse_fixture();
        assert!(
            doc.id.as_str().starts_with("doc_"),
            "DocId should have doc_ prefix"
        );
        // Same content → same ID.
        let doc2 = parse_fixture();
        assert_eq!(
            doc.id, doc2.id,
            "identical content must produce identical DocId"
        );
    }

    #[test]
    fn canonical_text_is_deterministic() {
        let doc1 = parse_fixture();
        let doc2 = parse_fixture();
        assert_eq!(
            doc1.canonical_text, doc2.canonical_text,
            "translation must be deterministic"
        );
    }

    #[test]
    fn json_serialization_is_stable() {
        let doc1 = parse_fixture();
        let doc2 = parse_fixture();
        let json1 = serde_json::to_string(&doc1).unwrap();
        let json2 = serde_json::to_string(&doc2).unwrap();
        assert_eq!(json1, json2, "JSON serialization must be deterministic");
    }
}

mod degraded_scanned {
    use super::*;
    use shiro_core::ir::LossKind;

    fn parse_fixture() -> shiro_core::ir::Document {
        let json = load_fixture("degraded_scanned.json");
        let docling: shiro_docling::__test_support::DoclingDocument =
            serde_json::from_slice(&json).unwrap();
        shiro_docling::__test_support::translate(
            &docling,
            "fixtures/degraded_scanned.pdf",
            b"fake-scanned-content",
        )
    }

    #[test]
    fn degraded_table_produces_loss() {
        let doc = parse_fixture();
        let table_losses: Vec<_> = doc
            .losses
            .iter()
            .filter(|l| l.kind == LossKind::Table)
            .collect();
        assert_eq!(
            table_losses.len(),
            1,
            "table without data should produce Table loss"
        );
    }

    #[test]
    fn multiple_pictures_produce_losses() {
        let doc = parse_fixture();
        let image_losses: Vec<_> = doc
            .losses
            .iter()
            .filter(|l| l.kind == LossKind::Image)
            .collect();
        assert_eq!(
            image_losses.len(),
            2,
            "two pictures should produce two Image losses"
        );
    }

    #[test]
    fn ocr_text_still_indexed() {
        let doc = parse_fixture();
        assert_eq!(doc.blocks.blocks.len(), 1, "one paragraph of OCR text");
        assert!(
            doc.blocks.blocks[0]
                .canonical_text
                .contains("Partially recognized"),
            "OCR text should be preserved"
        );
    }

    #[test]
    fn graph_validates_despite_losses() {
        let doc = parse_fixture();
        let violations = doc.blocks.validate(doc.canonical_text.len());
        assert!(
            violations.is_empty(),
            "graph should validate even with degraded input: {violations:?}"
        );
    }
}

mod empty_document {
    use super::*;

    fn parse_fixture() -> shiro_core::ir::Document {
        let json = load_fixture("empty_document.json");
        let docling: shiro_docling::__test_support::DoclingDocument =
            serde_json::from_slice(&json).unwrap();
        shiro_docling::__test_support::translate(&docling, "fixtures/empty.pdf", b"empty")
    }

    #[test]
    fn empty_document_produces_empty_graph() {
        let doc = parse_fixture();
        assert!(doc.canonical_text.is_empty());
        assert!(doc.blocks.blocks.is_empty());
        assert!(doc.blocks.edges.is_empty());
        assert!(doc.blocks.reading_order.is_empty());
        assert!(doc.losses.is_empty());
    }

    #[test]
    fn empty_document_validates() {
        let doc = parse_fixture();
        let violations = doc.blocks.validate(doc.canonical_text.len());
        assert!(violations.is_empty());
    }
}

mod parser_identity {
    use shiro_core::ports::Parser;

    #[test]
    fn fingerprint_is_stable() {
        let parser = shiro_docling::DoclingParser::new();
        let fp = shiro_core::ProcessingFingerprint::new(
            parser.name(),
            parser.version(),
            shiro_parse::SEGMENTER_VERSION,
        );
        assert_eq!(fp.parser_name, "docling");
        assert!(fp.parser_version >= 1);
        // Hash must be deterministic.
        let fp2 = shiro_core::ProcessingFingerprint::new(
            parser.name(),
            parser.version(),
            shiro_parse::SEGMENTER_VERSION,
        );
        assert_eq!(fp.content_hash(), fp2.content_hash());
    }
}
