//! Baseline PDF parser using pdf-extract.

use shiro_core::ir::{Document, LossKind, Metadata, ParseLoss};
use shiro_core::ports::Parser;
use shiro_core::{DocId, ShiroError};

use crate::build_paragraph_block_graph;

pub struct PdfParser;

impl Parser for PdfParser {
    fn name(&self) -> &str {
        "pdf"
    }

    fn version(&self) -> u32 {
        1
    }

    fn parse(&self, source_uri: &str, content: &[u8]) -> Result<Document, ShiroError> {
        let text =
            pdf_extract::extract_text_from_mem(content).map_err(|e| ShiroError::ParsePdf {
                message: format!("PDF extraction failed: {e}"),
            })?;

        let id = DocId::from_content(content);
        let source_hash = blake3::hash(content).to_hex().to_string();
        let mut losses = Vec::new();

        if content.len() > 10_000 && text.len() < 100 {
            losses.push(ParseLoss {
                kind: LossKind::Image,
                span: None,
                message: "PDF may contain scanned images; text extraction may be incomplete"
                    .to_string(),
            });
        }

        let title = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(|l| {
                if l.len() > 200 {
                    l[..200].to_string()
                } else {
                    l.to_string()
                }
            });

        let blocks = build_paragraph_block_graph(&text);

        Ok(Document {
            id,
            canonical_text: text,
            rendered_text: None,
            metadata: Metadata {
                title,
                source_uri: source_uri.to_string(),
                source_hash,
            },
            blocks,
            losses,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_invalid_pdf() {
        let parser = PdfParser;
        let result = parser.parse("test.pdf", b"not a real pdf");
        assert!(result.is_err());
    }

    #[test]
    fn test_pdf_parser_name() {
        assert_eq!(PdfParser.name(), "pdf");
    }
}
